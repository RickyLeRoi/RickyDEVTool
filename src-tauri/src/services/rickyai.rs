//! RickyAI: la chat del tool, servita da `of-free` (OnFeather Free) — il router
//! che aggrega i piani gratuiti degli LLM dietro un endpoint OpenAI-compatibile.
//!
//! Due responsabilità, tenute separate:
//!
//! 1. **Il supervisore.** All'avvio del tool `of-free serve` viene acceso da
//!    solo, in ascolto **solo su 127.0.0.1**: è un endpoint senza autenticazione
//!    (`api_key: unused`), esporlo in LAN significherebbe regalare le proprie
//!    quote a chiunque sia sulla rete. Se qualcuno sta già servendo su quella
//!    porta l'istanza viene *adottata* invece di avviarne una seconda — chi
//!    lancia `of-free serve` a mano da terminale non si ritrova due router che
//!    si contendono lo stesso ledger SQLite.
//!
//! 2. **Il proxy.** La SPA non parla mai con la 4141: passa da `/api/ai/chat`.
//!    Così la chat funziona anche dal telefono (che la 4141 del desktop non la
//!    vede), resta dentro il modello di permessi del tool, e la porta di of-free
//!    non ha bisogno di uscire da localhost.
//!
//! Streaming: `of-free` non lo supporta ancora e rifiuta `stream: true` con un
//! 400. [`build_payload`] forza sempre `stream: false` — la risposta arriva
//! intera, la UI mostra un indicatore di attesa.

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

/// Porta di default di `of-free serve`.
pub const DEFAULT_PORT: u16 = 4141;

/// Strategie di routing accettate da `of-free`.
pub const STRATEGIES: &[&str] = &["balanced", "fast", "local"];

/// Quante porte provare dopo quella configurata prima di arrendersi.
const PORT_FALLBACK_RANGE: u16 = 10;

/// Le probe sono su localhost: se non risponde in fretta non risponde affatto.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Quanto attendere che il processo appena avviato risponda. Il primo avvio
/// interroga Ollama e costruisce il ledger: parecchio più lento dei successivi.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// Tetto di una singola risposta: un modello gratuito lento su un prompt lungo
/// può metterci parecchio, ma non all'infinito.
const CHAT_TIMEOUT: Duration = Duration::from_secs(180);

/// Righe di output di `of-free` conservate per la diagnostica in UI.
const MAX_LOG_LINES: usize = 60;

/// Tetti sulla conversazione inoltrata: una chat lunga è legittima, un client
/// che manda dieci megabyte no.
const MAX_MESSAGES: usize = 400;
const MAX_CHARS: usize = 400_000;

/// Ogni quanto ricontrollare un'istanza adottata (non è figlia nostra: l'unico
/// modo di sapere che è morta è chiederglielo).
const ADOPTED_POLL: Duration = Duration::from_secs(5);

/// Ogni quanto riprovare quando `of-free` non è installato: l'utente può
/// installarlo a tool acceso e non deve riavviare niente.
const MISSING_RETRY: Duration = Duration::from_secs(300);

// ---------- stato osservabile ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AiState {
    /// Avvio automatico spento dalle impostazioni.
    Disabled,
    /// Binario `of-free` non trovato sulla macchina.
    NotInstalled,
    /// Processo avviato, endpoint non ancora in risposta.
    Starting,
    /// Endpoint raggiungibile.
    Ready,
    /// Ultimo tentativo fallito (uscita anomala, porta occupata, timeout).
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub state: AiState,
    /// Porta effettiva (può differire da quella configurata: fallback).
    pub port: u16,
    /// Base OpenAI-compatibile, esposta perché sia incollabile in altri client.
    pub base_url: String,
    /// `false` quando l'istanza era già in ascolto ed è stata adottata: in quel
    /// caso il tool non la spegne e non la riavvia.
    pub managed: bool,
    /// Path del binario risolto (o l'override configurato).
    pub command: Option<String>,
    /// Perché lo stato è quello che è: mostrato in UI così com'è.
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
            message: None,
            started_at: None,
            restarts: 0,
        }
    }
}

/// `http://127.0.0.1:{port}` — la radice; gli endpoint OpenAI stanno sotto `/v1`.
pub fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

// ---------- richiesta / risposta di chat ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    /// `auto` (default), `private` per restare in locale, o `provider/modello`.
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
    /// Chi ha davvero servito la richiesta (header `X-OnFeather-Provider`).
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Quanti provider hanno rifiutato prima di questo.
    pub failovers: Option<u32>,
    /// Il modello a cui la conversazione era agganciata, se è cambiato in corsa.
    pub repinned: Option<String>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
    pub elapsed_ms: u64,
}

/// Errore già tradotto nel vocabolario del tool: `code` finisce nel campo
/// omonimo della risposta REST, `status` è lo status HTTP da restituire.
#[derive(Debug, Clone, PartialEq)]
pub struct AiError {
    pub code: &'static str,
    pub message: String,
    /// Secondi da attendere prima di riprovare (solo su quota esaurita).
    pub retry_after: Option<u64>,
    pub status: u16,
}

impl AiError {
    fn new(code: &'static str, status: u16, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), retry_after: None, status }
    }
}

// ---------- servizio ----------

struct Inner {
    status: AiStatus,
    /// Ultime righe di stdout/stderr del processo: quando `of-free` non parte,
    /// il motivo è lì dentro e senza questo buffer resterebbe invisibile.
    log: Vec<String>,
    child: Option<ChildRef>,
}

/// Identificatori per terminare il figlio anche da contesto sincrono (uscita
/// dell'app), dove non si può attendere `Child::kill`.
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
    /// Sveglia il supervisore: richiesta di riavvio o cambio di configurazione.
    restart: Notify,
    client: reqwest::Client,
}

/// Riferimento globale al servizio, per lo spegnimento all'uscita dell'app —
/// che avviene in `RunEvent::Exit`, dove non c'è nessuno stato a portata di mano.
static INSTANCE: OnceLock<Arc<AiService>> = OnceLock::new();

impl AiService {
    /// Costruisce il servizio senza avviare il supervisore (usato dai test).
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
            // `no_proxy`: un HTTP_PROXY di sistema non deve intercettare le
            // chiamate a 127.0.0.1 (succede, e l'errore che ne esce non aiuta).
            client: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("client http"),
        }
    }

    /// Avvia il servizio e il supervisore. Richiede un runtime tokio attivo.
    pub fn start(config: ConfigHandle, bus: EventBus) -> Arc<Self> {
        let service = Arc::new(Self::new(config, bus));
        let _ = INSTANCE.set(service.clone());
        tokio::spawn(supervise(service.clone()));
        service
    }

    pub fn status(&self) -> AiStatus {
        self.inner.lock().expect("ai lock").status.clone()
    }

    /// Stato + configurazione + ultime righe di log, nella forma servita dalla
    /// REST e pubblicata sul topic WS `ai`.
    pub fn snapshot(&self) -> Value {
        let cfg = self.config.get();
        let inner = self.inner.lock().expect("ai lock");
        snapshot_of(&inner.status, &inner.log, &cfg)
    }

    /// Snapshot arricchito con quote e modelli letti da `of-free`. Best effort:
    /// se l'endpoint non risponde restano i campi locali, senza far fallire la
    /// pagina che li chiede.
    pub async fn detailed_snapshot(&self) -> Value {
        let mut snapshot = self.snapshot();
        let status = self.status();
        if status.state != AiState::Ready {
            return snapshot;
        }
        let base = base_url(status.port);
        if let Some(quota) = self.get_json(&format!("{base}/v1/status")).await {
            if let Some(providers) = quota.get("providers") {
                snapshot["providers"] = providers.clone();
            }
            if let Some(next) = quota.get("next") {
                snapshot["next"] = next.clone();
            }
        }
        if let Some(models) = self.get_json(&format!("{base}/v1/models")).await {
            snapshot["models"] = json!(model_ids(&models));
        }
        snapshot
    }

    async fn get_json(&self, url: &str) -> Option<Value> {
        let response = self.client.get(url).timeout(PROBE_TIMEOUT).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.json::<Value>().await.ok()
    }

    /// Chiede al supervisore di ripartire (dopo un cambio di configurazione o su
    /// richiesta esplicita dell'utente).
    pub fn request_restart(&self) {
        self.restart.notify_one();
    }

    /// Inoltra una conversazione a `of-free` e ne restituisce la risposta.
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatReply, AiError> {
        validate(&request).map_err(|m| AiError::new("AI_INVALID", 400, m))?;
        let status = self.status();
        if status.state != AiState::Ready {
            return Err(AiError::new("AI_NOT_READY", 503, not_ready_message(&status)));
        }
        let payload = build_payload(&request, &self.config.get().ai_system_prompt);
        complete(&self.client, &base_url(status.port), &payload).await
    }

    // -- transizioni di stato ------------------------------------------------

    fn update(&self, f: impl FnOnce(&mut AiStatus)) {
        let snapshot = {
            let cfg = self.config.get();
            let mut inner = self.inner.lock().expect("ai lock");
            f(&mut inner.status);
            inner.status.base_url = base_url(inner.status.port);
            snapshot_of(&inner.status, &inner.log, &cfg)
        };
        // Pubblicato fuori dal lock: il bus ha i suoi subscriber, tenerlo dentro
        // significherebbe tenere il lock per tutta la consegna.
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

    /// Ultime righe del processo, usate per spiegare un avvio fallito.
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

    /// Termina il processo figlio, se ce n'è uno nostro. Sincrona di proposito:
    /// la chiama anche l'uscita dell'applicazione, fuori dal runtime async.
    pub fn shutdown(&self) {
        let child = { self.inner.lock().expect("ai lock").child.take() };
        let Some(child) = child else { return };
        #[cfg(unix)]
        {
            // Il figlio ha un process group suo (vedi `spawn_serve`): il segnale
            // va all'albero, non al solo processo python di primo livello.
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

/// Spegne il processo di RickyAI all'uscita dell'app.
pub fn shutdown_all() {
    if let Some(service) = INSTANCE.get() {
        service.shutdown();
    }
}

// ---------- supervisore ----------

enum Outcome {
    /// Riavvio richiesto: si riparte subito, senza backoff.
    Restart,
    /// Niente da fare finché la configurazione non cambia (o scade l'attesa).
    Idle,
    /// Il processo è caduto: si riprova con backoff crescente.
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
                // `of-free` assente o nessuna porta libera: riprova ogni tanto,
                // e subito se l'utente cambia qualcosa nelle impostazioni.
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

/// Un giro completo: adozione o avvio, attesa della salute, sorveglianza.
async fn run_once(service: &Arc<AiService>, cfg: &AppConfig) -> Outcome {
    match choose_port(service, configured_port(cfg)).await {
        PortChoice::Adopt(port) => {
            tracing::info!(port, "of-free già in ascolto: istanza adottata");
            service.update(|s| {
                s.state = AiState::Ready;
                s.port = port;
                s.managed = false;
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
        Ok(()) => service.update(|s| {
            s.state = AiState::Ready;
            s.message = None;
            s.started_at = Some(now_ms());
        }),
        Err(message) => {
            kill(&mut child).await;
            service.forget_child();
            service.set_state(AiState::Failed, Some(message));
            return Outcome::Crashed;
        }
    }

    // In esecuzione: si esce di qui solo se il processo muore o se arriva una
    // richiesta di riavvio.
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

/// Attende che l'endpoint risponda, sorvegliando nel frattempo il processo: un
/// `of-free` che muore dopo due secondi non deve far aspettare il timeout pieno.
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
        if identify(&service.client, port).await {
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

/// Sorveglia un'istanza non nostra: l'unico modo di accorgersi che è sparita è
/// interrogarla.
async fn watch_adopted(service: &Arc<AiService>, port: u16) -> Outcome {
    loop {
        tokio::select! {
            _ = service.restart.notified() => return Outcome::Restart,
            _ = tokio::time::sleep(ADOPTED_POLL) => {}
        }
        if !alive(&service.client, port).await {
            service.set_state(
                AiState::Failed,
                Some("l'istanza esterna di of-free non risponde più".into()),
            );
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

// ---------- porta, binario, argomenti ----------

enum PortChoice {
    /// Un `of-free` risponde già qui: si adotta.
    Adopt(u16),
    /// Porta libera su cui avviare il nostro.
    Spawn(u16),
    None,
}

async fn choose_port(service: &Arc<AiService>, base: u16) -> PortChoice {
    for candidate in base..base.saturating_add(PORT_FALLBACK_RANGE) {
        if identify(&service.client, candidate).await {
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

/// C'è **of-free** dall'altra parte? Un 200 su `/health` non basta: qualsiasi
/// servizio può rispondere così, e adottarne uno sbagliato manderebbe le chat
/// dentro un tritacarne. `auto` è il modello virtuale che solo of-free espone.
async fn identify(client: &reqwest::Client, port: u16) -> bool {
    let url = format!("{}/v1/models", base_url(port));
    let Ok(response) = client.get(&url).timeout(PROBE_TIMEOUT).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let Ok(body) = response.json::<Value>().await else {
        return false;
    };
    model_ids(&body).iter().any(|id| id == "auto")
}

/// Liveness di un endpoint già identificato: basta `/health`.
async fn alive(client: &reqwest::Client, port: u16) -> bool {
    let url = format!("{}/health", base_url(port));
    match client.get(&url).timeout(PROBE_TIMEOUT).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
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

/// Path del binario `of-free`: override configurato (se esiste davvero) o
/// risoluzione nel PATH.
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

/// Riga di comando di `of-free serve`.
///
/// `--env` e `--ledger` sono opzioni **globali**: argparse le accetta solo prima
/// del sottocomando, metterle dopo `serve` fa fallire l'avvio.
pub fn serve_args(cfg: &AppConfig, port: u16) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(env) = env_file(cfg) {
        args.push("--env".to_string());
        args.push(env);
    }
    args.push("serve".to_string());
    args.push("--host".to_string());
    // Mai 0.0.0.0: l'endpoint non ha autenticazione. In LAN ci si arriva dal
    // proxy del tool, che ha pairing e permessi.
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

fn env_file(cfg: &AppConfig) -> Option<String> {
    cfg.ai_env_file
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn spawn_serve(
    program: &str,
    cfg: &AppConfig,
    port: u16,
) -> Result<tokio::process::Child, std::io::Error> {
    let mut command = exec::cmd(program);
    command
        .args(serve_args(cfg, port))
        .current_dir(working_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Process group proprio: permette di terminare l'albero (python + eventuali
    // figli) con un solo segnale, come fa il task runner.
    #[cfg(unix)]
    command.process_group(0);
    command.spawn()
}

/// Cartella di lavoro del processo. **Non** la home: `of-free` cerca le chiavi
/// nel primo `.env` che trova partendo da `Path.cwd()/.env`, e un `~/.env` di
/// tutt'altro contenuto interromperebbe la ricerca prima di `~/.onfeather/.env`.
/// La data dir del tool non contiene `.env`, quindi la catena resta quella
/// prevista da of-free.
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
        "enabled": cfg.ai_enabled,
        "configuredPort": configured_port(cfg),
        "strategy": strategy(cfg),
        "envFile": cfg.ai_env_file,
        "systemPrompt": cfg.ai_system_prompt,
        "providers": Value::Null,
        "next": Value::Null,
        "models": Vec::<String>::new(),
    })
}

// ---------- client OpenAI-compatibile ----------

/// Controlli sulla conversazione prima di spedirla. Sono tetti, non gusto: una
/// chat lunga è normale, un client che manda mezzo gigabyte no.
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

/// Corpo della richiesta OpenAI. `stream` è sempre `false` ed è esplicito: di
/// default of-free non streamma comunque, ma un `true` che scivolasse dentro
/// verrebbe rifiutato con un 400 poco leggibile.
pub fn build_payload(request: &ChatRequest, system_prompt: &str) -> Value {
    let mut messages: Vec<Value> = Vec::with_capacity(request.messages.len() + 1);
    let system = system_prompt.trim();
    // Il prompt di sistema configurato vale solo se la conversazione non ne
    // porta già uno suo: due system message in testa si contraddicono.
    if !system.is_empty() && request.messages.first().map(|m| m.role.as_str()) != Some("system") {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &request.messages {
        messages.push(json!({ "role": message.role, "content": message.content }));
    }

    let model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or("auto");

    let mut payload = json!({
        "model": model,
        "messages": messages,
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

/// POST su `/v1/chat/completions`, con gli errori tradotti nel vocabolario del
/// tool: un 429 non è un guasto ma una quota da aspettare, e la UI deve poterli
/// distinguere.
pub async fn complete(
    client: &reqwest::Client,
    base: &str,
    payload: &Value,
) -> Result<ChatReply, AiError> {
    let started = Instant::now();
    let response = client
        .post(format!("{base}/v1/chat/completions"))
        .json(payload)
        .timeout(CHAT_TIMEOUT)
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
            // of-free è in piedi ma non ha *niente* su cui instradare: quasi
            // sempre nessuna chiave configurata. Il suo "no provider available
            // for this request" è esatto e inutile — chi legge non sa che deve
            // scrivere un file di chiavi.
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
        // Gli header sono la verità su chi ha servito; il campo `model` del body
        // è il ripiego per un client che li perdesse per strada (proxy, cache).
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

/// Testo della risposta. `content` è normalmente una stringa, ma alcuni
/// provider (e i loro adattatori) restituiscono la forma a blocchi
/// `[{type:"text", text:"…"}]`: entrambe vanno lette, o la chat resta muta.
pub fn extract_content(body: &Value) -> Option<String> {
    let content = body.pointer("/choices/0/message/content")?;
    let text = match content {
        Value::String(s) => s.clone(),
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

    // -- riga di comando ----------------------------------------------------

    #[test]
    fn serve_args_tiene_of_free_su_localhost() {
        let args = serve_args(&AppConfig::default(), 4141);
        let host = args.iter().position(|a| a == "--host").expect("--host presente");
        // L'endpoint non ha autenticazione: se un giorno qualcuno mettesse
        // 0.0.0.0 qui, le quote dell'utente sarebbero di chiunque sia in LAN.
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
    fn env_file_precede_il_sottocomando() {
        // `--env` è un'opzione globale di of-free: dopo `serve` argparse la
        // rifiuta e il processo non parte affatto.
        let cfg = config_with(|c| c.ai_env_file = Some("/tmp/keys.env".into()));
        let args = serve_args(&cfg, 4141);
        let env = args.iter().position(|a| a == "--env").expect("--env presente");
        let serve = args.iter().position(|a| a == "serve").expect("sottocomando");
        assert!(env < serve, "--env deve stare prima di `serve`: {args:?}");
        assert_eq!(args[env + 1], "/tmp/keys.env");
    }

    #[test]
    fn env_file_vuoto_non_viene_passato() {
        let cfg = config_with(|c| c.ai_env_file = Some("   ".into()));
        assert!(!serve_args(&cfg, 4141).iter().any(|a| a == "--env"));
        assert!(!serve_args(&AppConfig::default(), 4141).iter().any(|a| a == "--env"));
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
        // of-free si ferma al primo `.env` trovato: se la cwd ne avesse uno,
        // `~/.onfeather/.env` non verrebbe mai letto e le chiavi sparirebbero.
        assert!(!working_dir().join(".env").exists());
    }

    // -- validazione --------------------------------------------------------

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
        // Ruolo inventato: non deve arrivare al provider.
        assert!(validate(&request(vec![message("tool", "x"), message("user", "ciao")])).is_err());
        // L'ultimo messaggio deve essere dell'utente, o si chiede al modello di
        // rispondere a se stesso.
        assert!(validate(&request(vec![message("user", "ciao"), message("assistant", "ciao")])).is_err());
        // Messaggio di soli spazi.
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

    // -- payload ------------------------------------------------------------

    #[test]
    fn build_payload_non_chiede_mai_lo_streaming() {
        // of-free rifiuta `stream: true` con un 400: qui non deve poterci finire.
        let payload = build_payload(&request(vec![message("user", "ciao")]), "");
        assert_eq!(payload["stream"], json!(false));
        assert_eq!(payload["model"], json!("auto"));
    }

    #[test]
    fn build_payload_antepone_il_prompt_di_sistema() {
        let payload = build_payload(&request(vec![message("user", "ciao")]), "sei RickyAI");
        assert_eq!(payload["messages"][0]["role"], json!("system"));
        assert_eq!(payload["messages"][0]["content"], json!("sei RickyAI"));
        assert_eq!(payload["messages"][1]["role"], json!("user"));
    }

    #[test]
    fn build_payload_non_duplica_il_system() {
        // La conversazione ha già il suo system: aggiungerne un altro davanti
        // significa dare al modello due istruzioni che si contraddicono.
        let req = request(vec![message("system", "sei un pirata"), message("user", "ciao")]);
        let payload = build_payload(&req, "sei RickyAI");
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
        let payload = build_payload(&req, "");
        assert_eq!(payload["model"], json!("private"));
        // Valori fuori scala vengono riportati nel range invece di essere spediti.
        assert_eq!(payload["temperature"], json!(2.0));
        assert_eq!(payload["max_tokens"], json!(1));
    }

    // -- lettura della risposta ---------------------------------------------

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

        // `content: null` è la forma che of-free usa per "nessun testo": non
        // deve diventare una risposta vuota che sembra una risposta.
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

    // -- proxy contro un of-free finto --------------------------------------

    /// Server minimo che imita of-free: risponde su `/v1/models`, `/health` e
    /// `/v1/chat/completions` secondo il copione passato.
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

        let reply = complete(&test_client(), &base_url(port), &json!({}))
            .await
            .expect("risposta");
        assert_eq!(reply.content, "ciao!");
        // La provenienza arriva dagli header: è l'unico modo di sapere chi ha
        // davvero servito, e la UI la mostra sotto la risposta.
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

        let reply = complete(&test_client(), &base_url(port), &json!({})).await.unwrap();
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

        let error = complete(&test_client(), &base_url(port), &json!({}))
            .await
            .expect_err("429 è un errore");
        // 429 non è un guasto: la UI deve poter dire "riprova tra 42 secondi"
        // invece di mostrare un errore generico.
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

        let error = complete(&test_client(), &base_url(port), &json!({})).await.unwrap_err();
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

        let error = complete(&test_client(), &base_url(port), &json!({})).await.unwrap_err();
        assert_eq!(error.code, "AI_EMPTY");
        server.abort();
    }

    #[tokio::test]
    async fn endpoint_spento_non_e_un_errore_di_protocollo() {
        // Porta chiusa: deve uscirne "non risponde", non un panic né un timeout
        // lungo quanto CHAT_TIMEOUT.
        let error = complete(&test_client(), &base_url(1), &json!({})).await.unwrap_err();
        assert_eq!(error.code, "AI_UNREACHABLE");
        assert_eq!(error.status, 502);
    }

    // -- identificazione dell'endpoint --------------------------------------

    #[tokio::test]
    async fn identify_riconosce_of_free() {
        use axum::routing::post;
        let (port, server) = fake_of_free(post(|| async { axum::Json(json!({})) })).await;
        assert!(identify(&test_client(), port).await);
        assert!(alive(&test_client(), port).await);
        server.abort();
    }

    #[tokio::test]
    async fn identify_non_adotta_un_servizio_qualsiasi() {
        use axum::routing::get;
        // Un server che risponde 200 ovunque ma non è of-free: adottarlo
        // significherebbe mandargli le chat dell'utente.
        let app = axum::Router::new().fallback(get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        assert!(!identify(&test_client(), port).await);
        server.abort();
    }

    // -- servizio -----------------------------------------------------------

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
        // Anche a servizio pronto, una richiesta malformata non deve partire:
        // spenderebbe quota per farsi rifiutare dal provider.
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
        // Restano le ultime: quando l'avvio fallisce, il motivo è in fondo.
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
        // Porta occupata da un servizio che non è of-free: niente adozione, si
        // avvia il proprio processo sulla successiva libera.
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

    /// Il giro completo contro `of-free` vero: risoluzione del binario, avvio,
    /// endpoint che risponde, e — la parte che nessun test con un finto può
    /// coprire — il processo che muore davvero quando il tool lo spegne.
    /// Contract test come gli altri: gira solo con `--ignored`, perché richiede
    /// of-free installato sulla macchina.
    #[tokio::test]
    #[ignore = "contract test: avvia davvero `of-free serve` (richiede of-free nel PATH)"]
    async fn supervisore_avvia_e_spegne_of_free_reale() {
        // `of-free` è una dipendenza facoltativa e in CI non c'è: senza, questo
        // test non ha niente da verificare e non deve far fallire la matrice.
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
        assert!(identify(&service.client, ready.port).await, "endpoint non riconosciuto");

        // Spegnimento: prima si disattiva, o il supervisore lo rimetterebbe su
        // (che è esattamente ciò che deve fare quando cade da solo).
        config.update(|c| c.ai_enabled = false);
        service.request_restart();
        service.shutdown();

        let spento = tokio::time::timeout(Duration::from_secs(15), async {
            while alive(&service.client, ready.port).await {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await;
        assert!(spento.is_ok(), "of-free è rimasto in ascolto dopo lo spegnimento");
    }

    #[tokio::test]
    async fn resolve_command_ignora_un_override_inesistente() {
        let cfg = config_with(|c| c.ai_command = Some("/percorso/che/non/esiste/of-free".into()));
        // Meglio ricadere su "non installato" che provare a lanciare un path
        // sbagliato a ogni giro del supervisore.
        assert!(resolve_command(&cfg).await.is_none());

        let file = std::env::current_exe().unwrap();
        let cfg = config_with(|c| c.ai_command = Some(file.to_string_lossy().to_string()));
        assert_eq!(resolve_command(&cfg).await, Some(file.to_string_lossy().to_string()));
    }
}
