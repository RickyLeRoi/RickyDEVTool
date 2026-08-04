use std::time::{Duration, Instant};

use crate::config::ConfigHandle;
#[cfg(target_os = "macos")]
use crate::exec;

const IDLE_THRESHOLD_SECS: u64 = 300;
const MOVE_INTERVAL: Duration = Duration::from_secs(180);
const TICK: Duration = Duration::from_secs(10);
const NUDGE_SETTLE_SECS: u64 = 25;
const ACTIVE_IDLE_SECS: u64 = 15;
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

            if enabled && keep_awake.is_none() {
                keep_awake = KeepAwake::start();
            } else if !enabled {
                keep_awake = None;
            }

            if !enabled {
                armed = false;
                continue;
            }

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

struct KeepAwake {
    #[cfg(target_os = "macos")]
    child: std::process::Child,
    #[cfg(target_os = "windows")]
    _stop: std::sync::mpsc::Sender<()>,
}

impl KeepAwake {
    #[cfg(target_os = "macos")]
    fn start() -> Option<Self> {
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

    #[cfg(target_os = "windows")]
    fn start() -> Option<Self> {
        use windows_sys::Win32::System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
        };

        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<bool>(0);
        // 20260704 RG l'asserzione è legata al thread che la registra e muore con lui: serve un
        // thread dedicato, non il task tokio che migra tra i worker.
        std::thread::spawn(move || {
            let ok = unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)
            } != 0;
            if ready_tx.send(ok).is_err() || !ok {
                return;
            }
            tracing::info!("anti-idle: schermo mantenuto acceso (SetThreadExecutionState)");
            let _ = rx.recv();
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            tracing::info!("anti-idle: schermo di nuovo libero di spegnersi");
        });

        match ready_rx.recv() {
            Ok(true) => Some(Self { _stop: tx }),
            _ => {
                tracing::warn!("anti-idle: SetThreadExecutionState fallito");
                None
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

#[cfg(target_os = "macos")]
fn idle_seconds() -> Option<u64> {
    let output = exec::sync_cmd("ioreg")
        .args(["-c", "IOHIDSystem"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
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
        Some((GetTickCount().wrapping_sub(info.dwTime) / 1000) as u64)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn idle_seconds() -> Option<u64> {
    None
}

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
