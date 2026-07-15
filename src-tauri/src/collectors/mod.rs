pub mod stats;

use std::sync::Arc;

use crate::config::ConfigHandle;
use crate::poller::PollerRegistry;

pub fn register_all(registry: &Arc<PollerRegistry>, config: &ConfigHandle) {
    stats::register(registry, config);
}
