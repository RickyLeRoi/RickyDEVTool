use std::sync::Arc;

use crate::adapters::ports::scan_tcp_listen;
use crate::poller::PollerRegistry;

pub const TOPIC: &str = "ports";
const DEFAULT_INTERVAL_MS: u64 = 3000;

pub fn register(registry: &Arc<PollerRegistry>) {
    registry.register(
        TOPIC,
        DEFAULT_INTERVAL_MS,
        Arc::new(|| {
            Box::pin(async {
                let scan = scan_tcp_listen(false).await?;
                serde_json::to_value(scan).map_err(|e| e.to_string())
            })
        }),
    );
}
