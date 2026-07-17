pub mod disks;
pub mod ports;
pub mod services;
pub mod stats;

use std::sync::Arc;

use crate::config::ConfigHandle;
use crate::poller::PollerRegistry;

pub fn register_all(registry: &Arc<PollerRegistry>, config: &ConfigHandle) {
    stats::register(registry, config);
    ports::register(registry);
    services::register(registry, config);
    disks::register(registry);
}
