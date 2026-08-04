use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub topic: String,
    pub ts: u64,
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    pub fn publish(&self, topic: &str, payload: serde_json::Value) {
// 20260704 RG errore in send = nessun subscriber collegato: normale, non un guasto.
        let event = Event {
            topic: topic.to_string(),
            ts: now_ms(),
            payload,
        };
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
