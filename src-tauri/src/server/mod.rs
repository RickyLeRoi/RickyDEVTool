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
    pub metrics: Arc<crate::services::metrics::MetricsService>,
    pub clipboard: Arc<crate::services::clipboard::ClipboardHistory>,
    /// RickyAI: supervisore di `of-free` e proxy verso il suo endpoint.
    pub ai: Arc<crate::services::rickyai::AiService>,
}

/// Le azioni che modificano il sistema sono locali, oppure LAN se l'utente
/// ha attivato esplicitamente il controllo remoto.
fn write_allowed(state: &ServerState, peer: SocketAddr) -> bool {
    write_permitted(peer.ip().is_loopback(), state.config.get().remote_control_enabled)
}
fn write_permitted(is_loopback: bool, remote_control_enabled: bool) -> bool {
    is_loopback || remote_control_enabled
}

/// I due controlli di permesso, applicati come layer sui gruppi di rotte in
/// [`start`] invece che riscritti dentro ogni handler. Prima erano 48 copie a
/// mano in un file da 2800 righe, e chi ne dimenticava una non se ne accorgeva:
/// `set_remote_control` è rimasto scoperto a undici righe da `set_anti_idle`,
/// che invece controllava.
async fn require_loopback(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if peer.ip().is_loopback() {
        return next.run(request).await;
    }
    remote_forbidden()
}

async fn require_write(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if write_allowed(&state, peer) {
        return next.run(request).await;
    }
    remote_forbidden()
}

#[derive(Clone)]
pub struct ServerInfo {
    pub port: u16,
    pub lan_enabled: bool,
    /// Handle condiviso col resto del backend: usato dal tray per costruire i
    /// menu (dischi/porte/servizi/strumenti) e per l'invio diretto via Drop.
    pub state: ServerState,
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
    // Campionamento metriche sempre attivo (thread OS dedicato).
    let metrics = crate::services::metrics::MetricsService::start();
    // Storico appunti sempre attivo (solo in memoria), thread OS dedicato.
    let clipboard = crate::services::clipboard::ClipboardHistory::start();
    // RickyAI: `of-free serve` acceso in background (solo su 127.0.0.1). Se il
    // binario non c'è, il supervisore lo dice nello stato senza far rumore.
    let ai = crate::services::rickyai::AiService::start(config.clone(), bus.clone());
    // Campionatore sensori a bassa frequenza SEMPRE attivo, solo per gli alert
    // termici/batteria: il poller "sensors" gira solo con la dashboard aperta,
    // ma questi alert devono scattare anche in background. Topic "sensorsbg"
    // (base diverso da "sensors": non arriva alle dashboard, niente flicker).
    {
        let bus_bg = bus.clone();
        tokio::spawn(async move {
            loop {
                let payload = crate::adapters::sensors::read_for_alerts().await;
                bus_bg.publish("sensorsbg", payload);
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
    }
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
        metrics,
        clipboard,
        ai,
    };

    // Il permesso si legge dal gruppo in cui la rotta è scritta, non dal corpo
    // dell'handler. Aggiungere un endpoint significa sceglierne uno: non esiste
    // una posizione "senza controllo" in cui finirci per distrazione.
    let local_layer = middleware::from_fn(require_loopback);
    let write_layer = middleware::from_fn_with_state(state.clone(), require_write);

    // (1) Solo dal desktop, sempre. Distruttive su disco, apertura di file/URL
    // locali, notifiche, e gli interruttori che *concedono* permessi: il
    // controllo remoto non le sblocca, altrimenti si auto-concederebbe.
    let local_only = Router::new()
        .route("/api/disks/eject", post(disk_eject))
        .route("/api/disks/format", post(disk_format))
        .route("/api/fs/compare/apply", post(fs_compare_apply))
        .route("/api/push/test", post(push_test))
        .route("/api/drop/received", get(drop_received))
        .route("/api/drop/open/{name}", post(drop_open_file))
        .route("/api/drop/reveal/{name}", post(drop_reveal_file))
        .route("/api/drop/received/{name}", axum::routing::delete(drop_received_delete))
        .route("/api/drop/open-folder", post(drop_open_folder))
        .route("/api/config/remote-control", post(set_remote_control))
        // Decide quale binario il tool avvia da solo e con quale file di
        // chiavi: sta accanto agli altri interruttori che concedono qualcosa.
        .route("/api/ai/config", post(ai_config_set))
        .route("/api/system/open-accessibility", post(open_accessibility))
        .route("/api/system/color-meter", post(open_color_meter))
        .route("/api/system/open-url", post(open_url))
        .layer(local_layer.clone());

    // (2) Modificano il sistema: locale, oppure LAN col controllo remoto
    // attivo. Ci sono anche delle GET, ed è voluto — leggere un `.env` o
    // sfogliare il filesystem espone segreti quanto scriverci.
    let write = Router::new()
        .route("/api/pollers/{topic}/interval", post(set_interval))
        .route("/api/processes/kill", post(kill_process))
        .route("/api/docker/images/prune", post(docker_prune_images))
        .route("/api/docker/{id}/action", post(docker_action))
        .route("/api/docker/{id}/logs", post(docker_logs))
        .route("/api/tools/{id}/launch", post(launch_tool))
        .route("/api/tools/{id}/path", post(set_tool_path))
        .route("/api/git/fetch", post(git_fetch))
        .route("/api/git/pull", post(git_pull))
        .route("/api/git/checkout", post(git_checkout))
        .route("/api/git/delete-branch", post(git_delete_branch))
        .route("/api/git/checkout-commit", post(git_checkout_commit))
        .route("/api/git/revert", post(git_revert))
        .route("/api/git/cherry-pick", post(git_cherry_pick))
        .route("/api/node/pm", post(node_set_pm))
        .route("/api/node/run", post(node_run))
        .route("/api/tasks/{id}/stop", post(task_stop))
        .route("/api/tasks/clear-finished", post(tasks_clear_finished))
        .route("/api/dotnet/select", post(dotnet_select))
        .route("/api/dotnet/run", post(dotnet_run))
        .route("/api/runner/run", post(runner_run))
        .route("/api/launch/bundles/delete", post(launch_bundle_delete))
        .route("/api/launch/run", post(launch_run))
        .route("/api/snippets/delete", post(snippets_delete))
        .route("/api/snippets/run", post(snippets_run))
        .route("/api/ssh/hosts/delete", post(ssh_host_delete))
        .route("/api/ssh/run", post(ssh_run))
        .route("/api/clipboard/copy", post(clipboard_copy))
        .route("/api/clipboard/send", post(clipboard_send))
        .route("/api/clipboard/record", post(clipboard_record))
        .route("/api/clipboard/pin", post(clipboard_pin))
        .route("/api/clipboard/delete", post(clipboard_delete))
        .route("/api/clipboard/clear", post(clipboard_clear))
        .route("/api/clipboard/enabled", post(clipboard_set_enabled))
        .route("/api/services/{id}", axum::routing::delete(services_delete))
        .route("/api/services/{id}/toggle", post(services_toggle))
        .route("/api/alerts/ack", post(alerts_ack))
        .route("/api/fs/entries", get(fs_entries))
        .route("/api/fs/compare", post(fs_compare))
        .route("/api/fs/compare/children", post(fs_compare_children))
        .route("/api/env/files", get(env_files))
        .route("/api/env/read", get(env_read))
        .route("/api/env/activate", post(env_activate))
        .route("/api/logtail/start", post(logtail_start))
        .route("/api/logtail/{id}/stop", post(logtail_stop))
        .route("/api/net/ping", post(net_ping))
        .route("/api/net/dns", post(net_dns))
        .route("/api/net/portcheck", post(net_portcheck))
        .route("/api/net/scan", post(net_scan))
        .route("/api/net/traceroute", post(net_traceroute))
        .route("/api/config/anti-idle", post(set_anti_idle))
        // Una chat esce dalla macchina e consuma quote condivise fra tutti i
        // client del tool: è una scrittura, non una lettura, e dal telefono
        // segue il "Controllo remoto" come tutto il resto.
        .route("/api/ai/chat", post(ai_chat))
        .route("/api/ai/restart", post(ai_restart))
        .layer(write_layer.clone());

    // (3) Sola lettura: qualsiasi device abbinato. Dove GET e POST condividono
    // il path il layer sta sul singolo metodo, così la lettura resta aperta
    // mentre la scrittura no.
    let read = Router::new()
        .route("/api/health", get(health))
        .route("/api/lan", get(lan_info))
        .route("/api/lan/qr.svg", get(lan_qr))
        .route("/api/pair", post(pair))
        .route("/api/log", post(client_log))
        .route("/api/processes/heavy", get(heavy_processes))
        .route("/api/metrics/history", get(metrics_history))
        .route("/api/ports", get(list_ports))
        .route("/api/disks", get(list_disks))
        .route("/api/docker", get(docker_state))
        .route("/api/docker/images", get(docker_images))
        .route("/api/tools", get(list_tools))
        .route("/api/fs/dirs", get(fs_dirs))
        .route("/api/projects/scan", get(projects_scan))
        .route(
            "/api/projects/pinned",
            get(pinned_get).merge(post(pinned_set).layer(write_layer.clone())),
        )
        .route("/api/git/info", get(git_info))
        .route("/api/git/branches", get(git_branches))
        .route("/api/git/commits", get(git_commits))
        .route("/api/node/info", get(node_info))
        .route("/api/tasks", get(tasks_list))
        .route("/api/tasks/{id}/log", get(task_log))
        .route("/api/dotnet/info", get(dotnet_info))
        .route("/api/runner/info", get(runner_info))
        .route(
            "/api/launch/bundles",
            get(launch_bundles_list).merge(post(launch_bundle_upsert).layer(write_layer.clone())),
        )
        .route(
            "/api/snippets",
            get(snippets_list).merge(post(snippets_upsert).layer(write_layer.clone())),
        )
        .route(
            "/api/ssh/hosts",
            get(ssh_hosts_list).merge(post(ssh_host_upsert).layer(write_layer.clone())),
        )
        .route("/api/clipboard/history", get(clipboard_history))
        .route("/api/clipboard/blob", get(clipboard_blob))
        .route(
            "/api/services",
            get(services_get).merge(post(services_upsert).layer(write_layer.clone())),
        )
        .route("/api/alerts", get(alerts_get))
        .route(
            "/api/alerts/config",
            get(alerts_config_get).merge(post(alerts_config_set).layer(write_layer.clone())),
        )
        .route("/api/scheduler", get(scheduler_list))
        .route("/api/scheduler/detail", get(scheduler_detail))
        .route("/api/logtail", get(logtail_list))
        .route(
            "/api/push",
            get(push_get).merge(post(push_set).layer(local_layer.clone())),
        )
        // drop/hello, /send e /text restano senza permesso di scrittura di
        // proposito: sono il canale Drop verso i device abbinati e fra hub,
        // autenticato in auth_middleware dalla reciprocità della discovery UDP.
        // Chiuderli qui spegnerebbe Drop dal telefono.
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
        .route(
            "/api/config/docker-host",
            get(docker_host_get).merge(post(docker_host_set).layer(local_layer.clone())),
        )
        // Lo stato è una lettura: il telefono deve poter vedere se è pronto.
        .route("/api/ai/status", get(ai_status))
        .route("/api/system/accessibility", get(accessibility_status))
        .route("/ws", get(ws::ws_handler));

    let api = local_only
        .merge(write)
        .merge(read)
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let mut app = Router::new()
        .merge(api)
        .fallback(static_assets)
        .with_state(state.clone());

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

    Ok(ServerInfo { port, lan_enabled, state })
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

async fn lan_info(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Json<serde_json::Value> {
    let cfg = state.config.get();
    let urls: Vec<String> = netinfo::lan_ips()
        .into_iter()
        .map(|ip| format!("http://{ip}:{}", state.port))
        .collect();
    Json(json!({
        "ok": true,
        // `remote`: la richiesta arriva da un device LAN (non dal desktop). La UI
        // lo usa per disabilitare i toggle finché il controllo remoto non è attivo.
        "data": { "urls": urls, "port": state.port, "lanEnabled": cfg.lan_enabled, "remoteControlEnabled": cfg.remote_control_enabled, "antiIdleEnabled": cfg.anti_idle_enabled, "remote": !peer.ip().is_loopback() }
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
struct MetricsHistoryQuery {
    hours: Option<u32>,
}

/// Storico metriche (CPU/RAM/disco di sistema) delle ultime `hours` ore.
/// La query SQLite è bloccante: fuori dal runtime async.
async fn metrics_history(
    State(state): State<ServerState>,
    axum::extract::Query(query): axum::extract::Query<MetricsHistoryQuery>,
) -> Json<serde_json::Value> {
    let hours = query.hours.unwrap_or(24);
    let metrics = state.metrics.clone();
    let samples = tokio::task::spawn_blocking(move || metrics.history(hours))
        .await
        .unwrap_or_default();
    Json(json!({ "ok": true, "data": { "samples": samples, "hours": hours } }))
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
    Json(body): Json<GitActionBody>,
) -> Response {
    match crate::services::git::fetch(&body.path).await {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(e) => git_error_response(e),
    }
}

async fn git_pull(
    Json(body): Json<GitActionBody>,
) -> Response {
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
    Json(body): Json<CheckoutBody>,
) -> Response {
    match crate::services::git::checkout(&body.path, &body.branch).await {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(e) => git_error_response(e),
    }
}

#[derive(Deserialize)]
struct DeleteBranchBody {
    path: String,
    branch: String,
    #[serde(default)]
    force: bool,
    /// Nome del remote (es. "origin") da cui eliminare anche il branch; assente
    /// = solo locale.
    #[serde(default)]
    remote: Option<String>,
}

async fn git_delete_branch(
    Json(body): Json<DeleteBranchBody>,
) -> Response {
    match crate::services::git::delete_branch(
        &body.path,
        &body.branch,
        body.force,
        body.remote.as_deref(),
    )
    .await
    {
        Ok(branches) => Json(json!({ "ok": true, "data": { "branches": branches } })).into_response(),
        Err(e) => git_error_response(e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitsQuery {
    path: String,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    skip: Option<u32>,
    /// Ref (branch/tag/hash) di cui elencare i commit; assente = HEAD.
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
}

async fn git_commits(
    axum::extract::Query(query): axum::extract::Query<CommitsQuery>,
) -> Response {
    match crate::services::git::commits(
        &query.path,
        query.git_ref.as_deref(),
        query.limit.unwrap_or(50),
        query.skip.unwrap_or(0),
    )
    .await
    {
        Ok(commits) => Json(json!({ "ok": true, "data": { "commits": commits } })).into_response(),
        Err(e) => git_error_response(e),
    }
}

#[derive(Deserialize)]
struct CheckoutCommitBody {
    path: String,
    hash: String,
}

async fn git_checkout_commit(
    Json(body): Json<CheckoutCommitBody>,
) -> Response {
    match crate::services::git::checkout_commit(&body.path, &body.hash).await {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(e) => git_error_response(e),
    }
}

async fn git_revert(
    Json(body): Json<CheckoutCommitBody>,
) -> Response {
    match crate::services::git::revert_commit(&body.path, &body.hash).await {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(e) => git_error_response(e),
    }
}

async fn git_cherry_pick(
    Json(body): Json<CheckoutCommitBody>,
) -> Response {
    match crate::services::git::cherry_pick_commit(&body.path, &body.hash).await {
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
    Json(body): Json<NodeRunBody>,
) -> Response {
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
    Json(body): Json<DotnetRunBody>,
) -> Response {
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

// ---------- runner generico (python / rust / tauri / flutter) ----------

#[derive(Deserialize)]
struct RunnerQuery {
    path: Option<String>,
    kind: Option<String>,
}

async fn runner_info(axum::extract::Query(query): axum::extract::Query<RunnerQuery>) -> Response {
    let (Some(path), Some(kind)) = (query.path, query.kind) else {
        return internal_error("parametri path/kind mancanti".into());
    };
    match crate::services::runners::inspect(&kind, &path) {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
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
struct RunnerRunBody {
    path: String,
    kind: String,
    action_id: String,
}

async fn runner_run(
    State(state): State<ServerState>,
    Json(body): Json<RunnerRunBody>,
) -> Response {
    // Il client manda solo l'id: il server rigenera lo spec verificato, così il
    // programma eseguito non è mai controllato dal chiamante.
    let spec = match crate::services::runners::resolve(&body.kind, &body.path, &body.action_id) {
        Ok(s) => s,
        Err(message) => return internal_error(message),
    };
    let arg_refs: Vec<&str> = spec.args.iter().map(String::as_str).collect();
    let label = format!("{} {} — {}", body.kind, spec.label, short_name(&body.path));
    match state.tasks.spawn(&label, &spec.program, &arg_refs, &body.path) {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(message) => internal_error(message),
    }
}

// ---------- profili di avvio composito ----------

#[derive(Deserialize)]
struct LaunchIdBody {
    id: String,
}

async fn launch_bundles_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "bundles": state.config.get().launch_bundles } }))
}

#[derive(Deserialize)]
struct LaunchBundleBody {
    /// Assente = nuovo profilo (id generato dal server).
    #[serde(default)]
    id: Option<String>,
    name: String,
    steps: Vec<crate::services::launch::LaunchStep>,
}

async fn launch_bundle_upsert(
    State(state): State<ServerState>,
    Json(body): Json<LaunchBundleBody>,
) -> Response {
    // Un profilo contiene comandi che verranno eseguiti: un remoto non deve
    // poterne piantare uno senza il controllo remoto attivo.
    let bundle = crate::services::launch::LaunchBundle {
        id: body.id.unwrap_or_else(crate::services::launch::new_id),
        name: body.name,
        steps: body.steps,
    };
    let bundle = match bundle.sanitized() {
        Ok(b) => b,
        Err(message) => return internal_error(message),
    };
    state.config.update(|c| {
        if let Some(existing) = c.launch_bundles.iter_mut().find(|b| b.id == bundle.id) {
            *existing = bundle.clone();
        } else {
            c.launch_bundles.push(bundle.clone());
        }
    });
    Json(json!({ "ok": true, "data": { "bundles": state.config.get().launch_bundles } })).into_response()
}

async fn launch_bundle_delete(
    State(state): State<ServerState>,
    Json(body): Json<LaunchIdBody>,
) -> Response {
    state.config.update(|c| c.launch_bundles.retain(|b| b.id != body.id));
    Json(json!({ "ok": true, "data": { "bundles": state.config.get().launch_bundles } })).into_response()
}

async fn launch_run(
    State(state): State<ServerState>,
    Json(body): Json<LaunchIdBody>,
) -> Response {
    let Some(bundle) = state.config.get().launch_bundles.into_iter().find(|b| b.id == body.id)
    else {
        return internal_error("profilo non trovato".into());
    };
    let mut tasks = Vec::new();
    let mut errors = Vec::new();
    for step in &bundle.steps {
        let label = format!("{} · {}", bundle.name, step.label);
        match state.tasks.spawn_shell(&label, &step.command, &expand_tilde(&step.cwd)) {
            Ok(info) => tasks.push(info),
            Err(e) => errors.push(format!("{}: {e}", step.label)),
        }
    }
    Json(json!({ "ok": true, "data": { "tasks": tasks, "errors": errors } })).into_response()
}

// ---------- snippet / comandi salvati ----------

async fn snippets_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "snippets": state.config.get().snippets } }))
}

#[derive(Deserialize)]
struct SnippetBody {
    #[serde(default)]
    id: Option<String>,
    name: String,
    command: String,
    #[serde(default)]
    cwd: String,
}

async fn snippets_upsert(
    State(state): State<ServerState>,
    Json(body): Json<SnippetBody>,
) -> Response {
    let snippet = crate::services::snippets::Snippet {
        id: body.id.unwrap_or_else(crate::services::snippets::new_id),
        name: body.name,
        command: body.command,
        cwd: body.cwd,
    };
    let snippet = match snippet.sanitized() {
        Ok(s) => s,
        Err(message) => return internal_error(message),
    };
    let mut too_many = false;
    state.config.update(|c| {
        if let Some(existing) = c.snippets.iter_mut().find(|s| s.id == snippet.id) {
            *existing = snippet.clone();
        } else if c.snippets.len() >= crate::services::snippets::MAX_SNIPPETS {
            too_many = true;
        } else {
            c.snippets.push(snippet.clone());
        }
    });
    if too_many {
        return internal_error(format!(
            "troppi snippet (max {})",
            crate::services::snippets::MAX_SNIPPETS
        ));
    }
    Json(json!({ "ok": true, "data": { "snippets": state.config.get().snippets } })).into_response()
}

async fn snippets_delete(
    State(state): State<ServerState>,
    Json(body): Json<LaunchIdBody>,
) -> Response {
    state.config.update(|c| c.snippets.retain(|s| s.id != body.id));
    Json(json!({ "ok": true, "data": { "snippets": state.config.get().snippets } })).into_response()
}

async fn snippets_run(
    State(state): State<ServerState>,
    Json(body): Json<LaunchIdBody>,
) -> Response {
    let Some(snippet) = state.config.get().snippets.into_iter().find(|s| s.id == body.id) else {
        return internal_error("snippet non trovato".into());
    };
    let cwd = if snippet.cwd.is_empty() {
        home_dir_string()
    } else {
        expand_tilde(&snippet.cwd)
    };
    match state.tasks.spawn_shell(&snippet.name, &snippet.command, &cwd) {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(message) => internal_error(message),
    }
}

// ---------- ssh quick-connect ----------

fn home_dir_string() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string())
}

/// Espande un `~`/`~/…` iniziale nella home dell'utente: la shell lo farebbe da
/// sé, ma qui la cartella è passata a `current_dir` senza shell, quindi un path
/// come "~/Documents/…" darebbe "No such file or directory". Nessun altro
/// glob/variabile viene toccato.
fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return home_dir_string();
    }
    let home = home_dir_string();
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{}/{}", home.trim_end_matches('/'), rest);
    }
    if let Some(rest) = path.strip_prefix("~\\") {
        return format!("{}\\{}", home.trim_end_matches(['/', '\\']), rest);
    }
    path.to_string()
}

async fn ssh_hosts_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "hosts": state.config.get().ssh_hosts } }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SshHostBody {
    #[serde(default)]
    id: Option<String>,
    name: String,
    host: String,
    #[serde(default)]
    default_command: String,
}

async fn ssh_host_upsert(
    State(state): State<ServerState>,
    Json(body): Json<SshHostBody>,
) -> Response {
    let host = crate::services::ssh::SshHost {
        id: body.id.unwrap_or_else(crate::services::ssh::new_id),
        name: body.name,
        host: body.host,
        default_command: body.default_command,
    };
    let host = match host.sanitized() {
        Ok(h) => h,
        Err(message) => return internal_error(message),
    };
    let mut too_many = false;
    state.config.update(|c| {
        if let Some(existing) = c.ssh_hosts.iter_mut().find(|h| h.id == host.id) {
            *existing = host.clone();
        } else if c.ssh_hosts.len() >= crate::services::ssh::MAX_HOSTS {
            too_many = true;
        } else {
            c.ssh_hosts.push(host.clone());
        }
    });
    if too_many {
        return internal_error(format!("troppi host (max {})", crate::services::ssh::MAX_HOSTS));
    }
    Json(json!({ "ok": true, "data": { "hosts": state.config.get().ssh_hosts } })).into_response()
}

async fn ssh_host_delete(
    State(state): State<ServerState>,
    Json(body): Json<LaunchIdBody>,
) -> Response {
    state.config.update(|c| c.ssh_hosts.retain(|h| h.id != body.id));
    Json(json!({ "ok": true, "data": { "hosts": state.config.get().ssh_hosts } })).into_response()
}

#[derive(Deserialize)]
struct SshRunBody {
    id: String,
    command: String,
}

async fn ssh_run(
    State(state): State<ServerState>,
    Json(body): Json<SshRunBody>,
) -> Response {
    let Some(host) = state.config.get().ssh_hosts.into_iter().find(|h| h.id == body.id) else {
        return internal_error("host non trovato".into());
    };
    let command = body.command.trim().to_string();
    if command.is_empty() {
        return internal_error("inserisci un comando da eseguire".into());
    }
    let args = crate::services::ssh::run_args(&host.host, &command);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let label = format!("ssh {} — {}", host.host, command);
    match state.tasks.spawn(&label, "ssh", &arg_refs, &home_dir_string()) {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(message) => internal_error(message),
    }
}

// ---------- storico appunti ----------

fn clipboard_payload(state: &ServerState) -> serde_json::Value {
    json!({
        "entries": state.clipboard.list(),
        "enabled": state.clipboard.enabled(),
        "supported": crate::adapters::clipboard::supported(),
    })
}

async fn clipboard_history(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": clipboard_payload(&state) }))
}

#[derive(Deserialize)]
struct ClipIdBody {
    id: u64,
}

async fn clipboard_copy(
    State(state): State<ServerState>,
    Json(body): Json<ClipIdBody>,
) -> Response {
    // Scrivere negli appunti tocca lo stato del sistema: stessa guardia delle
    // altre scritture (locale, o LAN col controllo remoto attivo).
    match state.clipboard.copy_to_clipboard(body.id) {
        Ok(()) => Json(json!({ "ok": true, "data": { "copied": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

#[derive(Deserialize)]
struct ClipBlobQuery {
    id: u64,
    /// Indice del file in una voce multi-file (default 0; ignorato per immagini).
    #[serde(default)]
    i: usize,
}

async fn clipboard_blob(
    State(state): State<ServerState>,
    axum::extract::Query(query): axum::extract::Query<ClipBlobQuery>,
) -> Response {
    let Some(serve) = state.clipboard.blob_for(query.id, query.i) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": { "code": "BLOB_NOT_FOUND", "message": "contenuto non più disponibile", "retryable": false }
            })),
        )
            .into_response();
    };
    let file = match tokio::fs::File::open(&serve.path).await {
        Ok(f) => f,
        Err(e) => return internal_error(format!("contenuto non leggibile: {e}")),
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    let mut headers = HeaderMap::new();
    if let Ok(v) = axum::http::HeaderValue::from_str(&serve.mime) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if !serve.inline {
        // RFC 5987 per i nomi non-ASCII; fallback ASCII per i client vecchi.
        let ascii: String = serve
            .name
            .chars()
            .map(|c| if c.is_ascii() && c != '"' && c != '\\' { c } else { '_' })
            .collect();
        let encoded: String = serve
            .name
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        let cd = format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}");
        if let Ok(v) = axum::http::HeaderValue::from_str(&cd) {
            headers.insert(header::CONTENT_DISPOSITION, v);
        }
    }
    (headers, Body::from_stream(stream)).into_response()
}

/// Invia il testo di una voce (o l'attuale clipboard di sistema) a un peer come
/// "clipboard di rete": sul ricevente finisce nel suo storico appunti.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipSendBody {
    to: String,
    from_name: String,
    /// Voce dello storico da inviare; assente = clipboard di sistema attuale.
    #[serde(default)]
    id: Option<u64>,
}

async fn clipboard_send(
    State(state): State<ServerState>,
    Json(body): Json<ClipSendBody>,
) -> Response {
    let text = match body.id {
        Some(id) => match state.clipboard.text_of(id) {
            Some(t) => t,
            None => return internal_error("voce non trovata".into()),
        },
        None => match crate::adapters::clipboard::read_text() {
            Some(t) => t,
            None => return internal_error("clipboard di sistema vuota".into()),
        },
    };
    // Verso un hub remoto non c'è il canale "clipboard": ripiega sul testo Drop
    // normale (arriva comunque, come testo ricevuto).
    if let Some(hub) = state.drop.remote_hub(&body.to) {
        return match state.drop.proxy_send_text(&hub, &body.from_name, &text).await {
            Ok(()) => Json(json!({ "ok": true, "data": { "sent": true } })).into_response(),
            Err(message) => internal_error(message),
        };
    }
    match state.drop.send_clipboard(&body.to, &body.from_name, &text) {
        Ok(()) => Json(json!({ "ok": true, "data": { "sent": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

/// Registra nello storico locale un testo ricevuto via clipboard di rete
/// (nessuna scrittura sulla clipboard di sistema: la applica l'utente con Copia).
#[derive(Deserialize)]
struct ClipTextBody {
    text: String,
}

async fn clipboard_record(
    State(state): State<ServerState>,
    Json(body): Json<ClipTextBody>,
) -> Json<serde_json::Value> {
    state.clipboard.record(body.text);
    Json(json!({ "ok": true, "data": clipboard_payload(&state) }))
}

#[derive(Deserialize)]
struct ClipPinBody {
    id: u64,
    pinned: bool,
}

async fn clipboard_pin(
    State(state): State<ServerState>,
    Json(body): Json<ClipPinBody>,
) -> Json<serde_json::Value> {
    let ok = state.clipboard.set_pinned(body.id, body.pinned);
    Json(json!({ "ok": ok, "data": clipboard_payload(&state) }))
}

async fn clipboard_delete(
    State(state): State<ServerState>,
    Json(body): Json<ClipIdBody>,
) -> Json<serde_json::Value> {
    let ok = state.clipboard.delete(body.id);
    Json(json!({ "ok": ok, "data": clipboard_payload(&state) }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipClearBody {
    keep_pinned: Option<bool>,
}

async fn clipboard_clear(
    State(state): State<ServerState>,
    Json(body): Json<ClipClearBody>,
) -> Json<serde_json::Value> {
    state.clipboard.clear(body.keep_pinned.unwrap_or(false));
    Json(json!({ "ok": true, "data": clipboard_payload(&state) }))
}

#[derive(Deserialize)]
struct ClipEnabledBody {
    enabled: bool,
}

async fn clipboard_set_enabled(
    State(state): State<ServerState>,
    Json(body): Json<ClipEnabledBody>,
) -> Json<serde_json::Value> {
    state.clipboard.set_enabled(body.enabled);
    Json(json!({ "ok": true, "data": { "enabled": state.clipboard.enabled() } }))
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
    Json(body): Json<EjectBody>,
) -> Response {
    match crate::adapters::disks::eject(&body.mount_point).await {
        Ok(()) => Json(json!({ "ok": true, "data": { "ejected": true } })).into_response(),
        Err(e) => disk_error_response(e),
    }
}

async fn disk_format(
    Json(req): Json<crate::adapters::disks::FormatRequest>,
) -> Response {
    match crate::adapters::disks::format(req).await {
        Ok(()) => Json(json!({ "ok": true, "data": { "formatted": true } })).into_response(),
        Err(e) => disk_error_response(e),
    }
}

// ---------- docker ----------

/// Stato Docker + lista container: read-only, aperto come la lista porte.
/// Include l'host configurato (vuoto = daemon locale) così la UI lo mostra.
async fn docker_state(State(state): State<ServerState>) -> Json<serde_json::Value> {
    let host = state.config.get().docker_host;
    let docker = crate::adapters::docker::state(host.as_deref()).await;
    let mut data = serde_json::to_value(&docker).unwrap_or_else(|_| json!({}));
    data["host"] = json!(host);
    Json(json!({ "ok": true, "data": data }))
}

/// Immagini locali: read-only.
async fn docker_images(State(state): State<ServerState>) -> Json<serde_json::Value> {
    let host = state.config.get().docker_host;
    let images = crate::adapters::docker::images(host.as_deref()).await;
    Json(json!({ "ok": true, "data": { "images": images } }))
}

/// Rimuove le immagini non usate (`docker image prune -a`): azione di scrittura.
async fn docker_prune_images(
    State(state): State<ServerState>,
) -> Response {
    let host = state.config.get().docker_host;
    match crate::adapters::docker::prune_images(host.as_deref()).await {
        Ok(summary) => Json(json!({ "ok": true, "data": { "summary": summary } })).into_response(),
        Err(crate::adapters::docker::DockerError::Failed(msg)) => internal_error(msg),
        Err(_) => internal_error("prune fallito".into()),
    }
}

/// Host Docker remoto configurato (vuoto = locale).
async fn docker_host_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "host": state.config.get().docker_host } }))
}

#[derive(Deserialize)]
struct DockerHostBody {
    /// Vuoto/null = torna al daemon locale.
    host: Option<String>,
}

/// Configura l'host Docker remoto. Solo dal desktop: cambia dove puntano tutte
/// le azioni Docker. L'host viene provato davvero prima di accettarlo: uno
/// schema valido non basta, se dall'altra parte non risponde nessun daemon
/// l'utente deve vedere l'errore invece di credere di esserci collegato.
async fn docker_host_set(
    State(state): State<ServerState>,
    Json(body): Json<DockerHostBody>,
) -> Response {
    let host = body.host.map(|h| h.trim().to_string()).filter(|h| !h.is_empty());
    if let Some(h) = &host {
        if !crate::adapters::docker::valid_host(h) {
            return internal_error(
                "Host non valido: usa uno schema come tcp://, ssh:// o unix://".into(),
            );
        }
    }
    if let Err(message) = crate::adapters::docker::probe(host.as_deref()).await {
        tracing::warn!(host = ?host, %message, "docker host non raggiungibile");
        let where_ = match &host {
            Some(h) => format!("Non riesco a collegarmi a {h}"),
            None => "Non riesco a collegarmi al Docker locale".to_string(),
        };
        return internal_error(format!("{where_}: {message}"));
    }
    state.config.update(|c| c.docker_host = host.clone());
    tracing::info!(host = ?host, "docker host aggiornato");
    Json(json!({ "ok": true, "data": { "host": host } })).into_response()
}

#[derive(Deserialize)]
struct DockerActionBody {
    action: String, // start | stop | restart
}

/// start/stop/restart di un container: azione di scrittura (guardia write_allowed).
async fn docker_action(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(body): Json<DockerActionBody>,
) -> Response {
    use crate::adapters::docker::DockerError;
    let host = state.config.get().docker_host;
    match crate::adapters::docker::action(host.as_deref(), &id, &body.action).await {
        Ok(()) => Json(json!({ "ok": true, "data": { "done": true } })).into_response(),
        Err(DockerError::InvalidRef) => internal_error("id container non valido".into()),
        Err(DockerError::InvalidAction) => internal_error("azione non valida".into()),
        Err(DockerError::Failed(msg)) => internal_error(msg),
    }
}

/// Log del container in streaming come task (topic WS `task:{id}`): read
/// sensibile, dietro la stessa guardia degli altri comandi che spawnano processi.
async fn docker_logs(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
    let host = state.config.get().docker_host;
    let Some((program, args)) = crate::adapters::docker::logs_command(host.as_deref(), &id) else {
        return internal_error("id container non valido".into());
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match state.tasks.spawn(&format!("docker logs {id}"), program, &arg_refs, ".") {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
        Err(message) => internal_error(message),
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

async fn alerts_config_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": state.config.get().alert_thresholds }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertConfigBody {
    cpu_pct: f64,
    mem_pct: f64,
    temp_c: f64,
    battery_pct: f64,
    temp_enabled: bool,
    battery_enabled: bool,
}

async fn alerts_config_set(
    State(state): State<ServerState>,
    Json(body): Json<AlertConfigBody>,
) -> Response {
    let thresholds = crate::config::AlertThresholds {
        cpu_pct: body.cpu_pct.clamp(10.0, 100.0),
        mem_pct: body.mem_pct.clamp(10.0, 100.0),
        temp_c: body.temp_c.clamp(30.0, 120.0),
        battery_pct: body.battery_pct.clamp(1.0, 100.0),
        temp_enabled: body.temp_enabled,
        battery_enabled: body.battery_enabled,
    };
    state.config.update(|c| c.alert_thresholds = thresholds.clone());
    Json(json!({ "ok": true, "data": state.config.get().alert_thresholds })).into_response()
}

/// Elenco (sola lettura) di cron/launchd/schtasks. On-demand come porte/servizi.
async fn scheduler_list() -> Json<serde_json::Value> {
    let listing = crate::adapters::scheduler::list().await;
    Json(json!({ "ok": true, "data": listing }))
}

#[derive(Deserialize)]
struct SchedDetailQuery {
    source: String,
    id: String,
}

/// Dettagli di una voce pianificata (sola lettura): plist di launchd, query
/// verbosa di schtasks. crontab calcola il prossimo avvio lato client.
async fn scheduler_detail(
    axum::extract::Query(query): axum::extract::Query<SchedDetailQuery>,
) -> Json<serde_json::Value> {
    let lines = crate::adapters::scheduler::detail(&query.source, &query.id).await;
    Json(json!({ "ok": true, "data": { "lines": lines } }))
}

#[derive(Deserialize)]
struct RemoteControlBody {
    enabled: bool,
}

/// Attivabile anche da un device LAN abbinato: è il toggle che sblocca gli altri
/// dal telefono. Il pairing (cookie) è già la barriera di fiducia; parte comunque
/// spento, così azioni distruttive non capitano per un tocco accidentale.
async fn set_remote_control(
    State(state): State<ServerState>,
    Json(body): Json<RemoteControlBody>,
) -> Response {
    state.config.update(|c| c.remote_control_enabled = body.enabled);
    tracing::info!(enabled = body.enabled, "remote control aggiornato");
    Json(json!({ "ok": true, "data": { "remoteControlEnabled": body.enabled } })).into_response()
}

/// Come le altre scritture: locale, oppure LAN col controllo remoto attivo. Così
/// dal telefono si può gestire l'anti-inattività una volta sbloccato il remoto.
async fn set_anti_idle(
    State(state): State<ServerState>,
    Json(body): Json<RemoteControlBody>,
) -> Response {
    state.config.update(|c| c.anti_idle_enabled = body.enabled);
    tracing::info!(enabled = body.enabled, "anti-idle aggiornato");
    Json(json!({ "ok": true, "data": { "antiIdleEnabled": body.enabled } })).into_response()
}

async fn accessibility_status() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": crate::adapters::accessibility::status() }))
}

/// Apre il pannello Accessibilità delle impostazioni di sistema (solo macOS).
async fn open_accessibility() -> Response {
    match crate::adapters::accessibility::open_settings() {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

/// Apre il Colorimetro digitale di macOS (fallback all'EyeDropper del color picker).
async fn open_color_meter() -> Response {
    match crate::adapters::accessibility::open_color_meter() {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

#[derive(Deserialize)]
struct OpenUrlBody {
    url: String,
}

/// Apre un URL nel browser di sistema del desktop. Serve alla webview Tauri
/// (dove `window.open` è un no-op): i browser LAN aprono da soli con window.open.
async fn open_url(
    Json(body): Json<OpenUrlBody>,
) -> Response {
    let url = body.url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return internal_error("URL non valido".into());
    }
    match tauri_plugin_opener::open_url(url, None::<String>) {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(e) => internal_error(e.to_string()),
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

/// Log bufferizzato di un task (per riaprirlo dopo la fine o da un'altra
/// sezione). 404 se il task è stato ripulito.
async fn task_log(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    match state.tasks.log(&id) {
        Some(lines) => Json(json!({ "ok": true, "data": { "lines": lines } })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": { "code": "PATH_NOT_FOUND", "message": "task non trovato", "retryable": false }
            })),
        )
            .into_response(),
    }
}

async fn task_stop(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
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
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Response {
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompareBody {
    left: String,
    right: String,
    /// Nomi di file/cartelle da saltare (es. ".git", "node_modules").
    #[serde(default)]
    excludes: Vec<String>,
}

/// Differenze tra due alberature. Elenca il contenuto di cartelle arbitrarie:
/// stessa guardia della navigazione filesystem.
async fn fs_compare(
    Json(body): Json<CompareBody>,
) -> Response {
    match crate::services::fscompare::compare(body.left, body.right, body.excludes).await {
        Ok(result) => Json(json!({ "ok": true, "data": result })).into_response(),
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
struct CompareChildrenBody {
    left: String,
    right: String,
    rel_path: String,
    #[serde(default)]
    excludes: Vec<String>,
}

/// Contenuto di una cartella comparsa nel confronto come voce unica: permette
/// alla UI di aprirla e agire sui singoli file.
async fn fs_compare_children(
    Json(body): Json<CompareChildrenBody>,
) -> Response {
    match crate::services::fscompare::children(body.left, body.right, body.rel_path, body.excludes)
        .await
    {
        Ok(entries) => Json(json!({ "ok": true, "data": { "entries": entries } })).into_response(),
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
struct CompareApplyBody {
    left: String,
    right: String,
    rel_path: String,
    /// toRight | toLeft | delete
    action: String,
    /// Solo per "delete": da quale dei due rami eliminare.
    side: Option<crate::services::fscompare::Side>,
}

/// Applica la scelta dell'utente su una differenza: copia da un lato all'altro
/// oppure elimina. Scrive (e cancella) sul disco: solo dal desktop, come il
/// format dei dischi — non basta il controllo remoto.
async fn fs_compare_apply(
    Json(body): Json<CompareApplyBody>,
) -> Response {
    use crate::services::fscompare::{self, Side};
    let result = match body.action.as_str() {
        "toRight" => {
            fscompare::copy_entry(body.left, body.right, body.rel_path, "di sinistra", "di destra")
                .await
        }
        "toLeft" => {
            fscompare::copy_entry(body.right, body.left, body.rel_path, "di destra", "di sinistra")
                .await
        }
        "delete" => match body.side {
            Some(Side::Left) => fscompare::delete_entry(body.left, body.rel_path, "di sinistra").await,
            Some(Side::Right) => fscompare::delete_entry(body.right, body.rel_path, "di destra").await,
            None => Err("indica da quale lato eliminare".to_string()),
        },
        _ => Err("azione non valida".to_string()),
    };
    match result {
        Ok(()) => Json(json!({ "ok": true, "data": { "done": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn env_files(
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Response {
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
    axum::extract::Query(query): axum::extract::Query<EnvQuery>,
) -> Response {
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
    Json(body): Json<EnvActivateBody>,
) -> Response {
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
    Json(body): Json<TailStartBody>,
) -> Response {
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
struct PingBody {
    host: String,
    #[serde(default)]
    count: Option<u32>,
}

async fn net_ping(Json(body): Json<PingBody>) -> Json<serde_json::Value> {
    let result = crate::services::nettools::ping(body.host.trim(), body.count).await;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TracerouteBody {
    host: String,
    #[serde(default)]
    resolve_hostnames: bool,
}

/// Traceroute come task in streaming (stessa infrastruttura di node/dotnet
/// run): l'output grezzo arriva riga per riga sul topic WS `task:{id}`.
async fn net_traceroute(State(state): State<ServerState>, Json(body): Json<TracerouteBody>) -> Response {
    let host = body.host.trim();
    if !crate::services::nettools::valid_host(host) {
        return internal_error("host non valido".into());
    }
    let (program, args) = crate::services::nettools::traceroute_command(host, body.resolve_hostnames);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match state.tasks.spawn(&format!("traceroute {host}"), program, &arg_refs, ".") {
        Ok(info) => Json(json!({ "ok": true, "data": info })).into_response(),
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
    Json(body): Json<PushBody>,
) -> Response {
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
) -> Response {
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

const MAX_PROXY_BYTES: usize = crate::services::drop::MAX_PROXY_BYTES;

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
                    return match state.drop.proxy_send_file(&hub, &from_name, &file_name, bytes.to_vec()).await {
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
        return match state.drop.proxy_send_text(&hub, &body.from_name, &body.text).await {
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
) -> Response {
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
    Path(name): Path<String>,
) -> Response {
    match state.drop.delete_received(&name) {
        Ok(()) => Json(json!({ "ok": true, "data": { "deleted": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn drop_open_folder(
    State(state): State<ServerState>,
) -> Response {
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
    Path(name): Path<String>,
) -> Response {
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
    Path(name): Path<String>,
) -> Response {
    let path = match state.drop.received_path(&name) {
        Ok(p) => p,
        Err(message) => return internal_error(message),
    };
    match tauri_plugin_opener::reveal_item_in_dir(path) {
        Ok(()) => Json(json!({ "ok": true, "data": { "revealed": true } })).into_response(),
        Err(e) => internal_error(e.to_string()),
    }
}

// ---------- RickyAI ----------

/// Stato del supervisore + quote e modelli letti da `of-free` quando è pronto.
async fn ai_status(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": state.ai.detailed_snapshot().await }))
}

fn ai_error_response(error: crate::services::rickyai::AiError) -> Response {
    let status =
        StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({
            "ok": false,
            "error": {
                "code": error.code,
                "message": error.message,
                // Secondi da attendere: su quota esaurita la UI propone il
                // riprova invece di far ripetere il messaggio a vuoto.
                "retryAfter": error.retry_after,
                "retryable": error.status == 429 || error.status >= 500,
            }
        })),
    )
        .into_response()
}

async fn ai_chat(
    State(state): State<ServerState>,
    Json(body): Json<crate::services::rickyai::ChatRequest>,
) -> Response {
    match state.ai.chat(body).await {
        Ok(reply) => Json(json!({ "ok": true, "data": reply })).into_response(),
        Err(error) => ai_error_response(error),
    }
}

/// Riavvia `of-free` (o ne ritenta l'avvio dopo un fallimento).
async fn ai_restart(State(state): State<ServerState>) -> Json<serde_json::Value> {
    state.ai.request_restart();
    Json(json!({ "ok": true, "data": { "restarting": true } }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiConfigBody {
    /// Campo assente = invariato. Stringa vuota = torna al default.
    #[serde(default)]
    enabled: Option<bool>,
    /// `local` (of-free avviato dal tool) | `remote` (servizio in rete).
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    remote_url: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    command: Option<String>,
    /// Chiavi dei provider: nome della variabile -> valore. Valore vuoto =
    /// rimuovi. Le chiavi assenti dall'oggetto restano come sono, così la UI
    /// può salvarne una sola senza rimandare (e senza conoscere) le altre.
    #[serde(default)]
    keys: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

/// Un percorso configurato che non esiste va rifiutato subito: altrimenti il
/// supervisore lo riprova a ogni giro e l'utente vede solo "non installato".
fn existing_file(path: &str, what: &str) -> Result<Option<String>, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if !std::path::Path::new(trimmed).is_file() {
        return Err(format!("{what} non trovato: {trimmed}"));
    }
    Ok(Some(trimmed.to_string()))
}

async fn ai_config_set(
    State(state): State<ServerState>,
    Json(body): Json<AiConfigBody>,
) -> Response {
    use crate::services::rickyai::{valid_remote_url, MODES, PROVIDER_KEYS, STRATEGIES};

    let command = match body.command.as_deref().map(|p| existing_file(p, "binario of-free")) {
        Some(Err(message)) => return internal_error(message),
        Some(Ok(value)) => Some(value),
        None => None,
    };
    if let Some(mode) = &body.mode {
        if !MODES.contains(&mode.trim()) {
            return internal_error("modalità non valida: usa local o remote".into());
        }
    }
    // L'indirizzo si normalizza qui (schema e porta impliciti) e si salva già
    // pronto: il supervisore non deve indovinare, e l'errore lo vede chi lo sta
    // scrivendo invece di comparire dopo, come "servizio irraggiungibile".
    let remote_url = match body.remote_url.as_deref().map(str::trim) {
        Some("") => Some(None),
        Some(raw) => match valid_remote_url(raw) {
            Ok(url) => Some(Some(url)),
            Err(message) => return internal_error(message),
        },
        None => None,
    };
    // Modalità remota senza indirizzo: si rifiuta invece di accendere una
    // sezione che non potrebbe funzionare.
    let final_mode = body.mode.as_deref().map(str::trim).unwrap_or(&state.config.get().ai_mode).to_string();
    if final_mode == "remote" {
        let configured = match &remote_url {
            Some(value) => value.clone(),
            None => state.config.get().ai_remote_url,
        };
        if configured.is_none() {
            return internal_error(
                "indica l'indirizzo del servizio of-free (es. 192.168.1.50:4141)".into(),
            );
        }
    }
    if let Some(keys) = &body.keys {
        for name in keys.keys() {
            if !PROVIDER_KEYS.iter().any(|(_, _, var)| var == name) {
                return internal_error(format!("chiave sconosciuta: {name}"));
            }
        }
    }
    if let Some(strategy) = &body.strategy {
        if !STRATEGIES.contains(&strategy.trim()) {
            return internal_error(format!(
                "strategia non valida: usa {}",
                STRATEGIES.join(", ")
            ));
        }
    }
    if let Some(port) = body.port {
        // Sotto la 1024 servono privilegi di root: of-free non partirebbe.
        if port < 1024 {
            return internal_error("porta non valida: usane una da 1024 in su".into());
        }
    }

    state.config.update(|c| {
        if let Some(enabled) = body.enabled {
            c.ai_enabled = enabled;
        }
        if let Some(mode) = &body.mode {
            c.ai_mode = mode.trim().to_string();
        }
        if let Some(url) = &remote_url {
            c.ai_remote_url = url.clone();
        }
        if let Some(port) = body.port {
            c.ai_port = port;
        }
        if let Some(command) = &command {
            c.ai_command = command.clone();
        }
        if let Some(keys) = &body.keys {
            for (name, value) in keys {
                let value = value.trim();
                if value.is_empty() {
                    c.ai_keys.remove(name);
                } else {
                    c.ai_keys.insert(name.clone(), value.to_string());
                }
            }
        }
        if let Some(strategy) = &body.strategy {
            c.ai_strategy = strategy.trim().to_string();
        }
        if let Some(prompt) = &body.system_prompt {
            c.ai_system_prompt = prompt.trim().to_string();
        }
    });
    // Il supervisore rilegge la config al giro successivo: senza questa sveglia
    // resterebbe fermo sull'attesa in corso (fino a cinque minuti).
    state.ai.request_restart();
    Json(json!({ "ok": true, "data": state.ai.snapshot() })).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// POST/DELETE che stanno di proposito nel gruppo di sola lettura, con la
    /// ragione per cui non richiedono il permesso di scrittura. Chi ne aggiunge
    /// una deve motivarla qui: è il punto in cui ci si ferma a pensare.
    const MUTANTI_SENZA_PERMESSO: &[(&str, &str)] = &[
        ("/api/pair", "stabilisce il pairing: prima di lui non esiste un device abbinato"),
        ("/api/log", "log del frontend, nessun effetto sul sistema"),
        ("/api/drop/hello", "un device abbinato deve potersi annunciare, o Drop dal telefono non funziona"),
        ("/api/drop/send", "hub-to-hub, autenticata in auth_middleware dalla discovery UDP"),
        ("/api/drop/text", "hub-to-hub, come sopra"),
    ];

    /// Ogni rotta che modifica qualcosa deve stare in un gruppo con un layer di
    /// permesso, o essere un'eccezione dichiarata sopra. È il seguito di
    /// `write_permitted_invariante_di_sicurezza`: quello prova che la regola è
    /// giusta, questo che è **applicata ovunque** — che è ciò che mancava
    /// quando `set_remote_control` è rimasto scoperto.
    #[test]
    fn ogni_rotta_mutante_ha_un_permesso() {
        let sorgente = std::fs::read_to_string("src/server/mod.rs").expect("sorgente del server");
        let inizio = sorgente.find("let read = Router::new()").expect("gruppo read");
        let fine = sorgente[inizio..].find("let api = ").expect("fine del gruppo read") + inizio;

        let mut scoperte = Vec::new();
        for blocco in sorgente[inizio..fine].split(".route(").skip(1) {
            let Some(path) = blocco.split('"').nth(1) else { continue };
            if !(blocco.contains("post(") || blocco.contains("delete(")) {
                continue;
            }
            let protetta =
                blocco.contains(".layer(write_layer") || blocco.contains(".layer(local_layer");
            let dichiarata = MUTANTI_SENZA_PERMESSO.iter().any(|(p, _)| *p == path);
            if !protetta && !dichiarata {
                scoperte.push(path.to_string());
            }
        }
        assert!(
            scoperte.is_empty(),
            "queste rotte modificano qualcosa dal gruppo di sola lettura, senza layer di \
             permesso né una motivazione in MUTANTI_SENZA_PERMESSO: {scoperte:#?}"
        );
    }

    /// Nel gruppo read alcune rotte hanno GET e POST sullo stesso path, col
    /// layer applicato al singolo metodo. Questo test prova il meccanismo axum
    /// su cui quella scelta si regge: se un aggiornamento lo cambiasse, la POST
    /// resterebbe aperta in silenzio.
    #[tokio::test]
    async fn il_layer_per_metodo_protegge_solo_la_post() {
        use tower::ServiceExt;

        let app: Router = Router::new().route(
            "/x",
            get(|| async { "letto" })
                .merge(post(|| async { "scritto" }).layer(middleware::from_fn(require_loopback))),
        );
        let lan: SocketAddr = "192.168.1.50:1234".parse().unwrap();
        let chiama = |metodo: &str| {
            let app = app.clone();
            let mut req = Request::builder()
                .method(metodo)
                .uri("/x")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(ConnectInfo(lan));
            async move { app.oneshot(req).await.unwrap().status() }
        };

        assert_eq!(chiama("GET").await, StatusCode::OK, "la lettura resta aperta");
        assert_eq!(
            chiama("POST").await,
            StatusCode::FORBIDDEN,
            "la scrittura sullo stesso path è bloccata da remoto"
        );
    }

    /// Il testo di una chat esce dalla macchina e consuma quote condivise, e la
    /// configurazione decide *quale binario* il tool avvia da solo: nessuna
    /// delle due può stare fra le letture aperte ai device abbinati. Spostare
    /// una riga di rotta è un attimo, e da fuori non si vedrebbe.
    #[test]
    fn le_rotte_di_rickyai_stanno_nel_gruppo_giusto() {
        let sorgente = std::fs::read_to_string("src/server/mod.rs").expect("sorgente del server");
        let gruppo = |inizio: &str, fine: &str| {
            let start = sorgente.find(inizio).unwrap_or_else(|| panic!("gruppo {inizio}"));
            let end = sorgente[start..].find(fine).expect("fine del gruppo") + start;
            sorgente[start..end].to_string()
        };

        let write = gruppo("let write = Router::new()", "let read = ");
        assert!(write.contains("\"/api/ai/chat\""), "la chat deve stare nel gruppo di scrittura");
        assert!(write.contains("\"/api/ai/restart\""), "il riavvio deve stare nel gruppo di scrittura");

        // La configurazione dice *quale binario* il tool avvia da solo: come
        // l'host Docker, si tocca solo dal desktop, e il controllo remoto non
        // la sblocca.
        let local = gruppo("let local_only = Router::new()", "let write = ");
        assert!(local.contains("\"/api/ai/config\""), "la config deve essere solo dal desktop");

        let read = gruppo("let read = Router::new()", "let api = ");
        assert!(read.contains("\"/api/ai/status\""), "lo stato deve restare leggibile");
    }

    #[test]
    fn write_permitted_invariante_di_sicurezza() {
        // Loopback: sempre concesso, anche col controllo remoto spento.
        assert!(write_permitted(true, false));
        assert!(write_permitted(true, true));
        // Remoto: negato salvo controllo remoto attivo.
        assert!(!write_permitted(false, false));
        assert!(write_permitted(false, true));
    }

    #[test]
    fn expand_tilde_risolve_la_home() {
        let home = home_dir_string();
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(
            expand_tilde("~/Documents/Progetti/Share WebUI"),
            format!("{}/Documents/Progetti/Share WebUI", home.trim_end_matches('/'))
        );
        // Nessun tilde iniziale: invariato (anche i path assoluti restano tali).
        assert_eq!(expand_tilde("/var/log/app.log"), "/var/log/app.log");
        assert_eq!(expand_tilde("relativo/dir"), "relativo/dir");
    }
}
