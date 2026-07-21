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
    pub tails: Arc<crate::services::logtail::TailRegistry>,
    pub drop: Arc<crate::services::drop::DropService>,
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
    let alerts = crate::alerts::AlertService::start(bus.clone(), config.clone());
    let tails = Arc::new(crate::services::logtail::TailRegistry::new(bus.clone()));
    // Beacon UDP broadcast: permette ad altre istanze RickyDEVTool sulla LAN
    // (altri computer) di scoprirsi a vicenda per Drop, indipendentemente
    // dall'hub a cui un browser/telefono è connesso.
    let hub_registry = crate::services::hubdiscovery::start(&config, port);
    let drop_service = Arc::new(crate::services::drop::DropService::new(
        bus.clone(),
        config.clone(),
        hub_registry,
    ));
    // Avviato qui perché serve il runtime tokio (attivo dentro server::start).
    crate::jiggler::start(config.clone());
    let state = ServerState {
        config,
        bus,
        pollers,
        port,
        tools_cache: Arc::new(tokio::sync::Mutex::new(None)),
        tasks,
        alerts,
        tails,
        drop: drop_service,
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
        .route("/api/disks", get(list_disks))
        .route("/api/disks/eject", post(disk_eject))
        .route("/api/disks/format", post(disk_format))
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
        .route("/api/fs/entries", get(fs_entries))
        .route("/api/env/files", get(env_files))
        .route("/api/env/read", get(env_read))
        .route("/api/env/activate", post(env_activate))
        .route("/api/logtail", get(logtail_list))
        .route("/api/logtail/start", post(logtail_start))
        .route("/api/logtail/{id}/stop", post(logtail_stop))
        .route("/api/net/ping", post(net_ping))
        .route("/api/net/dns", post(net_dns))
        .route("/api/net/portcheck", post(net_portcheck))
        .route("/api/net/scan", post(net_scan))
        .route("/api/push", get(push_get).post(push_set))
        .route("/api/push/test", post(push_test))
        .route("/api/drop/hello", post(drop_hello))
        .route("/api/drop/peers", get(drop_peers))
        .route("/api/drop/self", get(drop_self))
        .route("/api/drop/hubs", get(drop_hubs))
        .route(
            "/api/drop/send",
            post(drop_send).layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024 * 1024)),
        )
        .route("/api/drop/text", post(drop_text))
        .route("/api/drop/download/{id}", get(drop_download))
        .route("/api/drop/received", get(drop_received))
        .route("/api/drop/open/{name}", post(drop_open_file))
        .route("/api/drop/reveal/{name}", post(drop_reveal_file))
        .route("/api/drop/received/{name}",axum::routing::delete(drop_received_delete))
        .route("/api/drop/open-folder", post(drop_open_folder))
        .route("/api/config/remote-control", post(set_remote_control))
        .route("/api/config/anti-idle", post(set_anti_idle))
        .route("/api/system/accessibility", get(accessibility_status))
        .route("/api/system/open-accessibility", post(open_accessibility))
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
    // Richieste hub-to-hub (Drop cross-computer, proxy_send_*): non hanno il
    // pairing dell'utente (ogni hub genera il proprio token indipendente), ma
    // sono già verificate dalla discovery UDP reciproca — l'IP di provenienza
    // deve combaciare con un hub che abbiamo visto beaconare con quello
    // stesso hub_id. Vale solo per i due endpoint di trasferimento.
    if matches!(request.uri().path(), "/api/drop/send" | "/api/drop/text") {
        if let Some(claimed) = request
            .headers()
            .get("x-rickydev-hub-id")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(hub) = state.drop.remote_hub(claimed) {
                if hub.ip == peer.ip().to_string() {
                    return next.run(request).await;
                }
            }
        }
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
        "data": { "urls": urls, "port": state.port, "lanEnabled": cfg.lan_enabled, "remoteControlEnabled": cfg.remote_control_enabled, "antiIdleEnabled": cfg.anti_idle_enabled }
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

// ---------- dischi ----------

async fn list_disks() -> Json<serde_json::Value> {
    let disks = tokio::task::spawn_blocking(crate::adapters::disks::list)
        .await
        .unwrap_or_default();
    Json(json!({ "ok": true, "data": { "disks": disks } }))
}

fn disk_error_response(e: crate::adapters::disks::DiskError) -> Response {
    use crate::adapters::disks::DiskError;
    let (status, code, message, os_hint) = match e {
        DiskError::NotFound => (
            StatusCode::NOT_FOUND,
            "PATH_NOT_FOUND",
            "Disco non trovato (forse già rimosso)".to_string(),
            None,
        ),
        DiskError::NotRemovable => (
            StatusCode::FORBIDDEN,
            "ACCESS_DENIED",
            "Solo i dischi rimovibili possono essere espulsi o formattati".to_string(),
            None,
        ),
        DiskError::Unsupported(msg) => (StatusCode::NOT_IMPLEMENTED, "UNSUPPORTED", msg, None),
        DiskError::Failed { message, os_hint } => {
            (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", message, os_hint)
        }
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EjectBody {
    mount_point: String,
}

/// Eject e format sono distruttivi: sempre e solo da localhost, mai da remoto
/// (nemmeno col controllo remoto attivo).
async fn disk_eject(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<EjectBody>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    match crate::adapters::disks::eject(&body.mount_point).await {
        Ok(()) => Json(json!({ "ok": true, "data": { "ejected": true } })).into_response(),
        Err(e) => disk_error_response(e),
    }
}

async fn disk_format(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<crate::adapters::disks::FormatRequest>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    match crate::adapters::disks::format(req).await {
        Ok(()) => Json(json!({ "ok": true, "data": { "formatted": true } })).into_response(),
        Err(e) => disk_error_response(e),
    }
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

async fn set_anti_idle(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<RemoteControlBody>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    state.config.update(|c| c.anti_idle_enabled = body.enabled);
    tracing::info!(enabled = body.enabled, "anti-idle aggiornato");
    Json(json!({ "ok": true, "data": { "antiIdleEnabled": body.enabled } })).into_response()
}

async fn accessibility_status() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": crate::adapters::accessibility::status() }))
}

/// Apre il pannello Accessibilità delle impostazioni di sistema (solo macOS).
async fn open_accessibility(ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    match crate::adapters::accessibility::open_settings() {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(message) => internal_error(message),
    }
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

// ---------- fs / env / logtail ----------
// Leggere file arbitrari (log, .env) espone contenuti sensibili: come le
// azioni di scrittura, è riservato a localhost o al controllo remoto attivo.

async fn fs_entries(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match crate::services::projects::list_entries(query.path).await {
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

async fn env_files(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    let Some(path) = query.path else {
        return internal_error("parametro path mancante".into());
    };
    match crate::services::env::list(&path) {
        Ok(files) => Json(json!({ "ok": true, "data": { "files": files } })).into_response(),
        Err(message) => internal_error(message),
    }
}

#[derive(Deserialize)]
struct EnvQuery {
    path: String,
    file: String,
}

async fn env_read(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    axum::extract::Query(query): axum::extract::Query<EnvQuery>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match crate::services::env::read(&query.path, &query.file) {
        Ok(content) => Json(json!({ "ok": true, "data": content })).into_response(),
        Err(message) => internal_error(message),
    }
}

#[derive(Deserialize)]
struct EnvActivateBody {
    path: String,
    file: String,
}

async fn env_activate(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<EnvActivateBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match crate::services::env::activate(&body.path, &body.file) {
        Ok(()) => match crate::services::env::list(&body.path) {
            Ok(files) => Json(json!({ "ok": true, "data": { "files": files } })).into_response(),
            Err(message) => internal_error(message),
        },
        Err(message) => internal_error(message),
    }
}

async fn logtail_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "tails": state.tails.list() } }))
}

#[derive(Deserialize)]
struct TailStartBody {
    path: String,
}

async fn logtail_start(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<TailStartBody>,
) -> Response {
    if !write_allowed(&state, peer) {
        return remote_forbidden();
    }
    match state.tails.start(&body.path) {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn logtail_stop(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    match state.tails.stop(&id) {
        Ok(()) => Json(json!({ "ok": true, "data": { "stopped": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

// ---------- toolbox di rete ----------

#[derive(Deserialize)]
struct HostBody {
    host: String,
}

async fn net_ping(Json(body): Json<HostBody>) -> Json<serde_json::Value> {
    let result = crate::services::nettools::ping(body.host.trim()).await;
    Json(json!({ "ok": true, "data": result }))
}

#[derive(Deserialize)]
struct DnsBody {
    name: String,
}

async fn net_dns(Json(body): Json<DnsBody>) -> Response {
    match crate::services::nettools::dns_lookup(body.name.trim()).await {
        Ok(records) => Json(json!({ "ok": true, "data": { "records": records } })).into_response(),
        Err(message) => internal_error(message),
    }
}

#[derive(Deserialize)]
struct PortCheckBody {
    host: String,
    ports: Vec<u16>,
}

async fn net_portcheck(Json(body): Json<PortCheckBody>) -> Response {
    match crate::services::nettools::check_ports(body.host.trim(), &body.ports).await {
        Ok(results) => Json(json!({ "ok": true, "data": { "results": results } })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn net_scan() -> Response {
    match crate::services::nettools::scan_lan().await {
        Ok(hosts) => Json(json!({ "ok": true, "data": { "hosts": hosts } })).into_response(),
        Err(message) => internal_error(message),
    }
}

// ---------- push (ntfy) ----------

fn push_settings_json(state: &ServerState) -> serde_json::Value {
    let cfg = state.config.get();
    json!({
        "enabled": cfg.push_enabled,
        "server": cfg.push_server,
        "topic": cfg.push_topic,
        "minSeverity": cfg.push_min_severity,
    })
}

async fn push_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": push_settings_json(&state) }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushBody {
    enabled: Option<bool>,
    server: Option<String>,
    topic: Option<String>,
    min_severity: Option<String>,
}

/// Configurabile solo dal desktop, come il controllo remoto.
async fn push_set(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<PushBody>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    if let Some(severity) = &body.min_severity {
        if !["info", "warning", "critical"].contains(&severity.as_str()) {
            return internal_error(format!("severità non valida: {severity}"));
        }
    }
    state.config.update(|c| {
        if let Some(enabled) = body.enabled {
            c.push_enabled = enabled;
        }
        if let Some(server) = &body.server {
            let server = server.trim();
            if server.starts_with("http://") || server.starts_with("https://") {
                c.push_server = server.trim_end_matches('/').to_string();
            }
        }
        if let Some(topic) = &body.topic {
            let topic = topic.trim();
            if !topic.is_empty() && topic.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
                c.push_topic = topic.to_string();
            }
        }
        if let Some(severity) = &body.min_severity {
            c.push_min_severity = severity.clone();
        }
    });
    Json(json!({ "ok": true, "data": push_settings_json(&state) })).into_response()
}

async fn push_test(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    let cfg = state.config.get();
    match crate::notify::send(
        &cfg.push_server,
        &cfg.push_topic,
        "info",
        "Notifica di prova",
        "Se la leggi sul telefono, il push funziona. 🎉",
    )
    .await
    {
        Ok(()) => Json(json!({ "ok": true, "data": { "sent": true } })).into_response(),
        Err(message) => internal_error(format!("invio fallito: {message}")),
    }
}

// ---------- file drop ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropHelloBody {
    device_id: String,
    name: String,
}

fn valid_device_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

async fn drop_hello(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<DropHelloBody>,
) -> Response {
    if !valid_device_id(&body.device_id) {
        return internal_error("deviceId non valido".into());
    }
    // "Desktop" = la webview locale: è il peer che salva su disco.
    let peers = state
        .drop
        .hello(&body.device_id, &body.name, peer.ip().is_loopback());
    Json(json!({ "ok": true, "data": { "peers": peers } })).into_response()
}

#[derive(Deserialize)]
struct PeersQuery {
    #[serde(rename = "self")]
    self_id: Option<String>,
}

async fn drop_peers(
    State(state): State<ServerState>,
    axum::extract::Query(query): axum::extract::Query<PeersQuery>,
) -> Json<serde_json::Value> {
    let peers = state.drop.peers_except(query.self_id.as_deref().unwrap_or(""));
    Json(json!({ "ok": true, "data": { "peers": peers } }))
}

/// Identità stabile di questo hub: la UI la usa per sottoscriversi anche al
/// canale WS su cui arrivano i trasferimenti proxati da altri computer.
async fn drop_self(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "hubId": state.drop.hub_id() } }))
}

/// Altri computer RickyDEVTool visti in LAN via beacon UDP (indipendenti da
/// questo hub): utile per capire se la discovery cross-macchina funziona.
async fn drop_hubs(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "hubs": state.drop.remote_hubs() } }))
}

const MAX_PROXY_BYTES: usize = 200 * 1024 * 1024;

/// Inoltra un file all'hub remoto via HTTP (l'utente ha scelto un peer che
/// vive su un'altra macchina, scoperta via UDP broadcast). `to` per il
/// ricevente è il SUO hub_id: ogni hub riconosce sempre il proprio come "il
/// desktop", quindi non serve che il destinatario abbia fatto hello.
async fn proxy_send_file(
    hub: &crate::services::hubdiscovery::RemoteHub,
    own_hub_id: &str,
    from_name: &str,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<u64, String> {
    let size = bytes.len() as u64;
    let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name.to_string());
    let form = reqwest::multipart::Form::new()
        .text("to", hub.hub_id.clone())
        .text("fromName", from_name.to_string())
        .part("file", part);
    let url = format!("http://{}:{}/api/drop/send", hub.ip, hub.http_port);
    let response = reqwest::Client::new()
        .post(&url)
        .header("X-RickyDev-Hub-Id", own_hub_id)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("invio a {} fallito: {e}", hub.name))?;
    if !response.status().is_success() {
        return Err(format!("{} ha rifiutato il file (HTTP {})", hub.name, response.status()));
    }
    Ok(size)
}

async fn proxy_send_text(
    hub: &crate::services::hubdiscovery::RemoteHub,
    own_hub_id: &str,
    from_name: &str,
    text: &str,
) -> Result<(), String> {
    let url = format!("http://{}:{}/api/drop/text", hub.ip, hub.http_port);
    let body = json!({ "to": hub.hub_id, "fromName": from_name, "text": text });
    let response = reqwest::Client::new()
        .post(&url)
        .header("X-RickyDev-Hub-Id", own_hub_id)
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("invio a {} fallito: {e}", hub.name))?;
    if !response.status().is_success() {
        return Err(format!("{} ha rifiutato il messaggio (HTTP {})", hub.name, response.status()));
    }
    Ok(())
}

/// Upload multipart: campi `to`, `fromName` e poi `file` (l'ordine conta,
/// i campi testo devono precedere il file).
async fn drop_send(
    State(state): State<ServerState>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    use tokio::io::AsyncWriteExt;

    let mut to = String::new();
    let mut from_name = String::from("Sconosciuto");
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => return internal_error(format!("upload interrotto: {e}")),
        };
        match field.name() {
            Some("to") => to = field.text().await.unwrap_or_default(),
            Some("fromName") => from_name = field.text().await.unwrap_or_default(),
            Some("file") => {
                if to.is_empty() {
                    return internal_error("campo 'to' mancante prima del file".into());
                }
                let file_name = field
                    .file_name()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "file".to_string());

                // Il destinatario è un altro computer (scoperto in LAN via UDP),
                // non un peer di questo hub: si proxa l'intero file via HTTP.
                if let Some(hub) = state.drop.remote_hub(&to) {
                    let bytes = match field.bytes().await {
                        Ok(b) => b,
                        Err(e) => return internal_error(format!("upload interrotto: {e}")),
                    };
                    if bytes.len() > MAX_PROXY_BYTES {
                        return internal_error(format!(
                            "file troppo grande per l'invio a un altro computer (max {}MB)",
                            MAX_PROXY_BYTES / 1024 / 1024
                        ));
                    }
                    let own_hub_id = state.drop.hub_id();
                    return match proxy_send_file(&hub, &own_hub_id, &from_name, &file_name, bytes.to_vec()).await {
                        Ok(size) => Json(json!({
                            "ok": true,
                            "data": { "transferId": format!("proxy-{}", crate::events::now_ms()), "sizeBytes": size }
                        }))
                        .into_response(),
                        Err(message) => internal_error(message),
                    };
                }

                let (id, path, saved_on_disk) = match state.drop.prepare_incoming(&to, &file_name) {
                    Ok(prepared) => prepared,
                    Err(message) => return internal_error(message),
                };
                let mut out = match tokio::fs::File::create(&path).await {
                    Ok(f) => f,
                    Err(e) => return internal_error(format!("scrittura fallita: {e}")),
                };
                let mut size: u64 = 0;
                let mut field = field;
                loop {
                    match field.chunk().await {
                        Ok(Some(chunk)) => {
                            size += chunk.len() as u64;
                            if let Err(e) = out.write_all(&chunk).await {
                                let _ = tokio::fs::remove_file(&path).await;
                                return internal_error(format!("scrittura fallita: {e}"));
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&path).await;
                            return internal_error(format!("upload interrotto: {e}"));
                        }
                    }
                }
                if out.flush().await.is_err() {
                    let _ = tokio::fs::remove_file(&path).await;
                    return internal_error("scrittura fallita".into());
                }
                state
                    .drop
                    .finish_incoming(&id, &to, &from_name, &file_name, path, size, saved_on_disk);
                return Json(json!({ "ok": true, "data": { "transferId": id, "sizeBytes": size } }))
                    .into_response();
            }
            _ => {}
        }
    }
    internal_error("campo 'file' mancante".into())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropTextBody {
    to: String,
    from_name: String,
    text: String,
}

async fn drop_text(State(state): State<ServerState>, Json(body): Json<DropTextBody>) -> Response {
    if let Some(hub) = state.drop.remote_hub(&body.to) {
        let own_hub_id = state.drop.hub_id();
        return match proxy_send_text(&hub, &own_hub_id, &body.from_name, &body.text).await {
            Ok(()) => Json(json!({ "ok": true, "data": { "sent": true } })).into_response(),
            Err(message) => internal_error(message),
        };
    }
    match state.drop.send_text(&body.to, &body.from_name, &body.text) {
        Ok(()) => Json(json!({ "ok": true, "data": { "sent": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn drop_download(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    let Some((path, name)) = state.drop.transfer_file(&id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": { "code": "PATH_NOT_FOUND", "message": "trasferimento scaduto o inesistente", "retryable": false }
            })),
        )
            .into_response();
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => return internal_error(format!("file non leggibile: {e}")),
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    // RFC 5987 per i nomi non-ASCII; fallback ASCII per i client vecchi.
    let ascii: String = name
        .chars()
        .map(|c| if c.is_ascii() && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"),
            ),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

/// Lista/gestione dei file ricevuti sul desktop: solo da localhost
/// (la cartella vive sul computer che ospita il server).
async fn drop_received(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    Json(json!({
        "ok": true,
        "data": {
            "files": state.drop.received_files(),
            "folder": state.drop.received_dir().to_string_lossy(),
        }
    }))
    .into_response()
}

async fn drop_received_delete(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    match state.drop.delete_received(&name) {
        Ok(()) => Json(json!({ "ok": true, "data": { "deleted": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn drop_open_folder(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    let folder = state.drop.received_dir().clone();
    let _ = std::fs::create_dir_all(&folder);
    match tauri_plugin_opener::open_path(folder, None::<String>) {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(e) => internal_error(e.to_string()),
    }
}

/// Apre un file ricevuto con l'app di default (solo desktop/localhost).
async fn drop_open_file(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    let path = match state.drop.received_path(&name) {
        Ok(p) => p,
        Err(message) => return internal_error(message),
    };
    match tauri_plugin_opener::open_path(path, None::<String>) {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(e) => internal_error(e.to_string()),
    }
}

/// Mostra il file nel file manager (Finder/Explorer), evidenziandolo.
async fn drop_reveal_file(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
) -> Response {
    if !peer.ip().is_loopback() {
        return remote_forbidden();
    }
    let path = match state.drop.received_path(&name) {
        Ok(p) => p,
        Err(message) => return internal_error(message),
    };
    match tauri_plugin_opener::reveal_item_in_dir(path) {
        Ok(()) => Json(json!({ "ok": true, "data": { "revealed": true } })).into_response(),
        Err(e) => internal_error(e.to_string()),
    }
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
