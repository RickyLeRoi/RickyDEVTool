use serde::Serialize;
use tokio::sync::broadcast;

/// Evento push verso i client WS, instradato per topic.
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
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn publish(&self, topic: &str, payload: serde_json::Value) {
        let event = Event {
            topic: topic.to_string(),
            ts: now_ms(),
            payload,
        };
        // Errore = nessun subscriber: normale, non è un problema.
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
