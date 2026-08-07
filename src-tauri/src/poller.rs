use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::constants::POLLER_MAX_BACKOFF_MS;
use crate::events::EventBus;

pub type CollectResult = Result<serde_json::Value, String>;
pub type CollectFuture = Pin<Box<dyn Future<Output = CollectResult> + Send>>;
pub type CollectFn = Arc<dyn Fn() -> CollectFuture + Send + Sync>;

struct Poller {
    interval_ms: Arc<AtomicU64>,
    subscribers: usize,
    task: Option<tokio::task::JoinHandle<()>>,
    collect: CollectFn,
}

pub struct PollerRegistry {
    bus: EventBus,
    pollers: Mutex<HashMap<String, Poller>>,
}

impl PollerRegistry {
    pub fn new(bus: EventBus) -> Self {
        Self {
            bus,
            pollers: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, topic: &str, default_interval_ms: u64, collect: CollectFn) {
        let mut pollers = self.pollers.lock().expect("poller lock");
        pollers.insert(
            topic.to_string(),
            Poller {
                interval_ms: Arc::new(AtomicU64::new(default_interval_ms.max(200))),
                subscribers: 0,
                task: None,
                collect,
            },
        );
    }

    pub fn known_topic(&self, topic: &str) -> bool {
        self.pollers.lock().expect("poller lock").contains_key(topic)
    }

    pub fn add_subscriber(&self, topic: &str) {
        let mut pollers = self.pollers.lock().expect("poller lock");
        let Some(poller) = pollers.get_mut(topic) else {
            return;
        };
        poller.subscribers += 1;
        if poller.subscribers == 1 && poller.task.is_none() {
            tracing::debug!(topic, "poller avviato");
            poller.task = Some(spawn_loop(
                topic.to_string(),
                self.bus.clone(),
                poller.interval_ms.clone(),
                poller.collect.clone(),
            ));
        }
    }

    pub fn remove_subscriber(&self, topic: &str) {
        let mut pollers = self.pollers.lock().expect("poller lock");
        let Some(poller) = pollers.get_mut(topic) else {
            return;
        };
        poller.subscribers = poller.subscribers.saturating_sub(1);
        if poller.subscribers == 0 {
            if let Some(task) = poller.task.take() {
                task.abort();
                tracing::debug!(topic, "poller fermato");
            }
        }
    }

    pub fn set_interval(&self, topic: &str, interval_ms: u64) -> bool {
        let pollers = self.pollers.lock().expect("poller lock");
        match pollers.get(topic) {
            Some(poller) => {
                poller
                    .interval_ms
                    .store(interval_ms.clamp(200, 60_000), Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}

fn spawn_loop(
    topic: String,
    bus: EventBus,
    interval_ms: Arc<AtomicU64>,
    collect: CollectFn,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff_ms: u64 = 0;
        let mut last_tick: Option<Instant> = None;
        loop {
            let interval = interval_ms.load(Ordering::Relaxed);

            let skip_publish = matches!(
                last_tick,
                Some(t) if t.elapsed().as_millis() as u64 > interval.saturating_mul(3)
            );
            last_tick = Some(Instant::now());

            match collect().await {
                Ok(payload) => {
                    backoff_ms = 0;
                    if !skip_publish {
                        bus.publish(&topic, payload);
                    }
                }
                Err(message) => {
                    backoff_ms = if backoff_ms == 0 {
                        interval
                    } else {
                        (backoff_ms * 2).min(POLLER_MAX_BACKOFF_MS)
                    };
                    tracing::warn!(topic, %message, backoff_ms, "collect fallita");
                    bus.publish(
                        &format!("{topic}:error"),
                        serde_json::json!({ "message": message }),
                    );
                }
            }

            let sleep_ms = if backoff_ms > 0 { backoff_ms } else { interval };
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        }
    })
}
