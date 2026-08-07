// 20260807 RG valori defaults: cambiarli qui cambia il comportamento di una installazione nuova, non di una già configurata.

/* ---------- server ---------- */

pub const PORT: u16 = 6969;
pub const LAN_ENABLED: bool = true;
pub const REMOTE_CONTROL_ENABLED: bool = false;

/* ---------- finestra ---------- */

pub const WINDOW_WIDTH: f64 = 1100.0;
pub const WINDOW_HEIGHT: f64 = 720.0;
pub const WINDOW_MIN_WIDTH: f64 = 900.0;
pub const WINDOW_MIN_HEIGHT: f64 = 600.0;
pub const CLOSE_TO_TRAY: bool = true;

/* ---------- log ---------- */

/// livello usato quando RUST_LOG non dice altro.
pub const LOG_FILTER: &str = "info";

/* ---------- intervalli dei collector ---------- */

pub const STATS_INTERVAL_MS: u64 = 10_000;
pub const PORTS_INTERVAL_MS: u64 = 3000;
pub const SERVICES_INTERVAL_MS: u64 = 15_000;
pub const DISKS_INTERVAL_MS: u64 = 10_000;
pub const DOCKER_STATS_INTERVAL_MS: u64 = 3000;
pub const SENSORS_INTERVAL_MS: u64 = 5000;

/* ---------- anti-idle ---------- */

pub const ANTI_IDLE_ENABLED: bool = false;

/* ---------- notifiche push ---------- */

pub const PUSH_ENABLED: bool = false;
pub const PUSH_SERVER: &str = "https://ntfy.sh";
pub const PUSH_MIN_SEVERITY: &str = crate::constants::SEVERITY_WARNING;

/* ---------- soglie degli alert ---------- */

pub const ALERT_CPU_PCT: f64 = 90.0;
pub const ALERT_MEM_PCT: f64 = 92.0;
pub const ALERT_TEMP_C: f64 = 85.0;
pub const ALERT_BATTERY_PCT: f64 = 15.0;
pub const ALERT_TEMP_ENABLED: bool = true;
pub const ALERT_BATTERY_ENABLED: bool = true;

/* ---------- servizi online ---------- */

/// timeout dei preset predefiniti; per servizio è modificabile.
pub const SERVICE_TIMEOUT_MS: u64 = 4000;

/* ---------- strumenti di rete ---------- */

pub const PING_COUNT: u32 = 4;

/* ---------- RickyAI ---------- */

// 20260804 RG RickyAI parte spenta e in modalità remota: nessun processo locale finché non è
// l'utente a chiederlo.
pub const AI_ENABLED: bool = false;
pub const AI_MODE: &str = "remote";
pub const AI_STRATEGY: &str = "balanced";
pub const AI_PORT: u16 = 4141;
