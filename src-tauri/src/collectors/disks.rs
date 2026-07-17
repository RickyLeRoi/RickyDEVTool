use std::sync::Arc;

use crate::adapters::disks;
use crate::poller::PollerRegistry;

pub const TOPIC: &str = "disks";
const DEFAULT_INTERVAL_MS: u64 = 10_000;

pub fn register(registry: &Arc<PollerRegistry>) {
    registry.register(
        TOPIC,
        DEFAULT_INTERVAL_MS,
        Arc::new(|| {
            Box::pin(async {
                // La lista è veloce ma tocca il filesystem: fuori dal runtime async.
                let disks = tokio::task::spawn_blocking(disks::list)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({ "disks": disks }))
            })
        }),
    );
}
