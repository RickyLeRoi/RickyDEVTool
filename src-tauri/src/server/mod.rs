mod ws;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, HeaderMap, Request, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::json;

use crate::config::ConfigHandle;
use crate::events::EventBus;
use crate::netinfo;
use crate::poller::PollerRegistry;

#[derive(RustEmbed)]
#[folder = "../dist"]
struct Assets;

const PAIR_COOKIE: &str = "rdt";
const PORT_FALLBACK_RANGE: u16 = 10;

#[derive(Clone)]
pub struct ServerState {
    pub config: ConfigHandle,
    pub bus: EventBus,
    pub pollers: Arc<PollerRegistry>,
    pub port: u16,
    /// Cache della discovery tool: invalidata su refresh esplicito o override.
    pub tools_cache: Arc<tokio::sync::Mutex<Option<Vec<crate::adapters::tools::DiscoveredTool>>>>,
    pub tasks: Arc<crate::tasks::TaskRegistry>,
    pub alerts: Arc<crate::alerts::AlertService>,
}

/// Le azioni che modificano il sistema sono locali, oppure LAN se l'utente
/// ha attivato esplicitamente il controllo remoto.
fn write_allowed(state: &ServerState, peer: SocketAddr) -> bool {
    peer.ip().is_loopback() || state.config.get().remote_control_enabled
}

#[derive(Clone, Copy, Debug)]
pub struct ServerInfo {
    pub port: u16,
    pub lan_enabled: bool,
}

/// Binda su config.port con fallback alle porte successive, poi serve in background.
pub async fn start(
    config: ConfigHandle,
    bus: EventBus,
    pollers: Arc<PollerRegistry>,
) -> anyhow::Result<ServerInfo> {
    let cfg = config.get();
    let bind_ip: IpAddr = if cfg.lan_enabled {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };

    let mut listener = None;
    let mut port = cfg.port;
    for candidate in cfg.port..cfg.port + PORT_FALLBACK_RANGE {
        match tokio::net::TcpListener::bind(SocketAddr::new(bind_ip, candidate)).await {
            Ok(l) => {
                listener = Some(l);
                port = candidate;
                break;
            }
            Err(e) => tracing::warn!(port = candidate, %e, "porta occupata, provo la successiva"),
        }
    }
    let listener = listener
        .ok_or_else(|| anyhow::anyhow!("nessuna porta libera tra {} e {}", cfg.port, cfg.port + PORT_FALLBACK_RANGE - 1))?;

    let tasks = Arc::new(crate::tasks::TaskRegistry::new(bus.clone()));
    let alerts = crate::alerts::AlertService::start(bus.clone());
    let state = ServerState {
        config,
        bus,
        pollers,
        port,
        tools_cache: Arc::new(tokio::sync::Mutex::new(None)),
        tasks,
        alerts,
    };

    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/lan", get(lan_info))
        .route("/api/lan/qr.svg", get(lan_qr))
        .route("/api/pair", post(pair))
        .route("/api/log", post(client_log))
        .route("/api/pollers/{topic}/interval", post(set_interval))
        .route("/api/processes/heavy", get(heavy_processes))
        .route("/api/processes/kill", post(kill_process))
        .route("/api/ports", get(list_ports))
        .route("/api/tools", get(list_tools))
        .route("/api/tools/{id}/launch", post(launch_tool))
        .route("/api/tools/{id}/path", post(set_tool_path))
        .route("/api/fs/dirs", get(fs_dirs))
        .route("/api/projects/scan", get(projects_scan))
        .route("/api/projects/pinned", get(pinned_get).post(pinned_set))
        .route("/api/git/info", get(git_info))
        .route("/api/git/fetch", post(git_fetch))
        .route("/api/git/pull", post(git_pull))
        .route("/api/git/branches", get(git_branches))
        .route("/api/git/checkout", post(git_checkout))
        .route("/api/node/info", get(node_info))
        .route("/api/node/pm", post(node_set_pm))
        .route("/api/node/run", post(node_run))
        .route("/api/tasks", get(tasks_list))
        .route("/api/tasks/{id}/stop", post(task_stop))
        .route("/api/tasks/clear-finished", post(tasks_clear_finished))
        .route("/api/dotnet/info", get(dotnet_info))
        .route("/api/dotnet/select", post(dotnet_select))
        .route("/api/dotnet/run", post(dotnet_run))
        .route("/api/services", get(services_get).post(services_upsert))
        .route("/api/services/{id}", axum::routing::delete(services_delete))
        .route("/api/services/{id}/toggle", post(services_toggle))
        .route("/api/alerts", get(alerts_get))
        .route("/api/alerts/ack", post(alerts_ack))
        .route("/api/config/remote-control", post(set_remote_control))
        .route("/ws", get(ws::ws_handler))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let mut app = Router::new()
        .merge(api)
        .fallback(static_assets)
        .with_state(state);

    // In dev il frontend gira su Vite (porta 1420): serve CORS permissivo.
    if cfg!(debug_assertions) {
        app = app.layer(tower_http::cors::CorsLayer::very_permissive());
    }

    let lan_enabled = cfg.lan_enabled;
    tracing::info!(%bind_ip, port, lan_enabled, "server in ascolto");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            tracing::error!(%e, "server terminato con errore");
        }
    });

    Ok(ServerInfo { port, lan_enabled })
}

/// Localhost: sempre autorizzato. LAN: serve il cookie di pairing.
/// /api/pair è escluso (è l'endpoint che valida il token e imposta il cookie).
async fn auth_middleware(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if peer.ip().is_loopback() || request.uri().path() == "/api/pair" {
        return next.run(request).await;
    }
    let token = state.config.get().pair_token;
    if cookie_value(request.headers(), PAIR_COOKIE).as_deref() == Some(token.as_str()) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "ok": false,
            "error": {
                "code": "UNAUTHORIZED",
                "message": "Dispositivo non abbinato: scansiona il QR dal desktop",
                "retryable": false
            }
        })),
    )
        .into_response()
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

async fn health(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "data": {
            "name": "RickyDEVTool",
            "version": env!("CARGO_PKG_VERSION"),
            "port": state.port,
            "os": std::env::consts::OS
        }
    }))
}

async fn lan_info(State(state): State<ServerState>) -> Json<serde_json::Value> {
    let cfg = state.config.get();
    let urls: Vec<String> = netinfo::lan_ips()
        .into_iter()
        .map(|ip| format!("http://{ip}:{}", state.port))
        .collect();
    Json(json!({
        "ok": true,
        "data": { "urls": urls, "port": state.port, "lanEnabled": cfg.lan_enabled, "remoteControlEnabled": cfg.remote_control_enabled }
    }))
}

/// QR con URL primario + token di pairing nel fragment.
/// Protetto dall'auth middleware: lo vede solo il desktop (o un device già abbinato).
async fn lan_qr(State(state): State<ServerState>) -> Response {
    let cfg = state.config.get();
    let Some(ip) = netinfo::lan_ips().into_iter().next() else {
        return (StatusCode::NOT_FOUND, "nessun IP LAN").into_response();
    };
    let url = format!("http://{ip}:{}/#pair={}", state.port, cfg.pair_token);
    match qrcode::QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let svg = code
                .render::<qrcode::render::svg::Color>()
                .min_dimensions(220, 220)
                .build();
            ([(header::CONTENT_TYPE, "image/svg+xml")], svg).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct PairBody {
    token: String,
}

async fn pair(State(state): State<ServerState>, Json(body): Json<PairBody>) -> Response {
    let cfg = state.config.get();
    if body.token != cfg.pair_token {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "error": { "code": "ACCESS_DENIED", "message": "Token di pairing non valido", "retryable": false }
            })),
        )
            .into_response();
    }
    let cookie = format!(
        "{PAIR_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly",
        cfg.pair_token
    );
    (
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "ok": true, "data": { "paired": true } })),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogBody {
    level: Option<String>,
    message: String,
    stack: Option<String>,
}

async fn client_log(Json(body): Json<LogBody>) -> Json<serde_json::Value> {
    let stack = body.stack.unwrap_or_default();
    match body.level.as_deref() {
        Some("error") => tracing::error!(target: "frontend", message = %body.message, %stack),
        Some("warn") => tracing::warn!(target: "frontend", message = %body.message, %stack),
        _ => tracing::info!(target: "frontend", message = %body.message),
    }
    Json(json!({ "ok": true, "data": null }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeavyQuery {
    cpu_min: Option<f32>,
    mem_min: Option<f32>,
}

async fn heavy_processes(
    axum::extract::Query(query): axum::extract::Query<HeavyQuery>,
) -> Json<serde_json::Value> {
    let result = crate::adapters::procs::heavy_processes(
        query.cpu_min.unwrap_or(20.0).clamp(0.0, 100.0),
        query.mem_min.unwrap_or(10.0).clamp(0.0, 100.0),
    )
    .await;
    Json(json!({ "ok": true, "data": result }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortsQuery {
    include_system: Option<bool>,
}

async fn list_ports(
    axum::extract::Query(query): axum::extract::Query<PortsQuery>,
) -> Response {
    match crate::adapters::ports::scan_tcp_listen(query.include_system.unwrap_or(false)).await {
        Ok(scan) => Json(json!({ "ok": true, "data": scan })).into_response(),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "ok": false,
                "error": { "code": "INTERNAL", "message": message, "retryable": true }
            })),
        )
            .into_response(),
    }
}

/// Kill di un processo. Azione distruttiva: dalla LAN è negata finché
/// non esisterà il toggle "Remote control" (v1).
async fn kill_process(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<crate::adapters::kill::KillRequest>,
) -> Response {
    use crate::adapters::kill::{kill_process as do_kill, KillError};

    if !write_allowed(&state, peer) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "error": {
                    "code": "REMOTE_FORBIDDEN",
                    "message": "Il kill da remoto è disabilitato: usa il desktop",
                    "retryable": false
                }
            })),
        )
            .into_response();
    }

    match do_kill(req).await {
        Ok(outcome) => Json(json!({ "ok": true, "data": outcome })).into_response(),
        Err(e) => {
            let (status, code, message, os_hint) = match e {
                KillError::ProcessGone => (
                    StatusCode::CONFLICT,
                    "PROCESS_GONE",
                    "Il processo non esiste più o il PID è stato riusato".to_string(),
                    None,
                ),
                KillError::SystemProtected => (
                    StatusCode::FORBIDDEN,
                    "ACCESS_DENIED",
                    "Processo di sistema: non terminabile da questo tool".to_string(),
                    None,
                ),
                KillError::TypedConfirmRequired { name } => (
                    StatusCode::PRECONDITION_REQUIRED,
                    "TYPED_CONFIRM_REQUIRED",
                    format!("\"{name}\" è protetto: digita il nome per confermare"),
                    None,
                ),
                KillError::Failed { message, os_hint } => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL",
                    message,
                    os_hint,
                ),
            };
            (
                status,
                Json(json!({
                    "ok": false,
                    "error": { "code": code, "message": message, "osHint": os_hint, "retryable": false }
                })),
            )
                .into_response()
        }
    }
}

// ---------- tools ----------

#[derive(Deserialize)]
struct ToolsQuery {
    refresh: Option<bool>,
}

async fn cached_tools(
    state: &ServerState,
    force_refresh: bool,
) -> Vec<crate::adapters::tools::DiscoveredTool> {
    let mut cache = state.tools_cache.lock().await;
    if force_refresh || cache.is_none() {
        let overrides = state.config.get().tool_paths;
        *cache = Some(crate::adapters::tools::discover_all(&overrides).await);
    }
    cache.clone().unwrap_or_default()
}

async fn list_tools(
    State(state): State<ServerState>,
    axum::extract::Query(query): axum::extract::Query<ToolsQuery>,
) -> Json<serde_json::Value> {
    let tools = cached_tools(&state, query.refresh.unwrap_or(false)).await;
    Json(json!({ "ok": true, "data": { "tools": tools } }))
}

#[derive(Deserialize)]
struct LaunchBody {
    target: Option<String>,
}

/// Avvio di applicazioni: azione locale, negata dalla LAN come il kill.
async fn launch_tool(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    Json(body): Json<LaunchBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "error": { "code": "REMOTE_FORBIDDEN", "message": "L'avvio di applicazioni da remoto è disabilitato", "retryable": false }
            })),
        )
            .into_response();
    }
    let tools = cached_tools(&state, false).await;
    let Some(tool) = tools.iter().find(|t| t.id == id && t.found) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": { "code": "TOOL_NOT_FOUND", "message": format!("tool {id} non trovato sulla macchina"), "retryable": false }
            })),
        )
            .into_response();
    };
    match crate::adapters::tools::launch(tool, body.target.as_deref()).await {
        Ok(()) => Json(json!({ "ok": true, "data": { "launched": true } })).into_response(),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "ok": false,
                "error": { "code": "INTERNAL", "message": message, "retryable": true }
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ToolPathBody {
    /// null/assente = rimuovi l'override e torna alla discovery automatica.
    path: Option<String>,
}

async fn set_tool_path(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(body): Json<ToolPathBody>,
) -> Response {
    if !crate::adapters::tools::TOOL_IDS.contains(&id.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": { "code": "TOOL_NOT_FOUND", "message": format!("tool sconosciuto: {id}"), "retryable": false }
            })),
        )
            .into_response();
    }
    state.config.update(|c| match &body.path {
        Some(path) if !path.trim().is_empty() => {
            c.tool_paths.insert(id.clone(), path.trim().to_string());
        }
        _ => {
            c.tool_paths.remove(&id);
        }
    });
    let tools = cached_tools(&state, true).await;
    Json(json!({ "ok": true, "data": { "tools": tools } })).into_response()
}

// ---------- progetti / filesystem / git ----------

#[derive(Deserialize)]
struct PathQuery {
    path: Option<String>,
}

fn internal_error(message: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "ok": false,
            "error": { "code": "INTERNAL", "message": message, "retryable": true }
        })),
    )
        .into_response()
}

async fn fs_dirs(axum::extract::Query(query): axum::extract::Query<PathQuery>) -> Response {
    match crate::services::projects::list_dirs(query.path).await {
        Ok(listing) => Json(json!({ "ok": true, "data": listing })).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": { "code": "PATH_NOT_FOUND", "message": message, "retryable": false }
            })),
        )
            .into_response(),
    }
}

async fn projects_scan(axum::extract::Query(query): axum::extract::Query<PathQuery>) -> Response {
    let Some(path) = query.path else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": { "code": "PATH_NOT_FOUND", "message": "parametro path mancante", "retryable": false }
            })),
        )
            .into_response();
    };
    match crate::services::projects::scan(path).await {
        Ok(scan) => Json(json!({ "ok": true, "data": scan })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn pinned_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "folders": state.config.get().pinned_folders } }))
}

#[derive(Deserialize)]
struct PinBody {
    path: String,
    action: String, // "add" | "remove"
}

async fn pinned_set(State(state): State<ServerState>, Json(body): Json<PinBody>) -> Response {
    state.config.update(|c| {
        c.pinned_folders.retain(|p| p != &body.path);
        if body.action == "add" {
            c.pinned_folders.insert(0, body.path.clone());
            c.pinned_folders.truncate(12);
        }
    });
    Json(json!({ "ok": true, "data": { "folders": state.config.get().pinned_folders } }))
        .into_response()
}

fn git_error_response(e: crate::services::git::GitError) -> Response {
    use crate::services::git::GitError;
    let (status, code, message, os_hint) = match e {
        GitError::NotARepo => (
            StatusCode::BAD_REQUEST,
            "PATH_NOT_FOUND",
            "La cartella non è un repository git".to_string(),
            None,
        ),
        GitError::AuthFailed(detail) => (
            StatusCode::BAD_GATEWAY,
            "GIT_AUTH_FAILED",
            "Autenticazione git fallita".to_string(),
            Some(detail),
        ),
        GitError::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "TIMEOUT",
            "Operazione git scaduta".to_string(),
            None,
        ),
        GitError::Failed(detail) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", detail, None),
    };
    (
        status,
        Json(json!({
            "ok": false,
            "error": { "code": code, "message": message, "osHint": os_hint, "retryable": true }
        })),
    )
        .into_response()
}

async fn git_info(axum::extract::Query(query): axum::extract::Query<PathQuery>) -> Response {
    let Some(path) = query.path else {
        return internal_error("parametro path mancante".into());
    };
    match crate::services::git::repo_info(&path).await {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(e) => git_error_response(e),
    }
}

#[derive(Deserialize)]
struct GitActionBody {
    path: String,
}

fn remote_forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "ok": false,
            "error": { "code": "REMOTE_FORBIDDEN", "message": "Azione disabilitata da remoto: usa il desktop", "retryable": false }
        })),
    )
        .into_response()
}

async fn git_fetch(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<GitActionBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match crate::services::git::fetch(&body.path).await {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(e) => git_error_response(e),
    }
}

async fn git_pull(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<GitActionBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match crate::services::git::pull(&body.path).await {
        Ok((info, summary)) => {
            Json(json!({ "ok": true, "data": { "info": info, "summary": summary } }))
                .into_response()
        }
        Err(e) => git_error_response(e),
    }
}

// ---------- git branches / checkout ----------

async fn git_branches(axum::extract::Query(query): axum::extract::Query<PathQuery>) -> Response {
    let Some(path) = query.path else {
        return internal_error("parametro path mancante".into());
    };
    match crate::services::git::branches(&path).await {
        Ok(branches) => Json(json!({ "ok": true, "data": { "branches": branches } })).into_response(),
        Err(e) => git_error_response(e),
    }
}

#[derive(Deserialize)]
struct CheckoutBody {
    path: String,
    branch: String,
}

async fn git_checkout(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<CheckoutBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match crate::services::git::checkout(&body.path, &body.branch).await {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(e) => git_error_response(e),
    }
}

// ---------- node ----------

async fn node_info(
    State(state): State<ServerState>,
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Response {
    let Some(path) = query.path else {
        return internal_error("parametro path mancante".into());
    };
    let overrides = state.config.get().node_pm_overrides;
    match crate::services::node::inspect(&path, overrides.get(&path).map(String::as_str)) {
        Ok(project) => Json(json!({ "ok": true, "data": project })).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": { "code": "PATH_NOT_FOUND", "message": message, "retryable": false }
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct NodePmBody {
    path: String,
    /// null = torna alla detection automatica.
    pm: Option<String>,
}

async fn node_set_pm(State(state): State<ServerState>, Json(body): Json<NodePmBody>) -> Response {
    if let Some(pm) = &body.pm {
        if crate::services::node::PackageManager::from_str(pm).is_none() {
            return internal_error(format!("package manager non valido: {pm}"));
        }
    }
    state.config.update(|c| match &body.pm {
        Some(pm) => {
            c.node_pm_overrides.insert(body.path.clone(), pm.clone());
        }
        None => {
            c.node_pm_overrides.remove(&body.path);
        }
    });
    node_info(
        State(state),
        axum::extract::Query(PathQuery { path: Some(body.path) }),
    )
    .await
}

#[derive(Deserialize)]
struct NodeRunBody {
    path: String,
    /// None = install.
    script: Option<String>,
}

async fn node_run(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<NodeRunBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    let overrides = state.config.get().node_pm_overrides;
    let project = match crate::services::node::inspect(
        &body.path,
        overrides.get(&body.path).map(String::as_str),
    ) {
        Ok(p) => p,
        Err(message) => return internal_error(message),
    };
    if let Some(script) = &body.script {
        if !project.scripts.contains_key(script) {
            return internal_error(format!("script \"{script}\" non presente in package.json"));
        }
    }
    let (program, args) = crate::services::node::command_for(
        project.package_manager,
        body.script.as_deref(),
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let label = format!(
        "{} {} — {}",
        program,
        args.join(" "),
        project.package_name.as_deref().unwrap_or(&body.path)
    );
    match state.tasks.spawn(&label, &program, &arg_refs, &body.path) {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(message) => internal_error(message),
    }
}

// ---------- dotnet ----------

fn dotnet_inspect_with_config(
    state: &ServerState,
    path: &str,
) -> Result<crate::services::dotnet::DotnetProject, String> {
    let cfg = state.config.get();
    crate::services::dotnet::inspect(
        path,
        cfg.dotnet_startup.get(path).map(String::as_str),
        cfg.dotnet_profile.get(path).map(String::as_str),
    )
}

async fn dotnet_info(
    State(state): State<ServerState>,
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Response {
    let Some(path) = query.path else {
        return internal_error("parametro path mancante".into());
    };
    match dotnet_inspect_with_config(&state, &path) {
        Ok(project) => Json(json!({ "ok": true, "data": project })).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": { "code": "PATH_NOT_FOUND", "message": message, "retryable": false }
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DotnetSelectBody {
    path: String,
    startup_project: Option<String>,
    profile: Option<String>,
}

async fn dotnet_select(
    State(state): State<ServerState>,
    Json(body): Json<DotnetSelectBody>,
) -> Response {
    state.config.update(|c| {
        match &body.startup_project {
            Some(p) => {
                c.dotnet_startup.insert(body.path.clone(), p.clone());
            }
            None => {}
        }
        match &body.profile {
            Some(p) => {
                c.dotnet_profile.insert(body.path.clone(), p.clone());
            }
            None => {
                c.dotnet_profile.remove(&body.path);
            }
        }
    });
    dotnet_info(
        State(state),
        axum::extract::Query(PathQuery { path: Some(body.path) }),
    )
    .await
}

#[derive(Deserialize)]
struct DotnetRunBody {
    path: String,
    action: String, // run | build | rebuild | clean
}

async fn dotnet_run(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<DotnetRunBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    let project = match dotnet_inspect_with_config(&state, &body.path) {
        Ok(p) => p,
        Err(message) => return internal_error(message),
    };
    let (program, args) = match crate::services::dotnet::command_for(&project, &body.action) {
        Ok(cmd) => cmd,
        Err(message) => return internal_error(message),
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let label = format!("dotnet {} — {}", body.action, short_name(&body.path));
    match state.tasks.spawn(&label, &program, &arg_refs, &body.path) {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(message) => internal_error(message),
    }
}

fn short_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

// ---------- servizi online / alerts / remote control ----------

async fn services_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "services": state.config.get().services } }))
}

async fn services_upsert(
    State(state): State<ServerState>,
    Json(def): Json<crate::services::online::ServiceDef>,
) -> Response {
    if def.id.trim().is_empty() || def.label.trim().is_empty() || def.target.trim().is_empty() {
        return internal_error("id, label e target sono obbligatori".into());
    }
    state.config.update(|c| {
        match c.services.iter_mut().find(|s| s.id == def.id) {
            Some(existing) => {
                if existing.builtin {
                    // Dei preset si può cambiare solo enabled (via toggle): ignora il resto.
                    return;
                }
                *existing = crate::services::online::ServiceDef { builtin: false, ..def.clone() };
            }
            None => {
                c.services.push(crate::services::online::ServiceDef {
                    builtin: false,
                    ..def.clone()
                });
            }
        }
    });
    services_get(State(state)).await.into_response()
}

async fn services_delete(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    let mut removed = false;
    state.config.update(|c| {
        let before = c.services.len();
        c.services.retain(|s| s.id != id || s.builtin);
        removed = c.services.len() != before;
    });
    if !removed {
        return internal_error("servizio non trovato o preset non eliminabile".into());
    }
    services_get(State(state)).await.into_response()
}

async fn services_toggle(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    let mut found = false;
    state.config.update(|c| {
        if let Some(service) = c.services.iter_mut().find(|s| s.id == id) {
            service.enabled = !service.enabled;
            found = true;
        }
    });
    if !found {
        return internal_error("servizio non trovato".into());
    }
    services_get(State(state)).await.into_response()
}

async fn alerts_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "alerts": state.alerts.list() } }))
}

#[derive(Deserialize)]
struct AckBody {
    /// None = ack di tutti.
    id: Option<String>,
}

async fn alerts_ack(State(state): State<ServerState>, Json(body): Json<AckBody>) -> Json<serde_json::Value> {
    state.alerts.ack(body.id.as_deref());
    Json(json!({ "ok": true, "data": { "alerts": state.alerts.list() } }))
}

#[derive(Deserialize)]
struct RemoteControlBody {
    enabled: bool,
}

/// Attivabile solo dal desktop: un telefono non può auto-promuoversi.
async fn set_remote_control(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<RemoteControlBody>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    state.config.update(|c| c.remote_control_enabled = body.enabled);
    tracing::info!(enabled = body.enabled, "remote control aggiornato");
    Json(json!({ "ok": true, "data": { "remoteControlEnabled": body.enabled } })).into_response()
}

// ---------- tasks ----------

async fn tasks_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "tasks": state.tasks.list() } }))
}

async fn tasks_clear_finished(State(state): State<ServerState>) -> Json<serde_json::Value> {
    state.tasks.clear_finished();
    Json(json!({ "ok": true, "data": { "tasks": state.tasks.list() } }))
}

async fn task_stop(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match state.tasks.stop(&id) {
        Ok(()) => Json(json!({ "ok": true, "data": { "stopping": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntervalBody {
    interval_ms: u64,
}

async fn set_interval(
    State(state): State<ServerState>,
    Path(topic): Path<String>,
    Json(body): Json<IntervalBody>,
) -> Response {
    if !state.pollers.set_interval(&topic, body.interval_ms) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": { "code": "PATH_NOT_FOUND", "message": format!("topic sconosciuto: {topic}"), "retryable": false }
            })),
        )
            .into_response();
    }
    if topic == crate::collectors::stats::TOPIC {
        state
            .config
            .update(|c| c.stats_interval_ms = body.interval_ms.clamp(200, 60_000));
    }
    Json(json!({ "ok": true, "data": { "intervalMs": body.interval_ms } })).into_response()
}

/// Serve la SPA embedded; fallback su index.html per le route client-side.
async fn static_assets(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(file) => asset_response(path, file),
        None => match Assets::get("index.html") {
            Some(file) => asset_response("index.html", file),
            None => (
                StatusCode::NOT_FOUND,
                "SPA non trovata: esegui `npm run build`",
            )
                .into_response(),
        },
    }
}

fn asset_response(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
}
