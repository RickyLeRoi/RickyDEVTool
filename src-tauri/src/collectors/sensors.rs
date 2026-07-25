use std::sync::Arc;

use crate::events::now_ms;
use crate::poller::PollerRegistry;

/// Temperature + batteria + GPU in un unico poller: cambiano lentamente, quindi
/// un intervallo di default più lungo e una sola sottoscrizione bastano. Attivo
/// solo mentre la dashboard è aperta.
pub const TOPIC: &str = "sensors";

const DEFAULT_INTERVAL_MS: u64 = 5000;

pub fn register(registry: &Arc<PollerRegistry>) {
    registry.register(
        TOPIC,
        DEFAULT_INTERVAL_MS,
        Arc::new(|| {
            Box::pin(async move {
                let snap = crate::adapters::sensors::read().await;
                let mut val = serde_json::to_value(&snap).map_err(|e| e.to_string())?;
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("ts".to_string(), now_ms().into());
                }
                Ok(val)
            })
        }),
    );
}
