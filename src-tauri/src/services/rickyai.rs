// 20260804 RG due parti: il supervisore accende (o adotta) `of-free serve` su 127.0.0.1, il proxy
// /api/ai/chat lo espone alla SPA. L'endpoint non ha autenticazione: in LAN si passa dal proxy.
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Notify;

use crate::config::{AppConfig, ConfigHandle};
use crate::events::{now_ms, EventBus};
use crate::exec;

pub const DEFAULT_PORT: u16 = 4141;

pub const STRATEGIES: &[&str] = &["balanced", "fast", "local"];

pub const MODES: &[&str] = &["local", "remote"];

// 20260804 RG lista chiusa: solo queste variabili finiscono nell'environment di of-free, così una
// chiave inventata nella config non diventa una variabile arbitraria.
pub const PROVIDER_KEYS: &[(&str, &str, &str)] = &[
    ("groq", "Groq", "GROQ_API_KEY"),
    ("google", "Google AI Studio", "GEMINI_API_KEY"),
    ("cerebras", "Cerebras", "CEREBRAS_API_KEY"),
    ("github", "GitHub Models", "GITHUB_TOKEN"),
    ("mistral", "Mistral La Plateforme", "MISTRAL_API_KEY"),
    ("openrouter", "OpenRouter", "OPENROUTER_API_KEY"),
    ("cohere", "Cohere", "COHERE_API_KEY"),
];

const REMOTE_DEFAULT_PORT: u16 = DEFAULT_PORT;

const PORT_FALLBACK_RANGE: u16 = 10;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

const CHAT_TIMEOUT: Duration = Duration::from_secs(180);

const MAX_LOG_LINES: usize = 60;

const MAX_MESSAGES: usize = 400;
const MAX_CHARS: usize = 400_000;

const ADOPTED_POLL: Duration = Duration::from_secs(5);

const REMOTE_POLL: Duration = Duration::from_secs(30);

const MISSING_RETRY: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AiState {
    Disabled,
    NotInstalled,
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub state: AiState,
    pub port: u16,
    pub base_url: String,
    pub managed: bool,
    pub command: Option<String>,
    pub of_free: bool,
    pub models: Vec<String>,
    pub message: Option<String>,
    pub started_at: Option<u64>,
    pub restarts: u32,
}

impl AiStatus {
    fn new(port: u16) -> Self {
        Self {
            state: AiState::Starting,
            port,
            base_url: base_url(port),
            managed: false,
            command: None,
            of_free: false,
            models: Vec::new(),
            message: None,
            started_at: None,
            restarts: 0,
        }
    }
}

pub fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn valid_remote_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("indirizzo del servizio mancante".to_string());
    }
    let explicit_scheme = trimmed.contains("://");
    let candidate = if explicit_scheme {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let url = reqwest::Url::parse(&candidate)
        .map_err(|_| format!("indirizzo non valido: {trimmed}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("usa http:// o https://".to_string());
    }
    let host = url.host_str().filter(|h| !h.is_empty()).ok_or("manca l'indirizzo del server")?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("l'indirizzo non deve contenere ? o #".to_string());
    }
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/v1").unwrap_or(path);
    let port = match url.port() {
        Some(port) => format!(":{port}"),
        None if !explicit_scheme => format!(":{REMOTE_DEFAULT_PORT}"),
        None => String::new(),
    };
    Ok(format!("{}://{}{}{}", url.scheme(), host, port, path))
}

fn port_of(base: &str) -> u16 {
    reqwest::Url::parse(base)
        .ok()
        .and_then(|u| u.port_or_known_default())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    pub content: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub failovers: Option<u32>,
    pub repinned: Option<String>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiError {
    pub code: &'static str,
    pub message: String,
    pub retry_after: Option<u64>,
    pub status: u16,
}

impl AiError {
    fn new(code: &'static str, status: u16, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), retry_after: None, status }
    }
}

struct Inner {
    status: AiStatus,
    log: Vec<String>,
    child: Option<ChildRef>,
}

struct ChildRef {
    #[cfg(unix)]
    pgid: i32,
    #[cfg(not(unix))]
    pid: u32,
}

pub struct AiService {
    config: ConfigHandle,
    bus: EventBus,
    inner: Mutex<Inner>,
    restart: Notify,
    client: reqwest::Client,
}

static INSTANCE: OnceLock<Arc<AiService>> = OnceLock::new();

impl AiService {
    fn new(config: ConfigHandle, bus: EventBus) -> Self {
        let port = configured_port(&config.get());
        Self {
            config,
            bus,
            inner: Mutex::new(Inner {
                status: AiStatus::new(port),
                log: Vec::new(),
                child: None,
            }),
            restart: Notify::new(),
            client: reqwest::Client::builder()
                // 20260804 RG un HTTP_PROXY di sistema non deve intercettare le chiamate a 127.0.0.1.
                .no_proxy()
                .build()
                .expect("client http"),
        }
    }

    pub fn start(config: ConfigHandle, bus: EventBus) -> Arc<Self> {
        let service = Arc::new(Self::new(config, bus));
        let _ = INSTANCE.set(service.clone());
        tokio::spawn(supervise(service.clone()));
        service
    }

    pub fn status(&self) -> AiStatus {
        self.inner.lock().expect("ai lock").status.clone()
    }

    pub fn snapshot(&self) -> Value {
        let cfg = self.config.get();
        let inner = self.inner.lock().expect("ai lock");
        snapshot_of(&inner.status, &inner.log, &cfg)
    }

    pub async fn detailed_snapshot(&self) -> Value {
        let mut snapshot = self.snapshot();
        let status = self.status();
        if status.state != AiState::Ready {
            return snapshot;
        }
        let cfg = self.config.get();
        let base = status.base_url.clone();
        let key = is_remote(&cfg).then(|| cfg.ai_remote_key.clone()).flatten();
        if status.of_free {
            if let Some(quota) = self.get_json(&format!("{base}/v1/status"), key.as_deref()).await {
                if let Some(providers) = quota.get("providers") {
                    snapshot["providers"] = providers.clone();
                }
                if let Some(next) = quota.get("next") {
                    snapshot["next"] = next.clone();
                }
            }
        }
        if let Some(models) = self.get_json(&format!("{base}/v1/models"), key.as_deref()).await {
            snapshot["models"] = json!(model_ids(&models));
        }
        snapshot
    }

    async fn get_json(&self, url: &str, key: Option<&str>) -> Option<Value> {
        let mut request = self.client.get(url).timeout(PROBE_TIMEOUT);
        if let Some(key) = auth_key(key) {
            request = request.bearer_auth(key);
        }
        let response = request.send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.json::<Value>().await.ok()
    }

    pub fn request_restart(&self) {
        self.restart.notify_one();
    }

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatReply, AiError> {
        validate(&request).map_err(|m| AiError::new("AI_INVALID", 400, m))?;
        let status = self.status();
        if status.state != AiState::Ready {
            return Err(AiError::new("AI_NOT_READY", 503, not_ready_message(&status)));
        }
        let cfg = self.config.get();
        let model = effective_model(request.model.as_deref(), &status);
        let payload = build_payload(&request, &cfg.ai_system_prompt, &model);
        let key = is_remote(&cfg).then(|| cfg.ai_remote_key.clone()).flatten();
        complete(&self.client, &status.base_url, &payload, key.as_deref()).await
    }

    fn update(&self, f: impl FnOnce(&mut AiStatus)) {
        let snapshot = {
            let cfg = self.config.get();
            let mut inner = self.inner.lock().expect("ai lock");
            f(&mut inner.status);
            snapshot_of(&inner.status, &inner.log, &cfg)
        };
        self.bus.publish("ai", snapshot);
    }

    fn set_state(&self, state: AiState, message: Option<String>) {
        self.update(|s| {
            s.state = state;
            s.message = message;
            if state != AiState::Ready {
                s.started_at = None;
            }
        });
    }

    fn push_log(&self, line: String) {
        let mut inner = self.inner.lock().expect("ai lock");
        inner.log.push(line);
        if inner.log.len() > MAX_LOG_LINES {
            let excess = inner.log.len() - MAX_LOG_LINES;
            inner.log.drain(..excess);
        }
    }

    fn clear_log(&self) {
        self.inner.lock().expect("ai lock").log.clear();
    }

    fn log_tail(&self, lines: usize) -> String {
        let inner = self.inner.lock().expect("ai lock");
        let start = inner.log.len().saturating_sub(lines);
        inner.log[start..].join(" · ")
    }

    fn remember_child(&self, child: &tokio::process::Child) {
        let Some(pid) = child.id() else { return };
        let mut inner = self.inner.lock().expect("ai lock");
        inner.child = Some(ChildRef {
            #[cfg(unix)]
            pgid: pid as i32,
            #[cfg(not(unix))]
            pid,
        });
    }

    fn forget_child(&self) {
        self.inner.lock().expect("ai lock").child = None;
    }

    pub fn shutdown(&self) {
        let child = { self.inner.lock().expect("ai lock").child.take() };
        let Some(child) = child else { return };
        #[cfg(unix)]
        {
            unsafe { libc::killpg(child.pgid, libc::SIGTERM) };
        }
        #[cfg(not(unix))]
        {
            let _ = exec::sync_cmd("taskkill")
                .args(["/T", "/F", "/PID", &child.pid.to_string()])
                .output();
        }
        tracing::info!("of-free terminato");
    }
}

pub fn shutdown_all() {
    if let Some(service) = INSTANCE.get() {
        service.shutdown();
    }
}

enum Outcome {
    Restart,
    Idle,
    Crashed,
}

async fn supervise(service: Arc<AiService>) {
    let mut backoff = Duration::from_secs(2);
    loop {
        let cfg = service.config.get();
        if !cfg.ai_enabled {
            service.set_state(AiState::Disabled, Some("avvio automatico disattivato".into()));
            service.restart.notified().await;
            backoff = Duration::from_secs(2);
            continue;
        }

        match run_once(&service, &cfg).await {
            Outcome::Restart => backoff = Duration::from_secs(2),
            Outcome::Idle => {
                tokio::select! {
                    _ = service.restart.notified() => {}
                    _ = tokio::time::sleep(MISSING_RETRY) => {}
                }
                backoff = Duration::from_secs(2);
            }
            Outcome::Crashed => {
                tokio::select! {
                    _ = service.restart.notified() => backoff = Duration::from_secs(2),
                    _ = tokio::time::sleep(backoff) => {
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                    }
                }
            }
        }
    }
}

async fn run_once(service: &Arc<AiService>, cfg: &AppConfig) -> Outcome {
    if is_remote(cfg) {
        // 20260804 RG senza indirizzo non è un errore di configurazione ma un passo ancora da fare.
        let Some(raw) = cfg.ai_remote_url.as_deref().filter(|u| !u.trim().is_empty()) else {
            service.set_state(
                AiState::Failed,
                Some("indica l'indirizzo del servizio nelle impostazioni (es. 192.168.1.50:4141)".into()),
            );
            return Outcome::Idle;
        };
        return match valid_remote_url(raw) {
            Ok(base) => watch_remote(service, &base, cfg.ai_remote_key.as_deref()).await,
            Err(message) => {
                service.set_state(
                    AiState::Failed,
                    Some(format!("indirizzo del servizio non valido: {message}")),
                );
                Outcome::Idle
            }
        };
    }
    match choose_port(service, configured_port(cfg)).await {
        PortChoice::Adopt(port) => {
            tracing::info!(port, "of-free già in ascolto: istanza adottata");
            let models = probe(&service.client, &base_url(port), None)
                .await
                .map(|e| e.models)
                .unwrap_or_default();
            service.update(|s| {
                s.state = AiState::Ready;
                s.port = port;
                s.base_url = base_url(port);
                s.managed = false;
                s.of_free = true;
                s.models = models.clone();
                s.command = None;
                s.message = Some("istanza esterna già in ascolto (non gestita dal tool)".into());
                s.started_at = Some(now_ms());
            });
            watch_adopted(service, port).await
        }
        PortChoice::Spawn(port) => spawn_and_watch(service, cfg, port).await,
        PortChoice::None => {
            service.set_state(
                AiState::Failed,
                Some(format!(
                    "nessuna porta libera tra {} e {}",
                    configured_port(cfg),
                    configured_port(cfg) + PORT_FALLBACK_RANGE - 1
                )),
            );
            Outcome::Idle
        }
    }
}

async fn spawn_and_watch(service: &Arc<AiService>, cfg: &AppConfig, port: u16) -> Outcome {
    let Some(program) = resolve_command(cfg).await else {
        service.update(|s| {
            s.state = AiState::NotInstalled;
            s.managed = false;
            s.command = cfg.ai_command.clone();
            s.message = Some(
                "`of-free` non trovato: installalo (pip install -e .) o indica il percorso \
                 nelle impostazioni"
                    .into(),
            );
            s.started_at = None;
        });
        return Outcome::Idle;
    };

    service.clear_log();
    service.update(|s| {
        s.state = AiState::Starting;
        s.port = port;
        s.base_url = base_url(port);
        s.managed = true;
        s.command = Some(program.clone());
        s.message = None;
        s.started_at = None;
    });

    let mut child = match spawn_serve(&program, cfg, port) {
        Ok(child) => child,
        Err(message) => {
            service.set_state(AiState::Failed, Some(format!("avvio fallito: {message}")));
            return Outcome::Crashed;
        }
    };
    service.remember_child(&child);
    pump_output(service, &mut child);
    tracing::info!(port, program = %program, "of-free in avvio");

    match wait_healthy(service, &mut child, port).await {
        Ok(()) => {
            let models = probe(&service.client, &base_url(port), None)
                .await
                .map(|e| e.models)
                .unwrap_or_default();
            service.update(|s| {
                s.state = AiState::Ready;
                s.of_free = true;
                s.models = models.clone();
                s.message = None;
                s.started_at = Some(now_ms());
            });
        }
        Err(message) => {
            kill(&mut child).await;
            service.forget_child();
            service.set_state(AiState::Failed, Some(message));
            return Outcome::Crashed;
        }
    }

    let outcome = tokio::select! {
        status = child.wait() => {
            let detail = service.log_tail(3);
            let code = status.ok().and_then(|s| s.code());
            let message = match (code, detail.is_empty()) {
                (Some(code), false) => format!("of-free è uscito (codice {code}): {detail}"),
                (Some(code), true) => format!("of-free è uscito (codice {code})"),
                (None, false) => format!("of-free è stato terminato: {detail}"),
                (None, true) => "of-free è stato terminato".to_string(),
            };
            tracing::warn!(%message, "of-free non è più in esecuzione");
            service.update(|s| {
                s.state = AiState::Failed;
                s.message = Some(message);
                s.started_at = None;
                s.restarts = s.restarts.saturating_add(1);
            });
            Outcome::Crashed
        }
        _ = service.restart.notified() => {
            kill(&mut child).await;
            service.update(|s| {
                s.state = AiState::Starting;
                s.message = Some("riavvio richiesto".into());
                s.started_at = None;
                s.restarts = s.restarts.saturating_add(1);
            });
            Outcome::Restart
        }
    };
    service.forget_child();
    outcome
}

async fn wait_healthy(
    service: &Arc<AiService>,
    child: &mut tokio::process::Child,
    port: u16,
) -> Result<(), String> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let detail = service.log_tail(3);
            let code = status.code();
            return Err(match (code, detail.is_empty()) {
                (Some(code), false) => format!("of-free non è partito (codice {code}): {detail}"),
                (Some(code), true) => format!("of-free non è partito (codice {code})"),
                (_, false) => format!("of-free non è partito: {detail}"),
                (_, true) => "of-free non è partito".to_string(),
            });
        }
        if is_of_free(&service.client, &base_url(port)).await {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let detail = service.log_tail(3);
            return Err(if detail.is_empty() {
                format!("of-free non risponde sulla porta {port}")
            } else {
                format!("of-free non risponde sulla porta {port}: {detail}")
            });
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

async fn watch_adopted(service: &Arc<AiService>, port: u16) -> Outcome {
    watch_external(
        service,
        &base_url(port),
        None,
        "l'istanza esterna di of-free non risponde più",
    )
    .await
}

async fn watch_remote(service: &Arc<AiService>, base: &str, key: Option<&str>) -> Outcome {
    let Some(endpoint) = probe(&service.client, base, key).await else {
        service.update(|s| {
            s.state = AiState::Failed;
            s.managed = false;
            s.of_free = false;
            s.command = None;
            s.base_url = base.to_string();
            s.port = port_of(base);
            s.models = Vec::new();
            // 20260805 RG senza il permesso macOS ogni indirizzo in LAN risulta irraggiungibile: dirlo,
            // invece di far sospettare il servizio o la chiave.
            let stato = crate::adapters::localnetwork::status();
            s.message = Some(if stato.supported && !stato.granted {
                format!(
                    "{base} irraggiungibile: manca il permesso Rete locale di macOS. \
                     Concedilo in Impostazioni di Sistema → Privacy e sicurezza → Rete locale, \
                     poi riavvia RickyDEVTool"
                )
            } else {
                format!(
                    "nessun endpoint OpenAI-compatibile su {base}: controlla che il servizio sia \
                     acceso, che sia in ascolto su tutte le interfacce (non solo su 127.0.0.1), e \
                     che la chiave API sia quella giusta se la richiede"
                )
            });
            s.started_at = None;
        });
        return Outcome::Crashed;
    };
    tracing::info!(%base, of_free = endpoint.of_free, modelli = endpoint.models.len(), "servizio remoto raggiungibile");
    service.update(|s| {
        s.state = AiState::Ready;
        s.managed = false;
        s.of_free = endpoint.of_free;
        s.command = None;
        s.base_url = base.to_string();
        s.port = port_of(base);
        s.models = endpoint.models.clone();
        s.message = (!endpoint.of_free).then(|| {
            "endpoint OpenAI-compatibile (non of-free): niente routing fra provider né quote"
                .to_string()
        });
        s.started_at = Some(now_ms());
    });
    watch_external(service, base, key, "il servizio remoto non risponde più").await
}

async fn watch_external(
    service: &Arc<AiService>,
    base: &str,
    key: Option<&str>,
    caduto: &str,
) -> Outcome {
    let every = if base.contains("127.0.0.1") { ADOPTED_POLL } else { REMOTE_POLL };
    loop {
        tokio::select! {
            _ = service.restart.notified() => return Outcome::Restart,
            _ = tokio::time::sleep(every) => {}
        }
        if !alive(&service.client, base, key).await {
            service.set_state(AiState::Failed, Some(caduto.to_string()));
            return Outcome::Crashed;
        }
    }
}

fn pump_output(service: &Arc<AiService>, child: &mut tokio::process::Child) {
    if let Some(out) = child.stdout.take() {
        tokio::spawn(pump(service.clone(), out));
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(pump(service.clone(), err));
    }
}

async fn pump<R: tokio::io::AsyncRead + Unpin>(service: Arc<AiService>, reader: R) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        tracing::debug!(target: "of-free", "{line}");
        service.push_log(line);
    }
}

async fn kill(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
}

enum PortChoice {
    Adopt(u16),
    Spawn(u16),
    None,
}

async fn choose_port(service: &Arc<AiService>, base: u16) -> PortChoice {
    for candidate in base..base.saturating_add(PORT_FALLBACK_RANGE) {
        if is_of_free(&service.client, &base_url(candidate)).await {
            return PortChoice::Adopt(candidate);
        }
        if port_free(candidate).await {
            return PortChoice::Spawn(candidate);
        }
    }
    PortChoice::None
}

pub fn configured_port(cfg: &AppConfig) -> u16 {
    if cfg.ai_port == 0 {
        DEFAULT_PORT
    } else {
        cfg.ai_port
    }
}

pub fn is_remote(cfg: &AppConfig) -> bool {
    cfg.ai_mode.trim() == "remote"
}

pub fn mode(cfg: &AppConfig) -> String {
    let requested = cfg.ai_mode.trim();
    if MODES.contains(&requested) {
        requested.to_string()
    } else {
        "local".to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub models: Vec<String>,
    pub of_free: bool,
}

async fn probe(client: &reqwest::Client, base: &str, key: Option<&str>) -> Option<Endpoint> {
    // 20260804 RG /v1/models e non /health: quest'ultimo è solo di of-free, gli altri danno 404.
    let url = format!("{base}/v1/models");
    let mut request = client.get(&url).timeout(PROBE_TIMEOUT);
    if let Some(key) = auth_key(key) {
        request = request.bearer_auth(key);
    }
    // 20260805 RG l'errore di trasporto va a log: senza, un blocco di sistema e un servizio spento
    // sono indistinguibili.
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::debug!(%url, errore = %e, "sonda endpoint fallita");
            return None;
        }
    };
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    let models = model_ids(&body);
    // 20260804 RG Ollama senza modelli scaricati risponde 200 con `data: []`: non è pronto.
    if models.is_empty() {
        return None;
    }
    let of_free = models.iter().any(|id| id == "auto");
    Some(Endpoint { models, of_free })
}

async fn is_of_free(client: &reqwest::Client, base: &str) -> bool {
    probe(client, base, None).await.is_some_and(|e| e.of_free)
}

async fn alive(client: &reqwest::Client, base: &str, key: Option<&str>) -> bool {
    probe(client, base, key).await.is_some()
}

fn auth_key(key: Option<&str>) -> Option<&str> {
    key.map(str::trim).filter(|k| !k.is_empty())
}

pub fn model_ids(body: &Value) -> Vec<String> {
    body.get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

async fn port_free(port: u16) -> bool {
    tokio::net::TcpListener::bind(("127.0.0.1", port)).await.is_ok()
}

async fn resolve_command(cfg: &AppConfig) -> Option<String> {
    if let Some(custom) = cfg.ai_command.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Path::new(custom).is_file().then(|| custom.to_string());
    }
    which("of-free").await
}

async fn which(name: &str) -> Option<String> {
    #[cfg(windows)]
    let (program, arg) = ("where", name);
    #[cfg(not(windows))]
    let (program, arg) = ("/usr/bin/which", name);

    let out = exec::text(exec::cmd(program).arg(arg)).await?;
    let path = out.lines().next()?.trim().to_string();
    (!path.is_empty()).then_some(path)
}

pub fn serve_args(cfg: &AppConfig, port: u16) -> Vec<String> {
    let mut args = Vec::new();
    args.push("serve".to_string());
    args.push("--host".to_string());
    // 20260804 RG mai 0.0.0.0: l'endpoint non ha autenticazione.
    args.push("127.0.0.1".to_string());
    args.push("--port".to_string());
    args.push(port.to_string());
    args.push("--strategy".to_string());
    args.push(strategy(cfg));
    args
}

pub fn strategy(cfg: &AppConfig) -> String {
    let requested = cfg.ai_strategy.trim();
    if STRATEGIES.contains(&requested) {
        requested.to_string()
    } else {
        "balanced".to_string()
    }
}

pub fn serve_env(cfg: &AppConfig) -> Vec<(String, String)> {
    PROVIDER_KEYS
        .iter()
        .filter_map(|(_, _, var)| {
            let value = cfg.ai_keys.get(*var)?.trim();
            (!value.is_empty()).then(|| ((*var).to_string(), value.to_string()))
        })
        .collect()
}

pub fn keys_set(cfg: &AppConfig) -> Vec<String> {
    serve_env(cfg).into_iter().map(|(name, _)| name).collect()
}

fn spawn_serve(
    program: &str,
    cfg: &AppConfig,
    port: u16,
) -> Result<tokio::process::Child, std::io::Error> {
    let mut command = exec::cmd(program);
    command
        .args(serve_args(cfg, port))
        .envs(serve_env(cfg))
        .current_dir(working_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    // 20260804 RG process group proprio: un solo segnale termina python e i suoi figli.
    command.process_group(0);
    command.spawn()
}

// 20260804 RG non la home: of-free si ferma al primo `.env` che trova partendo dalla cwd, e un
// ~/.env di altro contenuto gli nasconderebbe ~/.onfeather/.env.
fn working_dir() -> std::path::PathBuf {
    crate::config::data_dir()
}

fn not_ready_message(status: &AiStatus) -> String {
    match status.state {
        AiState::Disabled => "RickyAI è disattivato dalle impostazioni".to_string(),
        AiState::NotInstalled => {
            "`of-free` non è installato su questo computer".to_string()
        }
        AiState::Starting => "RickyAI si sta avviando: riprova tra qualche secondo".to_string(),
        _ => status
            .message
            .clone()
            .unwrap_or_else(|| "RickyAI non è disponibile".to_string()),
    }
}

fn snapshot_of(status: &AiStatus, log: &[String], cfg: &AppConfig) -> Value {
    json!({
        "state": status.state,
        "port": status.port,
        "baseUrl": status.base_url,
        "managed": status.managed,
        "command": status.command,
        "message": status.message,
        "startedAt": status.started_at,
        "restarts": status.restarts,
        "log": log,
        "ofFree": status.of_free,
        "models": status.models,
        "enabled": cfg.ai_enabled,
        "mode": mode(cfg),
        "remoteUrl": cfg.ai_remote_url,
        "remoteKeySet": auth_key(cfg.ai_remote_key.as_deref()).is_some(),
        "configuredPort": configured_port(cfg),
        "strategy": strategy(cfg),
        "systemPrompt": cfg.ai_system_prompt,
        "keysSet": keys_set(cfg),
        "providerKeys": PROVIDER_KEYS
            .iter()
            .map(|(id, label, var)| json!({ "id": id, "label": label, "env": var }))
            .collect::<Vec<_>>(),
        "providers": Value::Null,
        "next": Value::Null,
    })
}

pub fn validate(request: &ChatRequest) -> Result<(), String> {
    if request.messages.is_empty() {
        return Err("nessun messaggio da inviare".to_string());
    }
    if request.messages.len() > MAX_MESSAGES {
        return Err(format!("troppi messaggi (max {MAX_MESSAGES})"));
    }
    for message in &request.messages {
        if !matches!(message.role.as_str(), "system" | "user" | "assistant") {
            return Err(format!("ruolo non valido: {}", message.role));
        }
    }
    let total: usize = request.messages.iter().map(|m| m.content.len()).sum();
    if total > MAX_CHARS {
        return Err(format!("conversazione troppo lunga (max {MAX_CHARS} caratteri)"));
    }
    let last = request.messages.last().expect("messaggi non vuoti");
    if last.role != "user" {
        return Err("l'ultimo messaggio deve essere dell'utente".to_string());
    }
    if last.content.trim().is_empty() {
        return Err("il messaggio è vuoto".to_string());
    }
    Ok(())
}

pub fn effective_model(requested: Option<&str>, status: &AiStatus) -> String {
    let requested = requested
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or("auto");
    if status.of_free {
        return requested.to_string();
    }
    // 20260804 RG `auto` e `private` esistono solo dentro of-free: altrove sono modelli inesistenti,
    // quindi si ripiega sul primo della lista.
    if matches!(requested, "auto" | "private" | "local") {
        return status
            .models
            .first()
            .cloned()
            .unwrap_or_else(|| requested.to_string());
    }
    requested.to_string()
}

pub fn build_payload(request: &ChatRequest, system_prompt: &str, model: &str) -> Value {
    let mut messages: Vec<Value> = Vec::with_capacity(request.messages.len() + 1);
    let system = system_prompt.trim();
    // 20260804 RG solo se la conversazione non porta già il suo system: due si contraddicono.
    if !system.is_empty() && request.messages.first().map(|m| m.role.as_str()) != Some("system") {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &request.messages {
        messages.push(json!({ "role": message.role, "content": message.content }));
    }

    let mut payload = json!({
        "model": model,
        "messages": messages,
        // 20260804 RG of-free rifiuta `stream: true` con un 400: qui non deve poterci finire.
        "stream": false,
    });
    if let Some(temperature) = request.temperature {
        payload["temperature"] = json!(temperature.clamp(0.0, 2.0));
    }
    if let Some(max_tokens) = request.max_tokens {
        payload["max_tokens"] = json!(max_tokens.clamp(1, 32_000));
    }
    payload
}

pub async fn complete(
    client: &reqwest::Client,
    base: &str,
    payload: &Value,
    key: Option<&str>,
) -> Result<ChatReply, AiError> {
    let started = Instant::now();
    let mut request = client
        .post(format!("{base}/v1/chat/completions"))
        .json(payload)
        .timeout(CHAT_TIMEOUT);
    if let Some(key) = auth_key(key) {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                AiError::new("AI_TIMEOUT", 504, "nessuna risposta entro il tempo massimo")
            } else {
                AiError::new("AI_UNREACHABLE", 502, format!("of-free non risponde: {e}"))
            }
        })?;

    let status = response.status();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let provider = header("x-onfeather-provider");
    let model = header("x-onfeather-model");
    let failovers = header("x-onfeather-failovers").and_then(|v| v.parse::<u32>().ok());
    let repinned = header("x-onfeather-repinned");
    let retry_after = header("retry-after").and_then(|v| v.parse::<u64>().ok());

    let body = response.json::<Value>().await.ok();

    if !status.is_success() {
        let error_body = body.as_ref().and_then(|b| b.get("error"));
        let field = |name: &str| {
            error_body
                .and_then(|e| e.get(name))
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        let detail = match field("message") {
            "" => "errore dall'endpoint",
            message => message,
        };
        let mut error = if status.as_u16() == 429 {
            AiError::new("AI_QUOTA", 429, format!("Quota esaurita: {detail}"))
        } else if field("type") == "no_route" {
            AiError::new(
                "AI_NO_ROUTE",
                503,
                format!(
                    "Nessun provider utilizzabile: configura le chiavi in ~/.onfeather/.env \
                     (o indica il file dalle impostazioni), oppure avvia Ollama per i modelli \
                     locali. Dettaglio: {detail}"
                ),
            )
        } else {
            AiError::new("AI_UPSTREAM", 502, detail.to_string())
        };
        error.retry_after = retry_after;
        return Err(error);
    }

    let body = body.ok_or_else(|| AiError::new("AI_UPSTREAM", 502, "risposta non leggibile"))?;
    let content = extract_content(&body).ok_or_else(|| {
        AiError::new("AI_EMPTY", 502, "il modello non ha restituito testo")
    })?;

    Ok(ChatReply {
        content,
        provider: provider.or_else(|| {
            body.get("model")
                .and_then(Value::as_str)
                .and_then(|m| m.split_once('/'))
                .map(|(p, _)| p.to_string())
        }),
        model: model.or_else(|| {
            body.get("model").and_then(Value::as_str).map(str::to_string)
        }),
        failovers,
        repinned,
        finish_reason: body
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage: extract_usage(&body),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

pub fn extract_content(body: &Value) -> Option<String> {
    let content = body.pointer("/choices/0/message/content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        // 20260804 RG alcuni provider rispondono a blocchi `[{type:"text", …}]` invece che con una
        // stringa: senza questo ramo la chat resta muta.
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}

fn extract_usage(body: &Value) -> Option<Usage> {
    let usage = body.get("usage")?;
    let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    Some(Usage {
        prompt_tokens: field("prompt_tokens"),
        completion_tokens: field("completion_tokens"),
        total_tokens: field("total_tokens"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(f: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let mut cfg = AppConfig::default();
        f(&mut cfg);
        cfg
    }

    fn message(role: &str, content: &str) -> ChatMessage {
        ChatMessage { role: role.into(), content: content.into() }
    }

    fn request(messages: Vec<ChatMessage>) -> ChatRequest {
        ChatRequest { messages, model: None, temperature: None, max_tokens: None }
    }

    #[test]
    fn serve_args_tiene_of_free_su_localhost() {
        let args = serve_args(&AppConfig::default(), 4141);
        let host = args.iter().position(|a| a == "--host").expect("--host presente");
        assert_eq!(args[host + 1], "127.0.0.1");
        assert!(!args.iter().any(|a| a.contains("0.0.0.0")));
    }

    #[test]
    fn serve_args_porta_e_strategia() {
        let cfg = config_with(|c| c.ai_strategy = "fast".into());
        let args = serve_args(&cfg, 4200);
        let port = args.iter().position(|a| a == "--port").unwrap();
        assert_eq!(args[port + 1], "4200");
        let strategy = args.iter().position(|a| a == "--strategy").unwrap();
        assert_eq!(args[strategy + 1], "fast");
    }

    #[test]
    fn le_chiavi_finiscono_nellenvironment_del_figlio() {
        let cfg = config_with(|c| {
            c.ai_keys.insert("GROQ_API_KEY".into(), "  gsk_abc  ".into());
            c.ai_keys.insert("MISTRAL_API_KEY".into(), "".into());
        });
        let env = serve_env(&cfg);
        assert_eq!(env, vec![("GROQ_API_KEY".to_string(), "gsk_abc".to_string())]);
        assert!(!serve_args(&cfg, 4141).iter().any(|a| a.contains("gsk_abc")));
    }

    #[test]
    fn solo_le_variabili_note_diventano_environment() {
        let cfg = config_with(|c| {
            c.ai_keys.insert("PATH".into(), "/tmp/evil".into());
            c.ai_keys.insert("LD_PRELOAD".into(), "/tmp/evil.so".into());
            c.ai_keys.insert("GROQ_API_KEY".into(), "gsk_ok".into());
        });
        let names: Vec<String> = serve_env(&cfg).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["GROQ_API_KEY"]);
    }

    #[test]
    fn lo_stato_dice_quali_chiavi_ci_sono_non_quali_sono() {
        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_keys.insert("GROQ_API_KEY".into(), "gsk_segretissima".into());
            c.ai_keys.insert("GEMINI_API_KEY".into(), "AIza_segretissima".into());
        });
        let service = Arc::new(AiService::new(config, EventBus::new()));
        let snapshot = service.snapshot();

        assert_eq!(snapshot["keysSet"], json!(["GROQ_API_KEY", "GEMINI_API_KEY"]));
        let serialized = snapshot.to_string();
        assert!(!serialized.contains("gsk_segretissima"), "chiave esposta: {serialized}");
        assert!(!serialized.contains("AIza_segretissima"), "chiave esposta: {serialized}");
    }

    #[test]
    fn il_catalogo_dei_provider_combacia_con_of_free() {
        let vars: Vec<&str> = PROVIDER_KEYS.iter().map(|(_, _, var)| *var).collect();
        assert!(vars.contains(&"GROQ_API_KEY"));
        assert!(vars.contains(&"GEMINI_API_KEY"));
        assert!(vars.contains(&"GITHUB_TOKEN"));
        assert_eq!(vars.len(), 7);
        assert!(!PROVIDER_KEYS.iter().any(|(id, _, _)| *id == "ollama"));
    }

    #[test]
    fn indirizzo_remoto_accetta_quello_che_si_scrive_davvero() {
        assert_eq!(valid_remote_url("192.168.1.50").unwrap(), "http://192.168.1.50:4141");
        assert_eq!(valid_remote_url(" 192.168.1.50:8080 ").unwrap(), "http://192.168.1.50:8080");
        assert_eq!(valid_remote_url("nas.local:4141").unwrap(), "http://nas.local:4141");
        assert_eq!(
            valid_remote_url("http://192.168.1.50:4141/v1").unwrap(),
            "http://192.168.1.50:4141"
        );
        assert_eq!(valid_remote_url("https://ai.casa.lan").unwrap(), "https://ai.casa.lan");
    }

    #[test]
    fn indirizzo_remoto_conserva_il_path() {
        assert_eq!(
            valid_remote_url("https://openrouter.ai/api/v1").unwrap(),
            "https://openrouter.ai/api"
        );
        assert_eq!(
            valid_remote_url("https://openrouter.ai/api").unwrap(),
            "https://openrouter.ai/api"
        );
        assert_eq!(
            valid_remote_url("http://nas.local/llm/v1/").unwrap(),
            "http://nas.local/llm"
        );
    }

    #[test]
    fn indirizzo_remoto_rifiuta_il_resto() {
        for bad in [
            "",
            "   ",
            "ftp://host",
            "http://",
            "http://host:4141/?token=abc",
            "http://host:4141/#/ciao",
        ] {
            assert!(valid_remote_url(bad).is_err(), "doveva rifiutare: {bad:?}");
        }
    }

    #[test]
    fn strategia_sconosciuta_ricade_su_balanced() {
        assert_eq!(strategy(&config_with(|c| c.ai_strategy = "turbo".into())), "balanced");
        assert_eq!(strategy(&config_with(|c| c.ai_strategy = "local".into())), "local");
        assert_eq!(strategy(&AppConfig::default()), "balanced");
    }

    #[test]
    fn porta_zero_ricade_sul_default() {
        assert_eq!(configured_port(&config_with(|c| c.ai_port = 0)), DEFAULT_PORT);
        assert_eq!(configured_port(&config_with(|c| c.ai_port = 5000)), 5000);
    }

    #[test]
    fn working_dir_non_contiene_un_env_che_dirotti_le_chiavi() {
        assert!(!working_dir().join(".env").exists());
    }

    #[test]
    fn validate_accetta_una_conversazione_normale() {
        let req = request(vec![
            message("system", "sei RickyAI"),
            message("user", "ciao"),
            message("assistant", "ciao!"),
            message("user", "come stai?"),
        ]);
        assert!(validate(&req).is_ok());
    }

    #[test]
    fn validate_rifiuta_i_casi_degeneri() {
        assert!(validate(&request(vec![])).is_err());
        assert!(validate(&request(vec![message("tool", "x"), message("user", "ciao")])).is_err());
        assert!(validate(&request(vec![message("user", "ciao"), message("assistant", "ciao")])).is_err());
        assert!(validate(&request(vec![message("user", "   ")])).is_err());
    }

    #[test]
    fn validate_applica_i_tetti() {
        let troppi: Vec<ChatMessage> = (0..MAX_MESSAGES + 1)
            .map(|i| message(if i % 2 == 0 { "user" } else { "assistant" }, "x"))
            .collect();
        assert!(validate(&request(troppi)).is_err());

        let enorme = request(vec![message("user", &"a".repeat(MAX_CHARS + 1))]);
        assert!(validate(&enorme).is_err());
    }

    #[test]
    fn build_payload_non_chiede_mai_lo_streaming() {
        let payload = build_payload(&request(vec![message("user", "ciao")]), "", "auto");
        assert_eq!(payload["stream"], json!(false));
        assert_eq!(payload["model"], json!("auto"));
    }

    #[test]
    fn build_payload_antepone_il_prompt_di_sistema() {
        let payload = build_payload(&request(vec![message("user", "ciao")]), "sei RickyAI", "auto");
        assert_eq!(payload["messages"][0]["role"], json!("system"));
        assert_eq!(payload["messages"][0]["content"], json!("sei RickyAI"));
        assert_eq!(payload["messages"][1]["role"], json!("user"));
    }

    #[test]
    fn build_payload_non_duplica_il_system() {
        let req = request(vec![message("system", "sei un pirata"), message("user", "ciao")]);
        let payload = build_payload(&req, "sei RickyAI", "auto");
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.iter().filter(|m| m["role"] == json!("system")).count(), 1);
        assert_eq!(messages[0]["content"], json!("sei un pirata"));
    }

    #[test]
    fn build_payload_passa_modello_e_parametri() {
        let req = ChatRequest {
            messages: vec![message("user", "ciao")],
            model: Some("  private  ".into()),
            temperature: Some(9.0),
            max_tokens: Some(0),
        };
        let payload = build_payload(&req, "", "private");
        assert_eq!(payload["model"], json!("private"));
        assert_eq!(payload["temperature"], json!(2.0));
        assert_eq!(payload["max_tokens"], json!(1));
    }

    #[test]
    fn extract_content_legge_stringa_e_blocchi() {
        let stringa = json!({ "choices": [{ "message": { "content": "ciao" } }] });
        assert_eq!(extract_content(&stringa).as_deref(), Some("ciao"));

        let blocchi = json!({
            "choices": [{ "message": { "content": [
                { "type": "text", "text": "ci" },
                { "type": "text", "text": "ao" },
            ] } }]
        });
        assert_eq!(extract_content(&blocchi).as_deref(), Some("ciao"));

        let vuoto = json!({ "choices": [{ "message": { "content": null } }] });
        assert!(extract_content(&vuoto).is_none());
        assert!(extract_content(&json!({ "choices": [] })).is_none());
    }

    #[test]
    fn model_ids_legge_la_lista_openai() {
        let body = json!({ "object": "list", "data": [
            { "id": "auto" },
            { "id": "groq/llama-3.3-70b" },
        ]});
        assert_eq!(model_ids(&body), vec!["auto", "groq/llama-3.3-70b"]);
        assert!(model_ids(&json!({})).is_empty());
    }

    async fn fake_of_free(
        chat: axum::routing::MethodRouter<()>,
    ) -> (u16, tokio::task::JoinHandle<()>) {
        use axum::routing::get;
        let app = axum::Router::new()
            .route("/health", get(|| async { axum::Json(json!({ "status": "ok" })) }))
            .route(
                "/v1/models",
                get(|| async {
                    axum::Json(json!({ "object": "list", "data": [{ "id": "auto" }] }))
                }),
            )
            .route("/v1/chat/completions", chat);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (port, handle)
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    #[tokio::test]
    async fn complete_legge_testo_e_provenienza() {
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async {
            (
                [
                    ("X-OnFeather-Provider", "groq"),
                    ("X-OnFeather-Model", "llama-3.3-70b"),
                    ("X-OnFeather-Failovers", "2"),
                ],
                axum::Json(json!({
                    "model": "groq/llama-3.3-70b",
                    "choices": [{ "message": { "role": "assistant", "content": "ciao!" },
                                  "finish_reason": "stop" }],
                    "usage": { "prompt_tokens": 12, "completion_tokens": 3, "total_tokens": 15 },
                })),
            )
        }))
        .await;

        let reply = complete(&test_client(), &base_url(port), &json!({}), None)
            .await
            .expect("risposta");
        assert_eq!(reply.content, "ciao!");
        assert_eq!(reply.provider.as_deref(), Some("groq"));
        assert_eq!(reply.model.as_deref(), Some("llama-3.3-70b"));
        assert_eq!(reply.failovers, Some(2));
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            reply.usage,
            Some(Usage { prompt_tokens: 12, completion_tokens: 3, total_tokens: 15 })
        );
        server.abort();
    }

    #[tokio::test]
    async fn complete_ricava_la_provenienza_dal_body_senza_header() {
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async {
            axum::Json(json!({
                "model": "ollama/qwen2.5:7b",
                "choices": [{ "message": { "content": "locale" } }],
            }))
        }))
        .await;

        let reply = complete(&test_client(), &base_url(port), &json!({}), None).await.unwrap();
        assert_eq!(reply.provider.as_deref(), Some("ollama"));
        assert_eq!(reply.model.as_deref(), Some("ollama/qwen2.5:7b"));
        server.abort();
    }

    #[tokio::test]
    async fn quota_esaurita_diventa_un_errore_con_attesa() {
        use axum::http::StatusCode;
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async {
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("Retry-After", "42")],
                axum::Json(json!({ "error": { "message": "no quota left", "code": "quota_exhausted" } })),
            )
        }))
        .await;

        let error = complete(&test_client(), &base_url(port), &json!({}), None)
            .await
            .expect_err("429 è un errore");
        assert_eq!(error.code, "AI_QUOTA");
        assert_eq!(error.status, 429);
        assert_eq!(error.retry_after, Some(42));
        assert!(error.message.contains("no quota left"));
        server.abort();
    }

    #[tokio::test]
    async fn errore_upstream_riporta_il_messaggio_del_provider() {
        use axum::http::StatusCode;
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async {
            (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({ "error": { "message": "modello sconosciuto" } })),
            )
        }))
        .await;

        let error = complete(&test_client(), &base_url(port), &json!({}), None).await.unwrap_err();
        assert_eq!(error.code, "AI_UPSTREAM");
        assert_eq!(error.message, "modello sconosciuto");
        assert!(error.retry_after.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn risposta_senza_testo_non_passa_per_buona() {
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async {
            axum::Json(json!({ "choices": [{ "message": { "content": null } }] }))
        }))
        .await;

        let error = complete(&test_client(), &base_url(port), &json!({}), None).await.unwrap_err();
        assert_eq!(error.code, "AI_EMPTY");
        server.abort();
    }

    #[tokio::test]
    async fn endpoint_spento_non_e_un_errore_di_protocollo() {
        let error = complete(&test_client(), &base_url(1), &json!({}), None).await.unwrap_err();
        assert_eq!(error.code, "AI_UNREACHABLE");
        assert_eq!(error.status, 502);
    }

    #[tokio::test]
    async fn identify_riconosce_of_free() {
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async { axum::Json(json!({})) })).await;
        assert!(is_of_free(&test_client(), &base_url(port)).await);
        assert!(alive(&test_client(), &base_url(port), None).await);
        server.abort();
    }

    #[tokio::test]
    async fn identify_non_adotta_un_servizio_qualsiasi() {
        use axum::routing::get;
        let app = axum::Router::new().fallback(get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        assert!(!is_of_free(&test_client(), &base_url(port)).await);
        server.abort();
    }

    fn test_service() -> Arc<AiService> {
        Arc::new(AiService::new(ConfigHandle::in_memory(), EventBus::new()))
    }

    #[tokio::test]
    async fn chat_rifiutata_finche_non_e_pronto() {
        let service = test_service();
        service.set_state(AiState::Starting, None);
        let error = service
            .chat(request(vec![message("user", "ciao")]))
            .await
            .unwrap_err();
        assert_eq!(error.code, "AI_NOT_READY");
        assert_eq!(error.status, 503);
        assert!(error.message.contains("avviando"));

        service.set_state(AiState::NotInstalled, None);
        let error = service.chat(request(vec![message("user", "ciao")])).await.unwrap_err();
        assert!(error.message.contains("of-free"));
    }

    #[tokio::test]
    async fn chat_valida_prima_di_uscire_dalla_macchina() {
        let service = test_service();
        service.set_state(AiState::Ready, None);
        let error = service.chat(request(vec![])).await.unwrap_err();
        assert_eq!(error.code, "AI_INVALID");
        assert_eq!(error.status, 400);
    }

    #[tokio::test]
    async fn lo_stato_viene_pubblicato_sul_bus() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let service = Arc::new(AiService::new(ConfigHandle::in_memory(), bus));

        service.set_state(AiState::Ready, Some("pronto".into()));
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("evento")
            .expect("bus aperto");
        assert_eq!(event.topic, "ai");
        assert_eq!(event.payload["state"], json!("ready"));
        assert_eq!(event.payload["message"], json!("pronto"));
    }

    #[tokio::test]
    async fn il_log_del_processo_e_un_ring_buffer() {
        let service = test_service();
        for i in 0..MAX_LOG_LINES + 10 {
            service.push_log(format!("riga {i}"));
        }
        let snapshot = service.snapshot();
        let log = snapshot["log"].as_array().unwrap();
        assert_eq!(log.len(), MAX_LOG_LINES);
        assert_eq!(log.last().unwrap(), &json!(format!("riga {}", MAX_LOG_LINES + 9)));
        assert_eq!(service.log_tail(2), format!("riga {} · riga {}", MAX_LOG_LINES + 8, MAX_LOG_LINES + 9));
    }

    #[tokio::test]
    async fn snapshot_riporta_la_configurazione_corrente() {
        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_enabled = true;
            c.ai_port = 4300;
            c.ai_strategy = "fast".into();
            c.ai_system_prompt = "sei RickyAI".into();
        });
        let service = Arc::new(AiService::new(config, EventBus::new()));
        let snapshot = service.snapshot();
        assert_eq!(snapshot["configuredPort"], json!(4300));
        assert_eq!(snapshot["strategy"], json!("fast"));
        assert_eq!(snapshot["systemPrompt"], json!("sei RickyAI"));
        assert_eq!(snapshot["enabled"], json!(true));
    }

    async fn fake_openai(
        modelli: &[&str],
        key: Option<&'static str>,
    ) -> (u16, tokio::task::JoinHandle<()>) {
        use axum::extract::Request;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use axum::routing::{get, post};

        let listed: Vec<String> = modelli.iter().map(|m| m.to_string()).collect();
        let autorizza = move |request: &Request| match key {
            None => true,
            Some(expected) => {
                request
                    .headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    == Some(&format!("Bearer {expected}"))
            }
        };

        let models_body = json!({
            "object": "list",
            "data": listed.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>(),
        });
        let app = axum::Router::new()
            .route(
                "/v1/models",
                get(move |request: Request| {
                    let body = models_body.clone();
                    async move {
                        if !autorizza(&request) {
                            return (StatusCode::UNAUTHORIZED, axum::Json(json!({}))).into_response();
                        }
                        axum::Json(body).into_response()
                    }
                }),
            )
            .route(
                "/v1/chat/completions",
                post(move |request: Request| async move {
                    if !autorizza(&request) {
                        return (
                            StatusCode::UNAUTHORIZED,
                            axum::Json(json!({ "error": { "message": "No auth credentials found" } })),
                        )
                            .into_response();
                    }
                    axum::Json(json!({
                        "model": "qwen2.5:7b",
                        "choices": [{ "message": { "content": "risposta generica" } }],
                    }))
                    .into_response()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (port, handle)
    }

    #[tokio::test]
    async fn un_endpoint_generico_viene_riconosciuto_ma_non_scambiato_per_of_free() {
        let (port, server) = fake_openai(&["qwen2.5:7b", "llama3.2:3b"], None).await;
        let endpoint = probe(&test_client(), &base_url(port), None)
            .await
            .expect("endpoint riconosciuto");
        assert!(!endpoint.of_free, "non espone `auto`: non è of-free");
        assert_eq!(endpoint.models, vec!["qwen2.5:7b", "llama3.2:3b"]);

        assert!(!is_of_free(&test_client(), &base_url(port)).await);
        match choose_port(&test_service(), port).await {
            PortChoice::Adopt(_) => panic!("non doveva adottare un endpoint non-of-free"),
            _ => {}
        }
        server.abort();
    }

    #[tokio::test]
    async fn un_endpoint_senza_modelli_non_e_pronto() {
        let (port, server) = fake_openai(&[], None).await;
        assert!(probe(&test_client(), &base_url(port), None).await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn su_un_endpoint_generico_auto_diventa_un_modello_vero() {
        let mut status = AiStatus::new(4141);
        status.of_free = false;
        status.models = vec!["qwen2.5:7b".into(), "llama3.2:3b".into()];

        assert_eq!(effective_model(None, &status), "qwen2.5:7b");
        assert_eq!(effective_model(Some("auto"), &status), "qwen2.5:7b");
        assert_eq!(effective_model(Some("private"), &status), "qwen2.5:7b");
        assert_eq!(effective_model(Some("llama3.2:3b"), &status), "llama3.2:3b");

        status.of_free = true;
        assert_eq!(effective_model(Some("auto"), &status), "auto");
        assert_eq!(effective_model(None, &status), "auto");
        assert_eq!(effective_model(Some("private"), &status), "private");
    }

    #[tokio::test]
    async fn la_chiave_del_servizio_remoto_viene_mandata() {
        let (port, server) = fake_openai(&["openai/gpt-4o-mini"], Some("sk-or-test")).await;
        let base = base_url(port);

        assert!(probe(&test_client(), &base, None).await.is_none(), "senza chiave: 401");
        let endpoint = probe(&test_client(), &base, Some("sk-or-test")).await;
        assert!(endpoint.is_some(), "con la chiave giusta deve rispondere");

        let payload = json!({ "model": "openai/gpt-4o-mini", "messages": [] });
        let senza = complete(&test_client(), &base, &payload, None).await;
        assert!(senza.is_err(), "senza chiave la chat deve fallire");
        let con = complete(&test_client(), &base, &payload, Some("  sk-or-test  "))
            .await
            .expect("con la chiave passa");
        assert_eq!(con.content, "risposta generica");
        server.abort();
    }

    #[tokio::test]
    async fn un_ollama_in_rete_diventa_utilizzabile() {
        let (port, server) = fake_openai(&["qwen2.5:7b"], None).await;
        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_enabled = true;
            c.ai_mode = "remote".into();
            c.ai_remote_url = Some(format!("127.0.0.1:{port}"));
        });
        let service = Arc::new(AiService::new(config.clone(), EventBus::new()));

        let watched = service.clone();
        let base = valid_remote_url(&format!("127.0.0.1:{port}")).unwrap();
        let watcher = tokio::spawn(async move {
            watch_remote(&watched, &base, None).await;
        });
        tokio::time::sleep(Duration::from_millis(300)).await;

        let status = service.status();
        assert_eq!(status.state, AiState::Ready);
        assert!(!status.of_free);
        assert_eq!(status.models, vec!["qwen2.5:7b"]);
        assert!(status.message.unwrap_or_default().contains("non of-free"));

        let reply = service.chat(request(vec![message("user", "ciao")])).await.expect("risposta");
        assert_eq!(reply.content, "risposta generica");

        let snapshot = service.detailed_snapshot().await;
        assert_eq!(snapshot["providers"], Value::Null);
        assert_eq!(snapshot["ofFree"], json!(false));

        watcher.abort();
        server.abort();
    }

    #[tokio::test]
    async fn la_chiave_remota_non_esce_dallo_stato() {
        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_mode = "remote".into();
            c.ai_remote_url = Some("http://192.168.1.50:4141".into());
            c.ai_remote_key = Some("sk-or-segretissima".into());
        });
        let service = Arc::new(AiService::new(config, EventBus::new()));
        let snapshot = service.snapshot();

        assert_eq!(snapshot["remoteKeySet"], json!(true));
        assert!(
            !snapshot.to_string().contains("sk-or-segretissima"),
            "chiave del servizio remoto esposta: {snapshot}"
        );
    }

    #[tokio::test]
    async fn in_modalita_remota_la_chat_va_sul_servizio_remoto() {
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async {
            (
                [("X-OnFeather-Provider", "groq")],
                axum::Json(json!({
                    "choices": [{ "message": { "content": "dal server di casa" } }]
                })),
            )
        }))
        .await;

        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_enabled = true;
            c.ai_mode = "remote".into();
            c.ai_remote_url = Some(format!("127.0.0.1:{port}"));
            c.ai_port = 4141;
        });
        let service = Arc::new(AiService::new(config.clone(), EventBus::new()));

        let watched = service.clone();
        let base = valid_remote_url(&format!("127.0.0.1:{port}")).unwrap();
        let watcher = tokio::spawn(async move {
            watch_remote(&watched, &base, None).await;
        });
        tokio::time::sleep(Duration::from_millis(300)).await;

        let status = service.status();
        assert_eq!(status.state, AiState::Ready);
        assert!(!status.managed, "un servizio remoto non è gestito dal tool");
        assert_eq!(status.base_url, format!("http://127.0.0.1:{port}"));

        let reply = service
            .chat(request(vec![message("user", "ciao")]))
            .await
            .expect("risposta dal remoto");
        assert_eq!(reply.content, "dal server di casa");

        watcher.abort();
        server.abort();
    }

    #[tokio::test]
    async fn modalita_remota_senza_risposta_lo_dice_e_riprova() {
        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_enabled = true;
            c.ai_mode = "remote".into();
            c.ai_remote_url = Some("127.0.0.1:1".into());
        });
        let service = Arc::new(AiService::new(config.clone(), EventBus::new()));
        let outcome = run_once(&service, &config.get()).await;

        assert!(matches!(outcome, Outcome::Crashed), "deve riprovare col backoff");
        let status = service.status();
        assert_eq!(status.state, AiState::Failed);
        let message = status.message.unwrap_or_default();
        assert!(message.contains("127.0.0.1:1"), "deve dire quale indirizzo: {message}");
        assert!(message.contains("127.0.0.1"), "{message}");
    }

    #[tokio::test]
    async fn modalita_remota_senza_indirizzo_non_avvia_niente_in_locale() {
        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_enabled = true;
            c.ai_mode = "remote".into();
            c.ai_remote_url = None;
        });
        let service = Arc::new(AiService::new(config.clone(), EventBus::new()));
        let outcome = run_once(&service, &config.get()).await;

        assert!(matches!(outcome, Outcome::Idle));
        assert_eq!(service.status().state, AiState::Failed);
        assert!(service.status().managed == false);
        let message = service.status().message.unwrap_or_default();
        assert!(message.contains("indica l'indirizzo"), "messaggio poco chiaro: {message}");
        assert!(!message.contains("non valido"), "non è un indirizzo sbagliato: {message}");
    }

    #[tokio::test]
    async fn choose_port_adotta_chi_risponde_gia() {
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async { axum::Json(json!({})) })).await;
        let service = test_service();
        match choose_port(&service, port).await {
            PortChoice::Adopt(adopted) => assert_eq!(adopted, port),
            _ => panic!("doveva adottare l'istanza su {port}"),
        }
        server.abort();
    }

    #[tokio::test]
    async fn choose_port_salta_le_porte_occupate() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = listener.local_addr().unwrap().port();
        let service = test_service();
        match choose_port(&service, busy).await {
            PortChoice::Spawn(port) => assert!(port > busy, "doveva scegliere una porta successiva"),
            PortChoice::Adopt(_) => panic!("non doveva adottare un servizio estraneo"),
            PortChoice::None => panic!("nessuna porta libera nel range"),
        }
        drop(listener);
    }

    #[tokio::test]
    #[ignore = "contract test: avvia davvero `of-free serve` (richiede of-free nel PATH)"]
    async fn supervisore_avvia_e_spegne_of_free_reale() {
        if resolve_command(&AppConfig::default()).await.is_none() {
            eprintln!("of-free non installato: contract test saltato");
            return;
        }

        let config = ConfigHandle::in_memory();
        config.update(|c| {
            c.ai_enabled = true;
            c.ai_port = 4390;
        });
        let service = AiService::start(config.clone(), EventBus::new());

        let ready = tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                let status = service.status();
                match status.state {
                    AiState::Ready => return Ok(status),
                    AiState::NotInstalled => return Err("of-free non installato".to_string()),
                    AiState::Disabled => return Err("RickyAI disattivata in config".to_string()),
                    AiState::Failed => {
                        return Err(status.message.unwrap_or_else(|| "avvio fallito".into()))
                    }
                    _ => tokio::time::sleep(Duration::from_millis(200)).await,
                }
            }
        })
        .await
        .expect("of-free non è diventato pronto entro 90s")
        .expect("of-free non è partito");

        assert!(ready.managed, "doveva essere un processo avviato da noi");
        assert_eq!(ready.port, 4390);
        assert!(is_of_free(&service.client, &ready.base_url).await, "endpoint non riconosciuto");

        config.update(|c| c.ai_enabled = false);
        service.request_restart();
        service.shutdown();

        let spento = tokio::time::timeout(Duration::from_secs(15), async {
            while alive(&service.client, &ready.base_url, None).await {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await;
        assert!(spento.is_ok(), "of-free è rimasto in ascolto dopo lo spegnimento");
    }

    #[tokio::test]
    async fn resolve_command_ignora_un_override_inesistente() {
        let cfg = config_with(|c| c.ai_command = Some("/percorso/che/non/esiste/of-free".into()));
        assert!(resolve_command(&cfg).await.is_none());

        let file = std::env::current_exe().unwrap();
        let cfg = config_with(|c| c.ai_command = Some(file.to_string_lossy().to_string()));
        assert_eq!(resolve_command(&cfg).await, Some(file.to_string_lossy().to_string()));
    }
}
