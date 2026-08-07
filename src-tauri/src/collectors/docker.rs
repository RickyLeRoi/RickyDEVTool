use std::sync::Arc;

use crate::config::ConfigHandle;
use crate::events::now_ms;
use crate::poller::PollerRegistry;

use crate::constants::TOPIC_DOCKER_STATS;

use crate::defaults::DOCKER_STATS_INTERVAL_MS;

pub fn register(registry: &Arc<PollerRegistry>, config: &ConfigHandle) {
    let cfg = config.clone();
    registry.register(TOPIC_DOCKER_STATS, DOCKER_STATS_INTERVAL_MS, {
        Arc::new(move || {
            let cfg = cfg.clone();
            Box::pin(async move {
                let host = cfg.get().docker_host;
                let stats = crate::adapters::docker::stats(host.as_deref()).await;
                serde_json::to_value(serde_json::json!({ "ts": now_ms(), "stats": stats }))
                    .map_err(|e| e.to_string())
            })
        })
    });
}
