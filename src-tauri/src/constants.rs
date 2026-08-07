// 20260807 RG valori costanti e indipendenti dal contesto.

use std::time::Duration;

/* ---------- applicazione e percorsi ---------- */

pub const APP_NAME: &str = "RickyDEVTool";
/// cartella sotto config_dir()/data_dir(): coincide col nome dell'app.
pub const APP_DIR_NAME: &str = APP_NAME;
pub const CONFIG_FILE_NAME: &str = "config.json";
pub const LOGS_DIR_NAME: &str = "logs";
pub const LOG_FILE_NAME: &str = "rickydevtool.log";
pub const MAIN_WINDOW_LABEL: &str = "main";
pub const TRAY_ID: &str = "main-tray";
pub const LOOPBACK_HOST: &str = "127.0.0.1";
/// in debug la webview punta al dev server di vite, non al server interno.
pub const DEV_SERVER_URL: &str = "http://localhost:1420";

/* ---------- windows ---------- */

// 20260704 RG senza questo flag ogni processo lampeggia una console: vedi exec.rs.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/* ---------- topic degli eventi ---------- */

pub const TOPIC_STATS: &str = "stats";
pub const TOPIC_PORTS: &str = "ports";
pub const TOPIC_DISKS: &str = "disks";
pub const TOPIC_SENSORS: &str = "sensors";
pub const TOPIC_SERVICES: &str = "services";
pub const TOPIC_DOCKER_STATS: &str = "docker:stats";
pub const TOPIC_SENSORS_BACKGROUND: &str = "sensorsbg";

/* ---------- server http/ws e pairing ---------- */

pub const PAIR_COOKIE: &str = "rdt";
/// quante porte oltre a quella configurata si provano prima di arrendersi.
pub const PORT_FALLBACK_RANGE: u16 = 10;
pub const DEVICE_SECRET_HEADER: &str = "x-rickydev-device-secret";
pub const MAX_PAIR_SESSIONS: usize = 32;
/// tronca i campi che finiscono nei log: un client può mandarli lunghi a piacere.
pub const MAX_LOG_FIELD: usize = 2000;
pub const TOKEN_BYTES: usize = 16;
pub const PUSH_TOPIC_PREFIX: &str = "rickydev-";
pub const HUB_ID_PREFIX: &str = "hub-";

// 20260806 ++ RG #Security il codice si legge a voce e si ridigita: alfabeto senza caratteri
// ambigui (niente 0/O, 1/I/L) e gruppi da 4. 12 caratteri = ~60 bit.
pub const HUB_CODE_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
pub const HUB_CODE_LEN: usize = 12;
pub const HUB_CODE_GROUP_LEN: usize = 4;
pub const MIN_HUB_CODE_LEN: usize = 8;

/* ---------- poller ---------- */

pub const POLLER_MAX_BACKOFF_MS: u64 = 60_000;

/* ---------- task ---------- */

pub const MAX_TASK_LOG_LINES: usize = 5000;
/// attesa fra il termine di un task e la pulizia del suo processo.
pub const TASK_REAP_DELAY: Duration = Duration::from_secs(3);

/* ---------- alert ---------- */

pub const CPU_SUSTAINED_SECS: u64 = 60;
pub const ALERT_COOLDOWN_MS: u64 = 600_000;
pub const CERT_ALERT_COOLDOWN_MS: u64 = 86_400_000;
pub const CERT_WARN_DAYS: i64 = 14;
pub const MAX_ALERTS: usize = 50;
pub const SEVERITY_WARNING: &str = "warning";
pub const SEVERITY_CRITICAL: &str = "critical";
/// lettura dei sensori dedicata agli alert, indipendente dal poller della UI.
pub const SENSORS_ALERT_INTERVAL: Duration = Duration::from_secs(60);

/* ---------- notifiche push ---------- */

pub const PUSH_TIMEOUT: Duration = Duration::from_secs(10);

/* ---------- anti-idle ---------- */

pub const IDLE_THRESHOLD_SECS: u64 = 300;
pub const JIGGLER_MOVE_INTERVAL: Duration = Duration::from_secs(180);
pub const JIGGLER_TICK: Duration = Duration::from_secs(10);
pub const NUDGE_SETTLE_SECS: u64 = 25;
pub const ACTIVE_IDLE_SECS: u64 = 15;
pub const NUDGE_PX: i32 = 12;

/* ---------- tray ---------- */

pub const TRAY_TICK: Duration = Duration::from_secs(3);
pub const TRAY_SERVICES_EVERY_TICKS: u32 = 7;
pub const TRAY_TOOLS_EVERY_TICKS: u32 = 20;
/// voci mostrate in un sottomenu prima di riassumere con "e altre N".
pub const TRAY_MAX_LISTED: usize = 25;

/* ---------- strumenti di sviluppo ---------- */

pub const TOOL_IDS: &[&str] = &[
    "vscode", "visualstudio", "git", "node", "npm", "yarn", "pnpm", "dotnet", "docker", "terminal",
];
pub const LAUNCHABLE_TOOLS: [&str; 3] = ["vscode", "visualstudio", "terminal"];
pub const TOOL_VERSION_TIMEOUT: Duration = Duration::from_secs(4);

/* ---------- docker ---------- */

pub const DOCKER_CMD_TIMEOUT: Duration = Duration::from_secs(12);
// 20260806 ++ RG #Security ogni invocazione è uno spawn di processo, e su host ssh:// anche un
// round-trip SSH: due cache lo tengono giù. DOCKER_PS_TTL sta appena sotto i 5s di poll della UI,
// così i pannelli container e immagini riusano la stessa lettura invece di farne una a testa, e le
// richieste che arrivano mentre una chiamata lenta è in corso si fondono in quella. Le azioni
// fatte da qui invalidano esplicitamente.
pub const DOCKER_AVAILABLE_TTL: Duration = Duration::from_secs(60);
pub const DOCKER_PS_TTL: Duration = Duration::from_millis(4000);
pub const DOCKER_HOST_SCHEMES: &[&str] =
    &["tcp://", "ssh://", "unix://", "npipe://", "http://", "https://"];
pub const DOCKER_REF_MAX_LEN: usize = 128;
pub const DOCKER_HOST_MAX_LEN: usize = 255;

/// come si presenta un daemon non raggiungibile sullo stderr della CLI.
pub const DOCKER_DAEMON_DOWN_MARKERS: &[&str] = &[
    "cannot connect to the docker daemon",
    "is the docker daemon running",
    "failed to connect to the docker api",
    "docker.sock",
    "error during connect",
    "connection refused",
    "no route to host",
    "could not resolve hostname",
    "host key verification failed",
    "permission denied",
    "connection timed out",
    "network is unreachable",
    "i/o timeout",
];

/* ---------- processi e porte ---------- */

pub const PROTECTED_PROCESS_NAMES: &[&str] = &[
    "sshd",
    "dockerd",
    "com.docker.backend",
    "smbd",
    "plex media server",
    "postgres",
    "mysqld",
    "redis-server",
    "nginx",
];

pub const LEGIT_DAEMONS: &[&str] =
    &["postgres", "mysql", "redis", "docker", "nginx", "plex", "ssh", "samba"];

pub const DEV_SERVER_APPS: &[&str] = &["node", "python", "dotnet", "java"];
pub const DEV_SERVER_NAMES: &[&str] = &[
    "node", "deno", "bun", "python", "python3", "ruby", "php", "dotnet",
    "vite", "next", "webpack", "nodemon", "rails", "flask", "gunicorn",
    "uvicorn", "cargo", "esbuild", "http-server", "ng",
];

/// nome dell'app riconosciuta a partire dal nome del processo.
pub const KNOWN_APP_RULES: &[(&str, &[&str])] = &[
    ("node", &["node", "node.exe"]),
    ("dotnet", &["dotnet", "dotnet.exe"]),
    ("docker", &["dockerd", "com.docker.backend", "docker.exe", "docker desktop"]),
    ("ssh", &["sshd", "ssh", "sshd.exe"]),
    ("plex", &["plex media server"]),
    ("samba", &["smbd", "nmbd"]),
    ("iisexpress", &["iisexpress.exe"]),
    ("visualstudio", &["devenv.exe"]),
    ("postgres", &["postgres", "postgres.exe"]),
    ("mysql", &["mysqld", "mysqld.exe"]),
    ("redis", &["redis-server", "redis-server.exe"]),
    ("nginx", &["nginx", "nginx.exe"]),
    ("python", &["python", "python3", "python.exe"]),
    ("java", &["java", "java.exe"]),
];

#[cfg(target_os = "macos")]
pub const MACOS_SYSTEM_PATHS: &[&str] = &["/System/", "/usr/libexec/", "/usr/sbin/", "/sbin/"];
#[cfg(target_os = "macos")]
pub const MACOS_SYSTEM_NAMES: &[&str] = &[
    "kernel_task", "launchd", "windowserver", "mds", "mds_stores", "mdworker",
    "distnoted", "cfprefsd", "coreaudiod", "logd", "notifyd", "securityd",
];
/// sotto questo uid i processi sono di sistema.
#[cfg(target_os = "macos")]
pub const MACOS_SYSTEM_UID_MAX: u32 = 500;

#[cfg(target_os = "windows")]
pub const WINDOWS_SYSTEM_USERS: &[&str] = &["system", "local service", "network service"];
#[cfg(target_os = "windows")]
pub const WINDOWS_SYSTEM_NAMES: &[&str] = &[
    "svchost.exe", "csrss.exe", "wininit.exe", "services.exe", "lsass.exe",
    "smss.exe", "winlogon.exe", "dwm.exe", "registry", "memory compression",
];
#[cfg(target_os = "windows")]
pub const WINDOWS_SYSTEM_PATH_PREFIX: &str = "c:\\windows\\";

/* ---------- kill ---------- */

/// attesa fra il segnale gentile e quello secco.
pub const KILL_GRACE_SECS: u64 = 5;
// 20260704 RG il pid può essere stato riciclato: si confronta anche l'istante di avvio.
pub const START_TIME_TOLERANCE_S: i64 = 2;

/* ---------- git ---------- */

pub const GIT_INFO_TIMEOUT: Duration = Duration::from_secs(10);
pub const GIT_NETWORK_TIMEOUT: Duration = Duration::from_secs(90);
pub const GIT_STALE_FETCH_DAYS: u64 = 7;
pub const GIT_MAX_COMMITS: u32 = 200;

/* ---------- progetti ---------- */

pub const PROJECT_SCAN_MAX_DEPTH: usize = 3;
pub const PROJECT_SCAN_MAX_VISITED: usize = 5000;
pub const PROJECT_IGNORED_DIRS: &[&str] = &[
    "node_modules", ".git", "bin", "obj", "dist", "build", "target", "Library",
    ".venv", "venv", "vendor", ".next", ".nuxt", "coverage", "DerivedData",
];

/* ---------- confronto cartelle ---------- */

pub const FSCOMPARE_MAX_ENTRIES: usize = 20_000;

/* ---------- log tail ---------- */

pub const LOGTAIL_POLL_MS: u64 = 500;
pub const LOGTAIL_INITIAL_BYTES: u64 = 64 * 1024;
pub const MAX_TAILS: usize = 5;
pub const TAIL_MAX_AGE_MS: u64 = 2 * 3600 * 1000;
pub const TAIL_MAX_LINE_LEN: usize = 4000;

/* ---------- appunti ---------- */

pub const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(1500);
pub const CLIPBOARD_MAX_ENTRIES: usize = 100;
pub const CLIPBOARD_MAX_TEXT_BYTES: usize = 256 * 1024;
pub const CLIPBOARD_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
pub const CLIPBOARD_CACHE_BUDGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/* ---------- drop ---------- */

pub const PEER_TTL_MS: u64 = 45_000;
pub const TRANSFER_TTL_MS: u64 = 3_600_000;
pub const CLAIM_TTL_MS: u64 = 24 * 3_600_000;
pub const DROP_MAX_TEXT_LEN: usize = 64 * 1024;
pub const MAX_PROXY_BYTES: usize = 200 * 1024 * 1024;

/* ---------- discovery degli hub ---------- */

pub const DISCOVERY_PORT: u16 = 51969;
pub const BEACON_INTERVAL_SECS: u64 = 5;
pub const HUB_TTL_MS: u64 = 22_000;
pub const DISCOVERY_MAGIC: &str = "rickydevtool-hub";

/* ---------- metriche storiche ---------- */

pub const METRICS_SAMPLE_INTERVAL: Duration = Duration::from_secs(30);
pub const METRICS_RETENTION_MS: u64 = 25 * 3_600_000;

/* ---------- servizi online ---------- */

pub const DEGRADED_LATENCY_MS: u64 = 2500;
pub const SERVICE_HISTORY_LEN: usize = 20;
pub const SERVICE_TIMEOUT_MIN_MS: u64 = 500;
pub const SERVICE_TIMEOUT_MAX_MS: u64 = 30_000;
pub const HTTP_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 RickyDEVTool/0.1";
pub const CERT_CACHE_TTL: Duration = Duration::from_secs(3600);

/* ---------- strumenti di rete ---------- */

pub const MAX_PORTS_PER_CALL: usize = 1000;
pub const MAX_CONCURRENT_PORT_CHECKS: usize = 200;

/* ---------- limiti degli elenchi in config ---------- */

pub const MAX_LAUNCH_STEPS: usize = 20;
pub const MAX_SNIPPETS: usize = 200;
pub const MAX_SSH_HOSTS: usize = 100;

/* ---------- RickyAI ---------- */

pub const AI_STRATEGIES: &[&str] = &["balanced", "fast", "local"];
pub const AI_MODES: &[&str] = &["local", "remote"];

// 20260804 RG lista chiusa: solo queste variabili finiscono nell'environment di of-free, così una
// chiave inventata nella config non diventa una variabile arbitraria.
pub const AI_PROVIDER_KEYS: &[(&str, &str, &str)] = &[
    ("groq", "Groq", "GROQ_API_KEY"),
    ("google", "Google AI Studio", "GEMINI_API_KEY"),
    ("cerebras", "Cerebras", "CEREBRAS_API_KEY"),
    ("github", "GitHub Models", "GITHUB_TOKEN"),
    ("mistral", "Mistral La Plateforme", "MISTRAL_API_KEY"),
    ("openrouter", "OpenRouter", "OPENROUTER_API_KEY"),
    ("cohere", "Cohere", "COHERE_API_KEY"),
];

pub const AI_PORT_FALLBACK_RANGE: u16 = 10;
pub const AI_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
pub const AI_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
pub const AI_CHAT_TIMEOUT: Duration = Duration::from_secs(180);
pub const AI_MAX_LOG_LINES: usize = 60;
pub const AI_MAX_MESSAGES: usize = 400;
pub const AI_MAX_CHARS: usize = 400_000;
pub const AI_ADOPTED_POLL: Duration = Duration::from_secs(5);
pub const AI_REMOTE_POLL: Duration = Duration::from_secs(30);
pub const AI_MISSING_RETRY: Duration = Duration::from_secs(300);

/* ---------- permesso rete locale (macOS) ---------- */

// 20260805 RG macOS non espone un AXIsProcessTrusted per la rete locale: l'unico modo di
// leggere il permesso è provare a usarlo.
#[cfg(target_os = "macos")]
pub const MDNS_PROBE_ADDR: &str = "224.0.0.251:5353";
// 20260805 RG EHOSTUNREACH è come si presenta il diniego, e solo così: un host spento dà
// timeout, un servizio chiuso ECONNREFUSED.
#[cfg(target_os = "macos")]
pub const EHOSTUNREACH: i32 = 65;

/* ---------- attività pianificate (Windows) ---------- */

/// campi tenuti dall'output di `schtasks /v`, in inglese e in italiano.
#[cfg(windows)]
pub const SCHTASKS_DETAIL_FIELDS: &[&str] = &[
    "Schedule Type",
    "Start Time",
    "Start Date",
    "Next Run Time",
    "Last Run Time",
    "Tipo pianificazione",
    "Ora di inizio",
    "Prossima esecuzione",
    "Ultima esecuzione",
];
