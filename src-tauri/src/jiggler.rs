//! Anti-inattività. Due meccanismi indipendenti, attivi mentre l'opzione è ON:
//!
//! 1. **Schermo sempre acceso** (macOS): finché l'opzione è attiva teniamo una
//!    power assertion via `caffeinate -d -i`. Questo impedisce lo spegnimento
//!    dello schermo a prescindere dal timing e **non richiede permessi**. È il
//!    fix al problema "lo schermo si spegne dopo ~5 minuti": prima ci
//!    attivavamo a 5 min, che è anche il timeout di sleep del Mac, quindi
//!    arrivavamo troppo tardi.
//!
//! 2. **Presenza nelle chat**: dopo IDLE_THRESHOLD di inattività reale muove il
//!    mouse ogni MOVE_INTERVAL così le app di chat non ti segnano "assente".
//!    Richiede il permesso Accessibilità (macOS); se manca, l'assertion di cui
//!    sopra tiene comunque acceso lo schermo.

use std::time::{Duration, Instant};

use crate::config::ConfigHandle;
#[cfg(target_os = "macos")]
use crate::exec;

const IDLE_THRESHOLD_SECS: u64 = 300; // 5 minuti
const MOVE_INTERVAL: Duration = Duration::from_secs(180); // 3 minuti
const TICK: Duration = Duration::from_secs(10);
/// Un idle piccolo entro questo margine dal nostro nudge è causato dal nudge
/// stesso, non dall'utente: non va interpretato come "utente tornato attivo".
const NUDGE_SETTLE_SECS: u64 = 25;
const ACTIVE_IDLE_SECS: u64 = 15;
/// Spostamento del cursore (px). Alternato in direzione a ogni nudge così il
/// puntatore si muove davvero — un +1/-1 verrebbe unito dal sistema in "fermo".
const NUDGE_PX: i32 = 12;

pub fn start(config: ConfigHandle) {
    tokio::spawn(async move {
        let mut armed = false;
        let mut forward = true;
        let mut keep_awake: Option<KeepAwake> = None;
        let mut last_nudge = Instant::now()
            .checked_sub(MOVE_INTERVAL)
            .unwrap_or_else(Instant::now);

        loop {
            tokio::time::sleep(TICK).await;
            let enabled = config.get().anti_idle_enabled;

            // (1) Schermo acceso finché l'opzione è ON.
            if enabled && keep_awake.is_none() {
                keep_awake = KeepAwake::start();
            } else if !enabled {
                keep_awake = None; // Drop => rilascia l'assertion
            }

            if !enabled {
                armed = false;
                continue;
            }

            // (2) Movimento mouse per la presenza chat.
            let Some(idle) = idle_seconds() else { continue };
            let since_nudge = last_nudge.elapsed();

            if armed {
                if since_nudge.as_secs() > NUDGE_SETTLE_SECS && idle < ACTIVE_IDLE_SECS {
                    armed = false;
                    tracing::info!("anti-idle: utente attivo, movimenti sospesi");
                } else if since_nudge >= MOVE_INTERVAL {
                    nudge(forward);
                    forward = !forward;
                    last_nudge = Instant::now();
                }
            } else if idle >= IDLE_THRESHOLD_SECS {
                armed = true;
                nudge(forward);
                forward = !forward;
                last_nudge = Instant::now();
                tracing::info!(idle_secs = idle, "anti-idle: inattività rilevata, avvio movimenti");
            }
        }
    });
}

/// Mantiene lo schermo acceso finché vive. Su macOS avvolge un processo
/// `caffeinate`; su altri OS è un no-op (lì il movimento mouse basta o il
/// concetto non si applica).
struct KeepAwake {
    #[cfg(target_os = "macos")]
    child: std::process::Child,
}

impl KeepAwake {
    #[cfg(target_os = "macos")]
    fn start() -> Option<Self> {
        // -d: no display sleep, -i: no idle sleep, -w <pid>: esce se moriamo noi.
        match exec::sync_cmd("caffeinate")
            .args(["-d", "-i", "-w", &std::process::id().to_string()])
            .spawn()
        {
            Ok(child) => {
                tracing::info!("anti-idle: schermo mantenuto acceso (caffeinate)");
                Some(Self { child })
            }
            Err(e) => {
                tracing::warn!(%e, "anti-idle: impossibile avviare caffeinate");
                None
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn start() -> Option<Self> {
        Some(Self {})
    }
}

#[cfg(target_os = "macos")]
impl Drop for KeepAwake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        tracing::info!("anti-idle: schermo di nuovo libero di spegnersi");
    }
}

/// Secondi dall'ultimo input reale dell'utente (tastiera/mouse), a livello di sistema.
#[cfg(target_os = "macos")]
fn idle_seconds() -> Option<u64> {
    let output = exec::sync_cmd("ioreg")
        .args(["-c", "IOHIDSystem"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Righe tipo:  "HIDIdleTime" = 12345678901   (nanosecondi)
    text.lines()
        .filter(|l| l.contains("\"HIDIdleTime\""))
        .filter_map(|l| l.rsplit('=').next()?.trim().parse::<u64>().ok())
        .map(|ns| ns / 1_000_000_000)
        .min()
}

#[cfg(target_os = "windows")]
fn idle_seconds() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info) == 0 {
            return None;
        }
        let now = GetTickCount();
        Some((now.wrapping_sub(info.dwTime) / 1000) as u64)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn idle_seconds() -> Option<u64> {
    None // idle non rilevabile: solo lo schermo resta gestito (dove possibile)
}

/// Sposta il cursore di NUDGE_PX nella direzione data: conta come attività di
/// input reale (resetta l'idle di sistema letto dalle chat).
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn nudge(forward: bool) {
    use enigo::{Coordinate, Enigo, Mouse, Settings};
    let dx = if forward { NUDGE_PX } else { -NUDGE_PX };
    match Enigo::new(&Settings::default()) {
        Ok(mut enigo) => match enigo.move_mouse(dx, 0, Coordinate::Rel) {
            Ok(()) => tracing::info!(dx, "anti-idle: mouse mosso"),
            Err(e) => tracing::warn!(%e, "anti-idle: move_mouse fallito"),
        },
        Err(e) => {
            tracing::warn!(%e, "anti-idle: Enigo non inizializzabile (permesso Accessibilità?)")
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn nudge(_forward: bool) {}
