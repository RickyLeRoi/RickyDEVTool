use std::sync::Arc;

use crate::events::now_ms;
use crate::poller::PollerRegistry;

use crate::constants::TOPIC_SENSORS;

use crate::defaults::SENSORS_INTERVAL_MS;

pub fn register(registry: &Arc<PollerRegistry>) {
    registry.register(
        TOPIC_SENSORS,
        SENSORS_INTERVAL_MS,
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
