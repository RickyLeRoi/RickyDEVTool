//! Cache aggiornata in background per il menu del tray.
//!
//! Il click sull'icona nel tray deve aprire il menu istantaneamente: uno scan
//! porte o un check servizi sincrono a quel punto (i servizi da soli possono
//! costare fino a qualche secondo di timeout) lo renderebbe percepibilmente
//! lento. Invece si tiene una foto sempre pronta, rinfrescata da un loop
//! indipendente dal `PollerRegistry` (quello si avvia/ferma in base ai
//! subscriber WS della UI; il tray è sempre presente, quindi ha bisogno di un
//! proprio ciclo, sempre attivo mentre l'app gira).

use std::sync::{Arc, RwLock};
use std::time::Duration;

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

use crate::config::ConfigHandle;

const TICK: Duration = Duration::from_secs(3);
/// Servizi online: check più costoso (fino a qualche secondo con timeout),
/// rinfrescato meno spesso. ~21s.
const SERVICES_EVERY_TICKS: u32 = 7;
/// Strumenti rilevati: cambia raramente (installazioni), refresh ogni ~60s.
const TOOLS_EVERY_TICKS: u32 = 20;

#[derive(Default, Clone)]
pub struct TraySnapshot {
    pub cpu_pct: f32,
    pub ram_pct: f32,
    pub disks: Vec<crate::adapters::disks::DiskInfo>,
    pub ports: Vec<crate::adapters::ports::PortEntry>,
    pub services: Vec<crate::services::online::ServiceStatus>,
    pub tools: Vec<crate::adapters::tools::DiscoveredTool>,
}

pub type SharedSnapshot = Arc<RwLock<TraySnapshot>>;

/// Avvia il refresh periodico e ritorna l'handle condiviso: sempre letto,
/// mai popolato sincronamente al click sul tray.
///
/// Chiamata da `tray::setup`, che gira nella closure `.setup()` di Tauri:
/// quel contesto NON è dentro un runtime Tokio (a differenza di
/// `server::start`, invocato via `block_on`), quindi serve
/// `tauri::async_runtime::spawn` — un `tokio::spawn` qui andrebbe in panico
/// ("no reactor running").
pub fn start(config: ConfigHandle) -> SharedSnapshot {
    let snapshot: SharedSnapshot = Arc::new(RwLock::new(TraySnapshot::default()));
    let shared = snapshot.clone();

    tauri::async_runtime::spawn(async move {
        let refresh = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(MemoryRefreshKind::everything());
        let mut sys = System::new_with_specifics(refresh);
        let mut tick: u32 = 0;

        loop {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let cpu_pct = sys.global_cpu_usage();
            let total = sys.total_memory();
            let ram_pct = if total > 0 {
                sys.used_memory() as f32 / total as f32 * 100.0
            } else {
                0.0
            };

            let disks = tokio::task::spawn_blocking(crate::adapters::disks::list)
                .await
                .unwrap_or_default();
            let ports = crate::adapters::ports::scan_tcp_listen(false)
                .await
                .map(|scan| scan.ports)
                .unwrap_or_default();

            {
                let mut guard = shared.write().expect("tray snapshot lock");
                guard.cpu_pct = cpu_pct;
                guard.ram_pct = ram_pct;
                guard.disks = disks;
                guard.ports = ports;
            }

            if tick % SERVICES_EVERY_TICKS == 0 {
                let defs = config.get().services;
                let services = crate::services::online::check_all(&defs).await;
                shared.write().expect("tray snapshot lock").services = services;
            }
            if tick % TOOLS_EVERY_TICKS == 0 {
                let overrides = config.get().tool_paths;
                let tools = crate::adapters::tools::discover_all(&overrides).await;
                shared.write().expect("tray snapshot lock").tools = tools;
            }

            tick = tick.wrapping_add(1);
            tokio::time::sleep(TICK).await;
        }
    });

    snapshot
}
