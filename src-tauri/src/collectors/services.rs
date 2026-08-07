use std::sync::Arc;

use crate::config::ConfigHandle;
use crate::poller::PollerRegistry;
use crate::services::online::check_all;

use crate::constants::TOPIC_SERVICES;
use crate::defaults::SERVICES_INTERVAL_MS;

pub fn register(registry: &Arc<PollerRegistry>, config: &ConfigHandle) {
    let cfg = config.clone();
    registry.register(
        TOPIC_SERVICES,
        SERVICES_INTERVAL_MS,
        Arc::new(move || {
            let cfg = cfg.clone();
            Box::pin(async move {
                let defs = cfg.get().services;
                let statuses = check_all(&defs).await;
                Ok(serde_json::json!({ "statuses": statuses }))
            })
        }),
    );
}
