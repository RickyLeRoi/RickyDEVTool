use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDef {
    pub id: String,
    pub label: String,
    pub kind: ServiceKind,
    pub target: String,
    #[serde(default)]
    pub expect_status: Option<Vec<u16>>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_timeout() -> u64 {
    4000
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceKind {
    Http,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    Up,
    Degraded,
    Down,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub id: String,
    pub label: String,
    pub state: ServiceState,
    pub latency_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub error: Option<String>,
    pub checked_at: u64,
    pub history: Vec<ServiceState>,
    pub cert_expires_at: Option<u64>,
    pub cert_days_left: Option<i64>,
}

const DEGRADED_LATENCY_MS: u64 = 2500;
const HISTORY_LEN: usize = 20;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 RickyDEVTool/0.1";

pub fn builtin_presets() -> Vec<ServiceDef> {
    let http = |id: &str, label: &str, target: &str, expect: Option<Vec<u16>>| ServiceDef {
        id: id.into(),
        label: label.into(),
        kind: ServiceKind::Http,
        target: target.into(),
        expect_status: expect,
        timeout_ms: 4000,
        builtin: true,
        enabled: true,
    };
    vec![
        http("google", "Google", "https://www.gstatic.com/generate_204", Some(vec![204])),
        http("cloudflare", "Cloudflare", "https://one.one.one.one/cdn-cgi/trace", None),
        http("whatsapp", "WhatsApp", "https://www.whatsapp.com", None),
        http("telegram", "Telegram", "https://core.telegram.org", None),
        http("netflix", "Netflix", "https://www.netflix.com", None),
        http("amazon", "Amazon", "https://www.amazon.it", None),
        http("primevideo", "Prime Video", "https://www.primevideo.com", None),
        http("icloud", "iCloud", "https://www.icloud.com", None),
    ]
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("client http")
    })
}

fn history_store() -> &'static Mutex<HashMap<String, Vec<ServiceState>>> {
    static STORE: OnceLock<Mutex<HashMap<String, Vec<ServiceState>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn check_all(defs: &[ServiceDef]) -> Vec<ServiceStatus> {
    let mut join_set = tokio::task::JoinSet::new();
    for def in defs.iter().filter(|d| d.enabled).cloned() {
        join_set.spawn(async move { check_one(def).await });
    }
    let mut results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(status) = res {
            results.push(status);
        }
    }
    results.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));

    let mut store = history_store().lock().expect("history lock");
    for status in &mut results {
        let entry = store.entry(status.id.clone()).or_default();
        entry.push(status.state);
        if entry.len() > HISTORY_LEN {
            let excess = entry.len() - HISTORY_LEN;
            entry.drain(..excess);
        }
        status.history = entry.clone();
    }
    results
}

async fn check_one(def: ServiceDef) -> ServiceStatus {
    let started = Instant::now();
    let timeout = Duration::from_millis(def.timeout_ms.clamp(500, 30_000));
    let (state, http_status, error) = match def.kind {
        ServiceKind::Http => check_http(&def, timeout).await,
        ServiceKind::Tcp => check_tcp(&def, timeout).await,
    };
    let latency = started.elapsed().as_millis() as u64;
    let state = match state {
        ServiceState::Up if latency > DEGRADED_LATENCY_MS => ServiceState::Degraded,
        s => s,
    };

    let cert_expires_at = match (&def.kind, https_host(&def.target)) {
        (ServiceKind::Http, Some((host, port))) if state != ServiceState::Down => {
            super::tlscert::cert_expiry_ms(&host, port).await
        }
        _ => None,
    };
    let cert_days_left = cert_expires_at.map(super::tlscert::days_left);
    let state = match cert_days_left {
        Some(days) if days < 0 && state == ServiceState::Up => ServiceState::Degraded,
        _ => state,
    };

    ServiceStatus {
        id: def.id,
        label: def.label,
        state,
        latency_ms: (state != ServiceState::Down).then_some(latency),
        http_status,
        error,
        checked_at: crate::events::now_ms(),
        history: Vec::new(),
        cert_expires_at,
        cert_days_left,
    }
}

fn https_host(target: &str) -> Option<(String, u16)> {
    let url = reqwest::Url::parse(target).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    Some((url.host_str()?.to_string(), url.port().unwrap_or(443)))
}

async fn check_http(
    def: &ServiceDef,
    timeout: Duration,
) -> (ServiceState, Option<u16>, Option<String>) {
    match http_client().get(&def.target).timeout(timeout).send().await {
        Ok(response) => {
            let code = response.status().as_u16();
            let ok = match &def.expect_status {
                Some(expected) => expected.contains(&code),
                None => (200..400).contains(&code),
            };
            if ok {
                (ServiceState::Up, Some(code), None)
            } else {
                (ServiceState::Degraded, Some(code), Some(format!("HTTP {code}")))
            }
        }
        Err(e) => {
            let message = if e.is_timeout() { "timeout".to_string() } else { e.to_string() };
            (ServiceState::Down, None, Some(message))
        }
    }
}

async fn check_tcp(
    def: &ServiceDef,
    timeout: Duration,
) -> (ServiceState, Option<u16>, Option<String>) {
    match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&def.target)).await {
        Ok(Ok(_)) => (ServiceState::Up, None, None),
        Ok(Err(e)) => (ServiceState::Down, None, Some(e.to_string())),
        Err(_) => (ServiceState::Down, None, Some("timeout".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tcp_check_su_listener_locale() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let def = ServiceDef {
            id: "test".into(),
            label: "Test".into(),
            kind: ServiceKind::Tcp,
            target: format!("127.0.0.1:{port}"),
            expect_status: None,
            timeout_ms: 1000,
            builtin: false,
            enabled: true,
        };
        let statuses = check_all(&[def]).await;
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, ServiceState::Up);
        assert_eq!(statuses[0].history.len(), 1);
    }

    #[tokio::test]
    async fn tcp_check_su_porta_chiusa() {
        let def = ServiceDef {
            id: "chiusa".into(),
            label: "Chiusa".into(),
            kind: ServiceKind::Tcp,
            target: "127.0.0.1:1".into(),
            expect_status: None,
            timeout_ms: 1000,
            builtin: false,
            enabled: true,
        };
        let statuses = check_all(&[def]).await;
        assert_eq!(statuses[0].state, ServiceState::Down);
        assert!(statuses[0].latency_ms.is_none());
    }

    #[test]
    fn preset_validi() {
        let presets = builtin_presets();
        assert!(presets.len() >= 8);
        assert!(presets.iter().all(|p| p.builtin && p.enabled));
        assert!(presets.iter().any(|p| p.id == "google"
            && p.expect_status.as_deref() == Some(&[204])));
    }
}
