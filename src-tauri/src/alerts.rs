use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::config::ConfigHandle;
use crate::events::{now_ms, EventBus};

/// Regole di alert valutate sugli eventi del bus. Girano solo quando i
/// relativi poller sono attivi (cioè quando una UI è collegata): per un
/// tool desktop è il comportamento giusto, non un limite.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    pub severity: &'static str, // info | warning | critical
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    pub created_at: u64,
    pub acknowledged: bool,
}

const CPU_SUSTAINED_PCT: f64 = 90.0;
const CPU_SUSTAINED_SECS: u64 = 60;
const MEM_HIGH_PCT: f64 = 92.0;
/// Non ripetere lo stesso alert per 10 minuti.
const COOLDOWN_MS: u64 = 600_000;
/// Un certificato in scadenza resta tale per giorni: un promemoria al giorno basta.
const CERT_COOLDOWN_MS: u64 = 86_400_000;
const CERT_WARN_DAYS: i64 = 14;
const MAX_ALERTS: usize = 50;

pub struct AlertService {
    bus: EventBus,
    config: ConfigHandle,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    alerts: Vec<Alert>,
    /// chiave dedup (kind+target) -> ultimo scatto
    last_fired: HashMap<String, u64>,
    /// campioni cpu (ts, pct) nell'ultima finestra
    cpu_window: Vec<(u64, f64)>,
    /// stato precedente dei servizi per rilevare le transizioni a down
    service_states: HashMap<String, String>,
    counter: u64,
}

impl AlertService {
    pub fn start(bus: EventBus, config: ConfigHandle) -> Arc<Self> {
        let service = Arc::new(Self {
            bus: bus.clone(),
            config,
            inner: Mutex::new(Inner::default()),
        });
        let this = Arc::clone(&service);
        tokio::spawn(async move {
            let mut rx = bus.subscribe();
            loop {
                match rx.recv().await {
                    Ok(event) => this.on_event(&event.topic, &event.payload),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        service
    }

    pub fn list(&self) -> Vec<Alert> {
        self.inner.lock().expect("alerts lock").alerts.clone()
    }

    pub fn ack(&self, id: Option<&str>) {
        {
            let mut inner = self.inner.lock().expect("alerts lock");
            for alert in inner.alerts.iter_mut() {
                if id.is_none() || id == Some(alert.id.as_str()) {
                    alert.acknowledged = true;
                }
            }
            inner.alerts.retain(|a| !a.acknowledged);
        }
        self.publish();
    }

    fn on_event(&self, topic: &str, payload: &serde_json::Value) {
        match topic {
            "stats" => self.eval_stats(payload),
            "services" => self.eval_services(payload),
            "tasks" => self.eval_tasks(payload),
            _ => {}
        }
    }

    fn eval_stats(&self, payload: &serde_json::Value) {
        let Some(cpu) = payload.get("cpuTotalPct").and_then(|v| v.as_f64()) else { return };
        let mem = payload
            .get("mem")
            .and_then(|m| m.get("usedPct"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let ts = now_ms();

        let fire_cpu = {
            let mut inner = self.inner.lock().expect("alerts lock");
            inner.cpu_window.push((ts, cpu));
            inner
                .cpu_window
                .retain(|(t, _)| ts.saturating_sub(*t) <= CPU_SUSTAINED_SECS * 1000);
            let window = &inner.cpu_window;
            window.len() >= 5
                && window.iter().all(|(_, pct)| *pct > CPU_SUSTAINED_PCT)
                // la finestra deve coprire quasi tutto il periodo
                && ts.saturating_sub(window[0].0) >= CPU_SUSTAINED_SECS * 900
        };
        if fire_cpu {
            self.fire(
                "cpu-sustained",
                "cpu",
                "warning",
                "CPU sostenuta".into(),
                format!("CPU sopra il {CPU_SUSTAINED_PCT}% da oltre {CPU_SUSTAINED_SECS}s"),
            );
        }
        if mem > MEM_HIGH_PCT {
            self.fire(
                "mem-high",
                "mem",
                "warning",
                "RAM quasi esaurita".into(),
                format!("Memoria al {mem:.0}%"),
            );
        }
    }

    fn eval_services(&self, payload: &serde_json::Value) {
        let Some(statuses) = payload.get("statuses").and_then(|s| s.as_array()) else { return };
        for status in statuses {
            let (Some(id), Some(label), Some(state)) = (
                status.get("id").and_then(|v| v.as_str()),
                status.get("label").and_then(|v| v.as_str()),
                status.get("state").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let previous = {
                let mut inner = self.inner.lock().expect("alerts lock");
                inner.service_states.insert(id.to_string(), state.to_string())
            };
            // Alert solo sulla transizione verso down (non al primo check).
            if state == "down" && previous.as_deref().is_some_and(|p| p != "down") {
                self.fire(
                    "service-down",
                    id,
                    "critical",
                    format!("{label} irraggiungibile"),
                    status
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("nessun dettaglio")
                        .to_string(),
                );
            }
            // Certificato TLS scaduto o in scadenza (solo target https).
            if let Some(days) = status.get("certDaysLeft").and_then(|v| v.as_i64()) {
                if days < 0 {
                    self.fire_with_cooldown(
                        "cert-expired",
                        id,
                        "critical",
                        format!("Certificato di {label} scaduto"),
                        format!("Scaduto da {} giorni", -days),
                        CERT_COOLDOWN_MS,
                    );
                } else if days <= CERT_WARN_DAYS {
                    self.fire_with_cooldown(
                        "cert-expiring",
                        id,
                        "warning",
                        format!("Certificato di {label} in scadenza"),
                        format!("Scade tra {days} giorni"),
                        CERT_COOLDOWN_MS,
                    );
                }
            }
        }
    }

    fn eval_tasks(&self, payload: &serde_json::Value) {
        let Some(tasks) = payload.get("tasks").and_then(|t| t.as_array()) else { return };
        for task in tasks {
            let (Some(id), Some(label), Some(state)) = (
                task.get("id").and_then(|v| v.as_str()),
                task.get("label").and_then(|v| v.as_str()),
                task.get("state").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            if state == "failed" {
                let code = task.get("exitCode").and_then(|c| c.as_i64());
                self.fire(
                    "task-failed",
                    id,
                    "warning",
                    "Task fallito".into(),
                    format!("{label} (exit {})", code.map_or("?".into(), |c| c.to_string())),
                );
            }
        }
    }

    fn fire(&self, kind: &'static str, key: &str, severity: &'static str, title: String, detail: String) {
        self.fire_with_cooldown(kind, key, severity, title, detail, COOLDOWN_MS);
    }

    fn fire_with_cooldown(
        &self,
        kind: &'static str,
        key: &str,
        severity: &'static str,
        title: String,
        detail: String,
        cooldown_ms: u64,
    ) {
        let dedup_key = format!("{kind}:{key}");
        let now = now_ms();
        {
            let mut inner = self.inner.lock().expect("alerts lock");
            if let Some(last) = inner.last_fired.get(&dedup_key) {
                if now.saturating_sub(*last) < cooldown_ms {
                    return;
                }
            }
            inner.last_fired.insert(dedup_key, now);
            inner.counter += 1;
            let id = format!("a{}", inner.counter);
            inner.alerts.insert(
                0,
                Alert {
                    id,
                    severity,
                    kind,
                    title: title.clone(),
                    detail: detail.clone(),
                    created_at: now,
                    acknowledged: false,
                },
            );
            inner.alerts.truncate(MAX_ALERTS);
        }
        crate::notify::push_alert(&self.config, severity, &title, &detail);
        self.publish();
    }

    fn publish(&self) {
        self.bus.publish(
            "alerts",
            serde_json::json!({ "alerts": self.list() }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn alert_su_servizio_down_solo_in_transizione() {
        let bus = EventBus::new();
        let service = AlertService::start(bus.clone(), ConfigHandle::in_memory());

        let payload_up = serde_json::json!({ "statuses": [{ "id": "x", "label": "X", "state": "up" }] });
        let payload_down = serde_json::json!({ "statuses": [{ "id": "x", "label": "X", "state": "down", "error": "timeout" }] });

        service.on_event("services", &payload_down); // primo check già down: nessun alert
        assert!(service.list().is_empty());
        service.on_event("services", &payload_up);
        service.on_event("services", &payload_down); // transizione up -> down
        let alerts = service.list();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, "service-down");

        service.ack(Some(&alerts[0].id));
        assert!(service.list().is_empty());
    }

    #[tokio::test]
    async fn task_failed_genera_alert_con_cooldown() {
        let bus = EventBus::new();
        let service = AlertService::start(bus, ConfigHandle::in_memory());
        let payload = serde_json::json!({ "tasks": [{ "id": "t1", "label": "npm run x", "state": "failed", "exitCode": 1 }] });
        service.on_event("tasks", &payload);
        service.on_event("tasks", &payload); // dedup
        assert_eq!(service.list().len(), 1);
    }
}
