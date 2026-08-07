use std::sync::Arc;

use crate::adapters::disks;
use crate::poller::PollerRegistry;

use crate::constants::TOPIC_DISKS;
use crate::defaults::DISKS_INTERVAL_MS;

pub fn register(registry: &Arc<PollerRegistry>) {
    registry.register(
        TOPIC_DISKS,
        DISKS_INTERVAL_MS,
        Arc::new(|| {
            Box::pin(async {
                let disks = tokio::task::spawn_blocking(disks::list)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({ "disks": disks }))
            })
        }),
    );
}
