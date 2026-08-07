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

use crate::constants::{
    MAX_LOG_FIELD, MAX_PAIR_SESSIONS, MIN_HUB_CODE_LEN, PAIR_COOKIE, PORT_FALLBACK_RANGE,
    SENSORS_ALERT_INTERVAL, TOPIC_SENSORS_BACKGROUND,
};

#[derive(Clone)]
pub struct ServerState {
    pub config: ConfigHandle,
    pub bus: EventBus,
    pub pollers: Arc<PollerRegistry>,
    pub port: u16,
    pub tools_cache: Arc<tokio::sync::Mutex<Option<Vec<crate::adapters::tools::DiscoveredTool>>>>,
    pub tasks: Arc<crate::tasks::TaskRegistry>,
    pub alerts: Arc<crate::alerts::AlertService>,
    pub tails: Arc<crate::services::logtail::TailRegistry>,
    pub drop: Arc<crate::services::drop::DropService>,
    pub metrics: Arc<crate::services::metrics::MetricsService>,
    pub clipboard: Arc<crate::services::clipboard::ClipboardHistory>,
    pub ai: Arc<crate::services::rickyai::AiService>,
    pub sessions: Arc<SessionActivity>,
}

fn write_allowed(state: &ServerState, peer: SocketAddr) -> bool {
    write_permitted(peer.ip().is_loopback(), state.config.get().remote_control_enabled)
}
fn write_permitted(is_loopback: bool, remote_control_enabled: bool) -> bool {
    is_loopback || remote_control_enabled
}

// 20260806 ++ RG #Security loopback non basta a dire "locale": col DNS rebinding un sito ostile
// risolve il proprio dominio a 127.0.0.1. L'Host è l'unica cosa che il browser non falsifica.
fn host_name(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('[') {
        return rest.split(']').next().unwrap_or_default().to_ascii_lowercase();
    }
    if raw.matches(':').count() > 1 {
        return raw.to_ascii_lowercase();
    }
    raw.rsplit_once(':').map_or(raw, |(h, _)| h).to_ascii_lowercase()
}

fn host_allowed(raw_host: &str, lan_enabled: bool, lan_ips: &[String]) -> bool {
    let host = host_name(raw_host);
    if host.is_empty() {
        return false;
    }
    if host == "localhost" {
        return true;
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return false;
    };
    if ip.is_loopback() {
        return true;
    }
    lan_enabled && lan_ips.iter().any(|l| l.parse::<IpAddr>().is_ok_and(|l| l == ip))
}

fn request_host(request: &Request<Body>) -> String {
    request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.to_string())
        // 20260806 ++ RG #Security in HTTP/2 l'autorità sta nella URI, non nell'header.
        .or_else(|| request.uri().host().map(|h| h.to_string()))
        .unwrap_or_default()
}

async fn require_known_host(
    State(state): State<ServerState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let raw = request_host(&request);

    let allowed = host_allowed(&raw, false, &[])
        || (state.config.get().lan_enabled && host_allowed(&raw, true, &netinfo::lan_ips()));

    if allowed {
        return next.run(request).await;
    }
    tracing::warn!(host = %raw, "richiesta rifiutata: host non consentito");
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "ok": false,
            "error": { "code": "HOST_FORBIDDEN", "message": "Host non consentito", "retryable": false }
        })),
    )
        .into_response()
}

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
    pub state: ServerState,
}

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
    let hub_registry = crate::services::hubdiscovery::start(&config, port);
    let drop_service = Arc::new(crate::services::drop::DropService::new(
        bus.clone(),
        config.clone(),
        hub_registry,
    ));
    crate::jiggler::start(config.clone());
    let metrics = crate::services::metrics::MetricsService::start();
    let clipboard = crate::services::clipboard::ClipboardHistory::start();
    let ai = crate::services::rickyai::AiService::start(config.clone(), bus.clone());
    {
        let bus_bg = bus.clone();
        tokio::spawn(async move {
            loop {
                let payload = crate::adapters::sensors::read_for_alerts().await;
                bus_bg.publish(TOPIC_SENSORS_BACKGROUND, payload);
                tokio::time::sleep(SENSORS_ALERT_INTERVAL).await;
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
        sessions: Arc::new(SessionActivity::default()),
    };

    let local_layer = middleware::from_fn(require_loopback);
    let write_layer = middleware::from_fn_with_state(state.clone(), require_write);

    // 20260704 RG il permesso viene dal gruppo in cui la rotta è scritta, non dall'handler:
    // aggiungere un endpoint obbliga a sceglierne uno. (1) solo desktop: distruttive
    // e interruttori che concedono permessi — il controllo remoto non li sblocca.
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
        .route("/api/config/close-to-tray", post(set_close_to_tray))
        .route("/api/config/hub-code", get(hub_code_get).merge(post(hub_code_set)))
        .route("/api/pair/sessions", get(pair_sessions_list))
        .route("/api/pair/sessions/{id}", axum::routing::delete(pair_session_revoke))
        .route("/api/pair/rotate", post(pair_token_rotate))
        .route("/api/ai/config", post(ai_config_set))
        .route("/api/system/open-accessibility", post(open_accessibility))
        .route("/api/system/open-local-network", post(open_local_network))
        .route("/api/system/color-meter", post(open_color_meter))
        .route("/api/system/open-url", post(open_url))
        .layer(local_layer.clone());

    // 20260704 RG (2) modificano il sistema: locale, o LAN col controllo remoto attivo.
    // Include delle GET: leggere un .env espone segreti quanto scriverci.
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
        .route("/api/ai/chat", post(ai_chat))
        .route("/api/ai/restart", post(ai_restart))
        .layer(write_layer.clone());

    // 20260704 RG (3) sola lettura, per ogni device abbinato. Dove GET e POST condividono
    // il path il layer sta sul singolo metodo.
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
        .route("/api/ai/status", get(ai_status))
        .route("/api/system/accessibility", get(accessibility_status))
        .route("/api/system/local-network", get(local_network_status))
        .route("/ws", get(ws::ws_handler));

    let api = local_only
        .merge(write)
        .merge(read)
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let mut app = Router::new()
        .merge(api)
        .fallback(static_assets)
        .with_state(state.clone());

    if cfg!(debug_assertions) {
        app = app.layer(tower_http::cors::CorsLayer::very_permissive());
    }

    // 20260806 ++ RG #Security deve restare il layer più esterno: prima di CORS, auth e asset.
    let app = app.layer(middleware::from_fn_with_state(state.clone(), require_known_host));

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

async fn auth_middleware(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if peer.ip().is_loopback() || request.uri().path() == "/api/pair" {
        return next.run(request).await;
    }
    // 20260806 ++ RG #Security la scorciatoia vale quanto il registro degli hub: i beacon sono firmati
    // col codice condiviso, e senza codice il registro resta vuoto.
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
    // 20260806 ++ RG #Security il cookie porta un id di sessione, non il pair_token: quello autorizza
    // solo /api/pair. Confronto a tempo costante.
    if let Some(cookie) = cookie_value(request.headers(), PAIR_COOKIE) {
        if session_valid(&state.config.get().pair_sessions, &cookie) {
            state.sessions.touch(&cookie);
            return next.run(request).await;
        }
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

fn session_valid(sessions: &[crate::config::PairSession], cookie: &str) -> bool {
    sessions.iter().any(|s| crate::config::secret_eq(&s.id, cookie))
}

// 20260806 ++ RG #Security "ultimo accesso" solo in RAM: in config riscriverebbe config.json a ogni poll.
#[derive(Default)]
pub struct SessionActivity {
    seen: std::sync::Mutex<std::collections::HashMap<String, u64>>,
}

impl SessionActivity {
    pub fn touch(&self, session_id: &str) {
        self.seen
            .lock()
            .expect("sessions lock")
            .insert(session_id.to_string(), crate::events::now_ms());
    }

    pub fn last_seen(&self, session_id: &str) -> Option<u64> {
        self.seen.lock().expect("sessions lock").get(session_id).copied()
    }

    pub fn forget(&self, session_id: &str) {
        self.seen.lock().expect("sessions lock").remove(session_id);
    }
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
        "data": { "urls": urls, "port": state.port, "lanEnabled": cfg.lan_enabled, "remoteControlEnabled": cfg.remote_control_enabled, "antiIdleEnabled": cfg.anti_idle_enabled, "closeToTray": cfg.close_to_tray, "remote": !peer.ip().is_loopback() }
    }))
}

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
#[serde(rename_all = "camelCase")]
struct PairBody {
    token: String,
    #[serde(default)]
    device_name: Option<String>,
}

async fn pair(State(state): State<ServerState>, Json(body): Json<PairBody>) -> Response {
    let cfg = state.config.get();
    if !crate::config::secret_eq(&cfg.pair_token, &body.token) {
        tracing::warn!("tentativo di pairing con token non valido");
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "error": { "code": "ACCESS_DENIED", "message": "Token di pairing non valido", "retryable": false }
            })),
        )
            .into_response();
    }

    // 20260806 ++ RG #Security il token vale solo per questo scambio: il device si porta a casa un id
    // di sessione suo, revocabile senza toccare gli altri.
    let session = crate::config::PairSession {
        id: crate::config::generate_token(),
        name: clean_device_name(body.device_name.as_deref()),
        created_at: crate::events::now_ms(),
    };
    let cookie = format!(
        "{PAIR_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly",
        session.id
    );
    state.config.update(|c| {
        c.pair_sessions.push(session.clone());
        // 20260806 ++ RG #Security le sessioni non scadono: senza tetto un device che si riabbina spesso
        // le farebbe crescere per sempre.
        if c.pair_sessions.len() > MAX_PAIR_SESSIONS {
            let excess = c.pair_sessions.len() - MAX_PAIR_SESSIONS;
            c.pair_sessions.drain(..excess);
        }
    });
    tracing::info!(device = %session.name, "nuovo dispositivo abbinato");
    (
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "ok": true, "data": { "paired": true } })),
    )
        .into_response()
}

fn clean_device_name(raw: Option<&str>) -> String {
    let cleaned: String = raw
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Dispositivo".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn pair_sessions_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    let sessions: Vec<serde_json::Value> = state
        .config
        .get()
        .pair_sessions
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "createdAt": s.created_at,
                "lastSeen": state.sessions.last_seen(&s.id),
            })
        })
        .collect();
    Json(json!({ "ok": true, "data": { "sessions": sessions } }))
}

async fn pair_session_revoke(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
    let before = state.config.get().pair_sessions.len();
    state.config.update(|c| c.pair_sessions.retain(|s| s.id != id));
    state.sessions.forget(&id);
    let removed = before != state.config.get().pair_sessions.len();
    tracing::info!(removed, "revoca sessione di pairing");
    Json(json!({ "ok": true, "data": { "revoked": removed } })).into_response()
}

// 20260806 ++ RG #Security ruotare il token invalida i QR vecchi ma non scollega nessuno.
async fn pair_token_rotate(State(state): State<ServerState>) -> Response {
    let token = crate::config::generate_token();
    state.config.update(|c| c.pair_token = token.clone());
    tracing::info!("token di pairing rigenerato");
    Json(json!({ "ok": true, "data": { "rotated": true } })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogBody {
    level: Option<String>,
    message: String,
    stack: Option<String>,
}

// 20260806 ++ RG #Security lo scrive un client e finisce nei nostri log: senza tetto satura il
// disco, e i caratteri di controllo ci scrivono righe finte.
fn clean_log(s: &str) -> String {
    let mut out: String = s
        .chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .take(MAX_LOG_FIELD)
        .collect();
    if s.chars().filter(|c| !c.is_control() || *c == ' ').count() > MAX_LOG_FIELD {
        out.push('…');
    }
    out
}

async fn client_log(Json(body): Json<LogBody>) -> Json<serde_json::Value> {
    let message = clean_log(&body.message);
    let stack = clean_log(body.stack.as_deref().unwrap_or_default());
    match body.level.as_deref() {
        Some("error") => tracing::error!(target: "frontend", %message, %stack),
        Some("warn") => tracing::warn!(target: "frontend", %message, %stack),
        _ => tracing::info!(target: "frontend", %message),
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
    path: Option<String>,
}

async fn set_tool_path(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(body): Json<ToolPathBody>,
) -> Response {
    if !crate::constants::TOOL_IDS.contains(&id.as_str()) {
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
    action: String,
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
    action: String,
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

#[derive(Deserialize)]
struct LaunchIdBody {
    id: String,
}

async fn launch_bundles_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "bundles": state.config.get().launch_bundles } }))
}

#[derive(Deserialize)]
struct LaunchBundleBody {
    #[serde(default)]
    id: Option<String>,
    name: String,
    steps: Vec<crate::services::launch::LaunchStep>,
}

async fn launch_bundle_upsert(
    State(state): State<ServerState>,
    Json(body): Json<LaunchBundleBody>,
) -> Response {
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
        } else if c.snippets.len() >= crate::constants::MAX_SNIPPETS {
            too_many = true;
        } else {
            c.snippets.push(snippet.clone());
        }
    });
    if too_many {
        return internal_error(format!(
            "troppi snippet (max {})",
            crate::constants::MAX_SNIPPETS
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

fn home_dir_string() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string())
}

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
        } else if c.ssh_hosts.len() >= crate::constants::MAX_SSH_HOSTS {
            too_many = true;
        } else {
            c.ssh_hosts.push(host.clone());
        }
    });
    if too_many {
        return internal_error(format!("troppi host (max {})", crate::constants::MAX_SSH_HOSTS));
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
    match state.clipboard.copy_to_clipboard(body.id) {
        Ok(()) => Json(json!({ "ok": true, "data": { "copied": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

#[derive(Deserialize)]
struct ClipBlobQuery {
    id: u64,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipSendBody {
    to: String,
    from_name: String,
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

async fn docker_state(State(state): State<ServerState>) -> Json<serde_json::Value> {
    let host = state.config.get().docker_host;
    let docker = crate::adapters::docker::state(host.as_deref()).await;
    let mut data = serde_json::to_value(&docker).unwrap_or_else(|_| json!({}));
    data["host"] = json!(host);
    Json(json!({ "ok": true, "data": data }))
}

async fn docker_images(State(state): State<ServerState>) -> Json<serde_json::Value> {
    let host = state.config.get().docker_host;
    let images = crate::adapters::docker::images(host.as_deref()).await;
    Json(json!({ "ok": true, "data": { "images": images } }))
}

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

async fn docker_host_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "host": state.config.get().docker_host } }))
}

#[derive(Deserialize)]
struct DockerHostBody {
    host: Option<String>,
}

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
    action: String,
}

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

async fn scheduler_list() -> Json<serde_json::Value> {
    let listing = crate::adapters::scheduler::list().await;
    Json(json!({ "ok": true, "data": listing }))
}

#[derive(Deserialize)]
struct SchedDetailQuery {
    source: String,
    id: String,
}

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

async fn set_remote_control(
    State(state): State<ServerState>,
    Json(body): Json<RemoteControlBody>,
) -> Response {
    state.config.update(|c| c.remote_control_enabled = body.enabled);
    tracing::info!(enabled = body.enabled, "remote control aggiornato");
    Json(json!({ "ok": true, "data": { "remoteControlEnabled": body.enabled } })).into_response()
}

async fn set_anti_idle(
    State(state): State<ServerState>,
    Json(body): Json<RemoteControlBody>,
) -> Response {
    state.config.update(|c| c.anti_idle_enabled = body.enabled);
    tracing::info!(enabled = body.enabled, "anti-idle aggiornato");
    Json(json!({ "ok": true, "data": { "antiIdleEnabled": body.enabled } })).into_response()
}

// 20260807 ++ RG #CloseToTray sta fra le locali: decide se la X spegne il tool
async fn set_close_to_tray(
    State(state): State<ServerState>,
    Json(body): Json<RemoteControlBody>,
) -> Response {
    state.config.update(|c| c.close_to_tray = body.enabled);
    tracing::info!(enabled = body.enabled, "chiusura nel tray aggiornata");
    Json(json!({ "ok": true, "data": { "closeToTray": body.enabled } })).into_response()
}

async fn accessibility_status() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": crate::adapters::accessibility::status() }))
}

async fn open_accessibility() -> Response {
    match crate::adapters::accessibility::open_settings() {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

async fn local_network_status() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": crate::adapters::localnetwork::status() }))
}

async fn open_local_network() -> Response {
    match crate::adapters::localnetwork::open_settings() {
        Ok(()) => Json(json!({ "ok": true, "data": { "opened": true } })).into_response(),
        Err(message) => internal_error(message),
    }
}

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

async fn tasks_list(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "tasks": state.tasks.list() } }))
}

async fn tasks_clear_finished(State(state): State<ServerState>) -> Json<serde_json::Value> {
    state.tasks.clear_finished();
    Json(json!({ "ok": true, "data": { "tasks": state.tasks.list() } }))
}

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
    if topic == crate::constants::TOPIC_STATS {
        state
            .config
            .update(|c| c.stats_interval_ms = body.interval_ms.clamp(200, 60_000));
    }
    Json(json!({ "ok": true, "data": { "intervalMs": body.interval_ms } })).into_response()
}

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
    #[serde(default)]
    excludes: Vec<String>,
}

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
    action: String,
    side: Option<crate::services::fscompare::Side>,
}

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropHelloBody {
    device_id: String,
    name: String,
    #[serde(default)]
    device_secret: String,
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
    match state.drop.hello(
        &body.device_id,
        &body.device_secret,
        &body.name,
        peer.ip().is_loopback(),
    ) {
        Ok(peers) => Json(json!({ "ok": true, "data": { "peers": peers } })).into_response(),
        Err(message) => internal_error(message),
    }
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

// 20260806 ++ RG #Security isDesktop dice al client se può sottoscrivere il canale dell'hub: è del
// desktop, il telefono non deve vedere i drop arrivati dagli altri PC.
async fn drop_self(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "data": { "hubId": state.drop.hub_id(), "isDesktop": peer.ip().is_loopback() }
    }))
}

async fn drop_hubs(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "hubs": state.drop.remote_hubs() } }))
}

// 20260806 ++ RG #Security il codice hub è un segreto: anche la GET sta nel gruppo solo-desktop.

#[derive(Deserialize)]
struct HubCodeBody {
    // 20260806 ++ RG #Security assente = generane uno nuovo, stringa vuota = spegni l'invio tra PC.
    code: Option<String>,
}

async fn hub_code_get(State(state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "data": { "code": state.config.get().drop_hub_code } }))
}

async fn hub_code_set(
    State(state): State<ServerState>,
    Json(body): Json<HubCodeBody>,
) -> Response {
    use crate::services::hubdiscovery::normalize_hub_code;

    let code = match body.code {
        None => crate::config::generate_hub_code(),
        Some(raw) => {
            let normalized = normalize_hub_code(&raw);
            if normalized.is_empty() {
                String::new()
            } else if normalized.len() < MIN_HUB_CODE_LEN {
                return internal_error(format!(
                    "codice troppo corto: servono almeno {MIN_HUB_CODE_LEN} caratteri"
                ));
            } else {
                raw.trim().to_string()
            }
        }
    };
    state.config.update(|c| c.drop_hub_code = code.clone());
    state.drop.forget_hubs();
    tracing::info!(attivo = !code.is_empty(), "codice hub aggiornato");
    Json(json!({ "ok": true, "data": { "code": code } })).into_response()
}

const MAX_PROXY_BYTES: usize = crate::constants::MAX_PROXY_BYTES;

fn proxy_too_big() -> String {
    format!(
        "file troppo grande per l'invio a un altro computer (max {}MB)",
        MAX_PROXY_BYTES / 1024 / 1024
    )
}

// 20260806 ++ RG #Security rifiuta il chunk *prima* di appenderlo: la memoria non supera mai il tetto.
struct CappedBuffer {
    buf: Vec<u8>,
    max: usize,
}

impl CappedBuffer {
    fn new(max: usize) -> Self {
        Self { buf: Vec::new(), max }
    }

    fn push(&mut self, chunk: &[u8]) -> Result<(), String> {
        if self.buf.len() + chunk.len() > self.max {
            return Err(proxy_too_big());
        }
        self.buf.extend_from_slice(chunk);
        Ok(())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.buf.len()
    }

    fn into_inner(self) -> Vec<u8> {
        self.buf
    }
}

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

                if let Some(hub) = state.drop.remote_hub(&to) {
                    // 20260806 ++ RG #Security il cap si applica mentre si legge: prima si allocava l'intero campo
                    // (il limite di rotta è 4 GB) e solo dopo lo si confrontava col tetto.
                    let mut buf = CappedBuffer::new(MAX_PROXY_BYTES);
                    let mut field = field;
                    loop {
                        match field.chunk().await {
                            Ok(Some(chunk)) => {
                                if let Err(message) = buf.push(&chunk) {
                                    return internal_error(message);
                                }
                            }
                            Ok(None) => break,
                            Err(e) => return internal_error(format!("upload interrotto: {e}")),
                        }
                    }
                    return match state.drop.proxy_send_file(&hub, &from_name, &file_name, buf.into_inner()).await {
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

use crate::constants::DEVICE_SECRET_HEADER;

async fn drop_download(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let secret = headers
        .get(DEVICE_SECRET_HEADER)
        .and_then(|v| v.to_str().ok());
    let (path, name) = match state.drop.transfer_file(&id, secret, peer.ip().is_loopback()) {
        Ok(found) => found,
        Err(crate::services::drop::TransferError::Forbidden) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "ok": false,
                    "error": { "code": "NOT_RECIPIENT", "message": "questo trasferimento non è per questo dispositivo", "retryable": false }
                })),
            )
                .into_response()
        }
        Err(crate::services::drop::TransferError::NotFound) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "ok": false,
                    "error": { "code": "PATH_NOT_FOUND", "message": "trasferimento scaduto o inesistente", "retryable": false }
                })),
            )
                .into_response()
        }
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => return internal_error(format!("file non leggibile: {e}")),
    };
    let stream = tokio_util::io::ReaderStream::new(file);
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

async fn ai_restart(State(state): State<ServerState>) -> Json<serde_json::Value> {
    state.ai.request_restart();
    Json(json!({ "ok": true, "data": { "restarting": true } }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiConfigBody {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    remote_url: Option<String>,
    #[serde(default)]
    remote_key: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    keys: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

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
    use crate::constants::{AI_MODES, AI_PROVIDER_KEYS, AI_STRATEGIES};
    use crate::services::rickyai::valid_remote_url;

    let command = match body.command.as_deref().map(|p| existing_file(p, "binario of-free")) {
        Some(Err(message)) => return internal_error(message),
        Some(Ok(value)) => Some(value),
        None => None,
    };
    if let Some(mode) = &body.mode {
        if !AI_MODES.contains(&mode.trim()) {
            return internal_error("modalità non valida: usa local o remote".into());
        }
    }
    let remote_url = match body.remote_url.as_deref().map(str::trim) {
        Some("") => Some(None),
        Some(raw) => match valid_remote_url(raw) {
            Ok(url) => Some(Some(url)),
            Err(message) => return internal_error(message),
        },
        None => None,
    };
    // 20260804 RG passare a "remote" senza indirizzo non è un errore: il campo compare dopo.
    if let Some(keys) = &body.keys {
        for name in keys.keys() {
            if !AI_PROVIDER_KEYS.iter().any(|(_, _, var)| var == name) {
                return internal_error(format!("chiave sconosciuta: {name}"));
            }
        }
    }
    if let Some(strategy) = &body.strategy {
        if !AI_STRATEGIES.contains(&strategy.trim()) {
            return internal_error(format!(
                "strategia non valida: usa {}",
                AI_STRATEGIES.join(", ")
            ));
        }
    }
    if let Some(port) = body.port {
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
        if let Some(key) = &body.remote_key {
            let key = key.trim();
            c.ai_remote_key = (!key.is_empty()).then(|| key.to_string());
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
    state.ai.request_restart();
    Json(json!({ "ok": true, "data": state.ai.snapshot() })).into_response()
}

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

    const MUTANTI_SENZA_PERMESSO: &[(&str, &str)] = &[
        ("/api/pair", "stabilisce il pairing: prima di lui non esiste un device abbinato"),
        ("/api/log", "log del frontend, nessun effetto sul sistema"),
        ("/api/drop/hello", "un device abbinato deve potersi annunciare, o Drop dal telefono non funziona"),
        ("/api/drop/send", "hub-to-hub, autenticata in auth_middleware dalla discovery UDP"),
        ("/api/drop/text", "hub-to-hub, come sopra"),
    ];

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

        let local = gruppo("let local_only = Router::new()", "let write = ");
        assert!(local.contains("\"/api/ai/config\""), "la config deve essere solo dal desktop");

        let read = gruppo("let read = Router::new()", "let api = ");
        assert!(read.contains("\"/api/ai/status\""), "lo stato deve restare leggibile");
    }

    #[test]
    fn il_codice_hub_si_legge_solo_dal_desktop() {
        let file = std::fs::read_to_string("src/server/mod.rs").expect("sorgente del server");
        let sorgente = &file[..file.find("#[cfg(test)]\nmod tests {").expect("modulo di test")];
        let inizio = sorgente.find("let local_only = Router::new()").expect("gruppo local_only");
        let fine = sorgente[inizio..].find("let write = ").expect("fine del gruppo") + inizio;
        assert!(
            sorgente[inizio..fine].contains("\"/api/config/hub-code\""),
            "il codice hub è un segreto condiviso: anche la GET deve stare nel gruppo \
             solo-desktop, o un device abbinato può leggerlo e spacciarsi per un hub"
        );
    }

    #[test]
    fn host_name_toglie_la_porta() {
        assert_eq!(host_name("127.0.0.1:6969"), "127.0.0.1");
        assert_eq!(host_name("localhost"), "localhost");
        assert_eq!(host_name("LocalHost:6969"), "localhost");
        assert_eq!(host_name("[::1]:6969"), "::1");
        assert_eq!(host_name("::1"), "::1");
        assert_eq!(host_name("192.168.1.50:6969"), "192.168.1.50");
    }

    #[test]
    fn host_allowed_ferma_il_dns_rebinding() {
        let lan = vec!["192.168.1.50".to_string()];

        assert!(host_allowed("127.0.0.1:6969", false, &[]), "il desktop parla con 127.0.0.1");
        assert!(host_allowed("localhost:6969", false, &[]), "e con localhost");
        assert!(host_allowed("[::1]:6969", false, &[]), "loopback IPv6");

        assert!(!host_allowed("evil.com:6969", true, &lan), "il dominio ostile del rebinding");
        assert!(!host_allowed("", true, &lan), "richiesta senza Host");
        assert!(!host_allowed("rickydev.local:6969", true, &lan), "solo IP e localhost");

        assert!(host_allowed("192.168.1.50:6969", true, &lan), "il telefono in LAN");
        assert!(!host_allowed("192.168.1.50:6969", false, &lan), "LAN spenta: solo locale");
        assert!(!host_allowed("192.168.1.99:6969", true, &lan), "IP che non è nostro");
    }

    #[test]
    fn request_host_legge_header_e_uri() {
        let con_header = Request::builder()
            .uri("/api/health")
            .header(header::HOST, "evil.com:6969")
            .body(Body::empty())
            .unwrap();
        assert_eq!(request_host(&con_header), "evil.com:6969");
        assert!(
            !host_allowed(&request_host(&con_header), true, &["192.168.1.50".to_string()]),
            "il DNS rebinding non deve arrivare all'auth_middleware"
        );

        let senza_header = Request::builder()
            .uri("http://127.0.0.1:6969/api/health")
            .body(Body::empty())
            .unwrap();
        assert_eq!(request_host(&senza_header), "127.0.0.1", "ripiego sull'autorità della URI");

        let nudo = Request::builder().uri("/api/health").body(Body::empty()).unwrap();
        assert_eq!(request_host(&nudo), "", "niente Host, niente accesso");
        assert!(!host_allowed(&request_host(&nudo), false, &[]));
    }

    #[tokio::test]
    async fn il_guardiano_dellhost_copre_anche_gli_asset_statici() {
        use tower::ServiceExt;

        let api = Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .layer(middleware::from_fn(|r: Request<Body>, n: Next| async move { n.run(r).await }));
        let app: Router = Router::new()
            .merge(api)
            .fallback(|| async { "index.html" })
            .layer(middleware::from_fn(|r: Request<Body>, n: Next| async move {
                if host_allowed(&request_host(&r), false, &[]) {
                    n.run(r).await
                } else {
                    StatusCode::FORBIDDEN.into_response()
                }
            }));

        let chiama = |path: &'static str, host: &'static str| {
            let app = app.clone();
            async move {
                let req = Request::builder()
                    .uri(path)
                    .header(header::HOST, host)
                    .body(Body::empty())
                    .unwrap();
                app.oneshot(req).await.unwrap().status()
            }
        };

        assert_eq!(chiama("/api/health", "127.0.0.1:6969").await, StatusCode::OK);
        assert_eq!(chiama("/", "127.0.0.1:6969").await, StatusCode::OK);
        assert_eq!(
            chiama("/api/health", "evil.com:6969").await,
            StatusCode::FORBIDDEN,
            "le API non devono rispondere a un host sconosciuto"
        );
        assert_eq!(
            chiama("/", "evil.com:6969").await,
            StatusCode::FORBIDDEN,
            "nemmeno la SPA: servirla darebbe all'attaccante una origin same-site"
        );
    }

    #[test]
    fn il_guardiano_dellhost_e_il_layer_piu_esterno() {
        let file = std::fs::read_to_string("src/server/mod.rs").expect("sorgente del server");
        // 20260806 ++ RG #Security solo il codice vero: dentro mod tests le stesse stringhe sono letterali.
        let sorgente = &file[..file.find("#[cfg(test)]\nmod tests {").expect("modulo di test")];
        let guardia = sorgente
            .find("app.layer(middleware::from_fn_with_state(state.clone(), require_known_host))")
            .expect("il layer require_known_host deve essere applicato in start()");
        let cors = sorgente.find("CorsLayer::very_permissive").expect("layer CORS");
        let auth = sorgente
            .find("layer(middleware::from_fn_with_state(state.clone(), auth_middleware))")
            .expect("layer di auth");
        assert!(
            guardia > cors && guardia > auth,
            "in axum l'ultimo layer applicato è il più esterno: require_known_host deve venire \
             dopo CORS e auth_middleware, o l'anti-rebinding viene scavalcato"
        );
    }

    fn sessione(id: &str) -> crate::config::PairSession {
        crate::config::PairSession {
            id: id.to_string(),
            name: "iPhone".to_string(),
            created_at: 0,
        }
    }

    #[test]
    fn il_cookie_vale_solo_se_e_una_sessione_emessa() {
        let sessioni = vec![sessione("aaaa1111"), sessione("bbbb2222")];

        assert!(session_valid(&sessioni, "aaaa1111"));
        assert!(session_valid(&sessioni, "bbbb2222"));
        assert!(!session_valid(&sessioni, "cccc3333"), "sessione mai emessa");
        assert!(!session_valid(&sessioni, ""), "cookie vuoto");
        assert!(!session_valid(&sessioni, "aaaa"), "prefisso di una sessione");
        assert!(!session_valid(&sessioni, "aaaa1111x"), "sessione più lunga");
        assert!(!session_valid(&[], "aaaa1111"), "nessuna sessione, nessun accesso");
    }

    #[test]
    fn il_token_di_pairing_non_e_piu_un_cookie_valido() {
        let token = crate::config::generate_token();
        let sessioni = vec![sessione(&crate::config::generate_token())];
        assert!(
            !session_valid(&sessioni, &token),
            "il token di pairing non deve autorizzare da solo una richiesta"
        );
    }

    #[test]
    fn revocare_una_sessione_non_tocca_le_altre() {
        let mut sessioni = vec![sessione("telefono1"), sessione("tablet22")];
        sessioni.retain(|s| s.id != "telefono1");

        assert!(!session_valid(&sessioni, "telefono1"), "revocata");
        assert!(session_valid(&sessioni, "tablet22"), "l'altro device resta abbinato");
    }

    #[test]
    fn secret_eq_confronta_tutta_la_stringa() {
        use crate::config::secret_eq;
        assert!(secret_eq("abc123", "abc123"));
        assert!(!secret_eq("abc123", "abc124"));
        assert!(!secret_eq("abc123", "abc12"), "più corta");
        assert!(!secret_eq("abc123", "abc1234"), "più lunga");
        assert!(!secret_eq("", ""), "il vuoto non è un segreto valido");
        assert!(!secret_eq("", "x"));
    }

    #[test]
    fn clean_device_name_non_si_fa_iniettare_nei_log() {
        assert_eq!(clean_device_name(Some("iPhone di Ricky")), "iPhone di Ricky");
        assert_eq!(clean_device_name(Some("  Tablet  ")), "Tablet");
        assert_eq!(clean_device_name(None), "Dispositivo");
        assert_eq!(clean_device_name(Some("   ")), "Dispositivo");
        assert_eq!(clean_device_name(Some("PC\nERROR finto")), "PCERROR finto");
        assert_eq!(clean_device_name(Some(&"z".repeat(200))), "z".repeat(40));
    }

    #[test]
    fn la_gestione_delle_sessioni_e_solo_dal_desktop() {
        let file = std::fs::read_to_string("src/server/mod.rs").expect("sorgente del server");
        let sorgente = &file[..file.find("#[cfg(test)]\nmod tests {").expect("modulo di test")];
        let inizio = sorgente.find("let local_only = Router::new()").expect("gruppo local_only");
        let fine = sorgente[inizio..].find("let write = ").expect("fine del gruppo") + inizio;
        let gruppo = &sorgente[inizio..fine];

        for rotta in ["\"/api/pair/sessions\"", "\"/api/pair/sessions/{id}\"", "\"/api/pair/rotate\""] {
            assert!(
                gruppo.contains(rotta),
                "{rotta} decide chi ha accesso all'app: deve stare nel gruppo solo-desktop, \
                 o un telefono abbinato può revocare gli altri o elencare le sessioni"
            );
        }
    }

    #[test]
    fn capped_buffer_rifiuta_prima_di_allocare() {
        let mut buf = CappedBuffer::new(10);
        assert!(buf.push(&[0u8; 6]).is_ok());
        assert!(buf.push(&[0u8; 4]).is_ok(), "arrivare esatti al tetto è lecito");
        assert_eq!(buf.len(), 10);

        let mut buf = CappedBuffer::new(10);
        buf.push(&[0u8; 8]).expect("sotto il tetto");
        let esito = buf.push(&[0u8; 5]);
        assert!(esito.is_err(), "il chunk che sfonda va rifiutato");
        assert!(esito.unwrap_err().contains("troppo grande"));
        assert_eq!(
            buf.len(),
            8,
            "ed è il punto del finding: il chunk non deve essere appeso prima del controllo, \
             o la memoria supera comunque il tetto"
        );

        let mut buf = CappedBuffer::new(10);
        assert!(buf.push(&[0u8; 4096]).is_err());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn il_ramo_proxy_non_bufferizza_piu_tutto_il_body() {
        let file = std::fs::read_to_string("src/server/mod.rs").expect("sorgente del server");
        let sorgente = &file[..file.find("#[cfg(test)]\nmod tests {").expect("modulo di test")];
        let inizio = sorgente.find("async fn drop_send(").expect("drop_send");
        let fine = sorgente[inizio..].find("\nasync fn ").map_or(sorgente.len(), |i| i + inizio);
        let corpo = &sorgente[inizio..fine];

        assert!(
            !corpo.contains(".bytes().await"),
            "leggere il campo in una volta carica l'intero body in RAM prima di poterlo \
             rifiutare: il ramo proxy deve leggere a chunk dentro un CappedBuffer"
        );
        assert!(corpo.contains("CappedBuffer::new(MAX_PROXY_BYTES)"));
    }

    #[test]
    fn clean_log_toglie_i_controlli_e_tronca() {
        let forgiato = "login fallito\nERROR utente admin cancellato dal sistema";
        let pulito = clean_log(forgiato);
        assert!(!pulito.contains('\n'), "niente a capo: una riga resta una riga");
        assert!(!pulito.contains('\r'));
        assert_eq!(
            pulito,
            "login fallitoERROR utente admin cancellato dal sistema",
            "il testo resta leggibile, solo appiattito"
        );

        assert_eq!(clean_log("a\tb\u{7}c"), "abc", "tab e bell sono di controllo");
        assert_eq!(clean_log("spazi   normali"), "spazi   normali", "lo spazio resta");
        assert_eq!(clean_log(""), "");

        let lungo = "x".repeat(MAX_LOG_FIELD * 3);
        let troncato = clean_log(&lungo);
        assert_eq!(troncato.chars().count(), MAX_LOG_FIELD + 1, "tronca e segnala col puntino");
        assert!(troncato.ends_with('…'));

        let esatto = "y".repeat(MAX_LOG_FIELD);
        assert_eq!(clean_log(&esatto), esatto, "al limite non si segnala nulla");
    }

    #[test]
    fn clean_log_non_spezza_i_caratteri_multibyte() {
        // 20260806 ++ RG #Security take() conta caratteri, non byte: troncare a metà di una é darebbe una String invalida.
        let accentato = "à".repeat(MAX_LOG_FIELD * 2);
        let troncato = clean_log(&accentato);
        assert_eq!(troncato.chars().count(), MAX_LOG_FIELD + 1);
        assert!(troncato.starts_with('à'));
    }

    #[test]
    fn write_permitted_invariante_di_sicurezza() {
        assert!(write_permitted(true, false));
        assert!(write_permitted(true, true));
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
        assert_eq!(expand_tilde("/var/log/app.log"), "/var/log/app.log");
        assert_eq!(expand_tilde("relativo/dir"), "relativo/dir");
    }
}
