use std::sync::Arc;

use crate::config::ConfigHandle;
use crate::events::now_ms;
use crate::poller::PollerRegistry;

/// Topic dedicato: il base "docker" resta libero per la lista container (REST),
/// questo poller streamma solo gli stats live. Attivo solo con la sezione aperta.
pub const TOPIC: &str = "docker:stats";

const DEFAULT_INTERVAL_MS: u64 = 3000;

pub fn register(registry: &Arc<PollerRegistry>, config: &ConfigHandle) {
    let cfg = config.clone();
    registry.register(TOPIC, DEFAULT_INTERVAL_MS, {
        Arc::new(move || {
            let cfg = cfg.clone();
            Box::pin(async move {
                // Rilegge l'host a ogni giro: se cambia (locale ↔ remoto) segue.
                let host = cfg.get().docker_host;
                let stats = crate::adapters::docker::stats(host.as_deref()).await;
                serde_json::to_value(serde_json::json!({ "ts": now_ms(), "stats": stats }))
                    .map_err(|e| e.to_string())
            })
        })
    });
}
