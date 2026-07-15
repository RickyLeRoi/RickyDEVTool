use std::sync::Arc;

use serde::Serialize;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tokio::sync::Mutex;

use crate::config::ConfigHandle;
use crate::events::now_ms;
use crate::poller::PollerRegistry;

pub const TOPIC: &str = "stats";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineStats {
    ts: u64,
    cpu_total_pct: f32,
    cores: Vec<CoreSample>,
    mem: MemStats,
    swap: Option<SwapStats>,
    interval_ms: u64,
}

#[derive(Serialize)]
struct CoreSample {
    core: usize,
    pct: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemStats {
    total_bytes: u64,
    used_bytes: u64,
    used_pct: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SwapStats {
    total_bytes: u64,
    used_bytes: u64,
}

pub fn register(registry: &Arc<PollerRegistry>, config: &ConfigHandle) {
    let refresh = RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
        .with_memory(MemoryRefreshKind::everything());
    let system = Arc::new(Mutex::new(System::new_with_specifics(refresh)));
    let cfg = config.clone();

    registry.register(TOPIC, config.get().stats_interval_ms, {
        let system = system.clone();
        Arc::new(move || {
            let system = system.clone();
            let interval_ms = cfg.get().stats_interval_ms;
            Box::pin(async move {
                let mut sys = system.lock().await;
                sys.refresh_cpu_usage();
                sys.refresh_memory();

                let cores: Vec<CoreSample> = sys
                    .cpus()
                    .iter()
                    .enumerate()
                    .map(|(i, cpu)| CoreSample {
                        core: i,
                        pct: cpu.cpu_usage(),
                    })
                    .collect();

                let total = sys.total_memory();
                let used = sys.used_memory();
                let swap_total = sys.total_swap();

                let stats = MachineStats {
                    ts: now_ms(),
                    cpu_total_pct: sys.global_cpu_usage(),
                    cores,
                    mem: MemStats {
                        total_bytes: total,
                        used_bytes: used,
                        used_pct: if total > 0 {
                            used as f32 / total as f32 * 100.0
                        } else {
                            0.0
                        },
                    },
                    swap: (swap_total > 0).then(|| SwapStats {
                        total_bytes: swap_total,
                        used_bytes: sys.used_swap(),
                    }),
                    interval_ms,
                };
                serde_json::to_value(stats).map_err(|e| e.to_string())
            })
        })
    });
}
