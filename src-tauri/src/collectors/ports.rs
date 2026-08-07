use std::sync::Arc;

use crate::adapters::ports::scan_tcp_listen;
use crate::poller::PollerRegistry;

use crate::constants::TOPIC_PORTS;
use crate::defaults::PORTS_INTERVAL_MS;

pub fn register(registry: &Arc<PollerRegistry>) {
    registry.register(
        TOPIC_PORTS,
        PORTS_INTERVAL_MS,
        Arc::new(|| {
            Box::pin(async {
                let scan = scan_tcp_listen(false).await?;
                serde_json::to_value(scan).map_err(|e| e.to_string())
            })
        }),
    );
}
