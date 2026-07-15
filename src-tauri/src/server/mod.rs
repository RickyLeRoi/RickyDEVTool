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

    let state = ServerState {
        config,
        bus,
        pollers,
        port,
    };

    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/lan", get(lan_info))
        .route("/api/lan/qr.svg", get(lan_qr))
        .route("/api/pair", post(pair))
        .route("/api/log", post(client_log))
        .route("/api/pollers/{topic}/interval", post(set_interval))
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
            "port": state.port
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
        "data": { "urls": urls, "port": state.port, "lanEnabled": cfg.lan_enabled }
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
