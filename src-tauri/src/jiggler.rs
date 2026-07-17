//! Anti-inattività: dopo IDLE_THRESHOLD di inattività reale muove il mouse ogni
//! MOVE_INTERVAL, così lo schermo non si spegne e le app di chat non ti segnano
//! "assente". Se l'utente torna attivo, smette. Solo mentre è abilitato in config.
//!
//! Nota macOS: muovere il mouse in modo sintetico richiede il permesso
//! "Accessibilità" (Impostazioni di Sistema > Privacy). Senza, il movimento
//! fallisce in silenzio.

use std::time::{Duration, Instant};

use crate::config::ConfigHandle;

const IDLE_THRESHOLD_SECS: u64 = 300; // 5 minuti
const MOVE_INTERVAL: Duration = Duration::from_secs(180); // 3 minuti
const TICK: Duration = Duration::from_secs(15);
/// Un idle piccolo entro questo margine dal nostro nudge è causato dal nudge
/// stesso, non dall'utente: non va interpretato come "utente tornato attivo".
const NUDGE_SETTLE_SECS: u64 = 25;
const ACTIVE_IDLE_SECS: u64 = 15;

pub fn start(config: ConfigHandle) {
    tokio::spawn(async move {
        let mut armed = false;
        let mut last_nudge = Instant::now()
            .checked_sub(MOVE_INTERVAL)
            .unwrap_or_else(Instant::now);

        loop {
            tokio::time::sleep(TICK).await;
            if !config.get().anti_idle_enabled {
                armed = false;
                continue;
            }
            let Some(idle) = idle_seconds() else { continue };
            let since_nudge = last_nudge.elapsed();

            if armed {
                // Idle piccolo ma non subito dopo un nudge = utente tornato attivo.
                if since_nudge.as_secs() > NUDGE_SETTLE_SECS && idle < ACTIVE_IDLE_SECS {
                    armed = false;
                    tracing::info!("anti-idle: utente attivo, movimenti sospesi");
                } else if since_nudge >= MOVE_INTERVAL {
                    nudge();
                    last_nudge = Instant::now();
                    tracing::debug!("anti-idle: mouse mosso");
                }
            } else if idle >= IDLE_THRESHOLD_SECS {
                armed = true;
                nudge();
                last_nudge = Instant::now();
                tracing::info!(idle_secs = idle, "anti-idle: inattività rilevata, avvio movimenti");
            }
        }
    });
}

/// Secondi dall'ultimo input reale dell'utente (tastiera/mouse), a livello di sistema.
#[cfg(target_os = "macos")]
fn idle_seconds() -> Option<u64> {
    let output = std::process::Command::new("ioreg")
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
    None // idle non rilevabile: il jiggler resta inattivo
}

/// Micro-movimento del mouse (1px avanti e indietro): non sposta il puntatore
/// in modo percepibile ma conta come attività di input per il sistema.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn nudge() {
    use enigo::{Coordinate, Enigo, Mouse, Settings};
    match Enigo::new(&Settings::default()) {
        Ok(mut enigo) => {
            let _ = enigo.move_mouse(1, 0, Coordinate::Rel);
            let _ = enigo.move_mouse(-1, 0, Coordinate::Rel);
        }
        Err(e) => tracing::warn!(%e, "anti-idle: impossibile muovere il mouse (permesso Accessibilità?)"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn nudge() {}
