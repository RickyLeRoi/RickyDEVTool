// 20260807 RG valori costanti e indipendenti dal contesto: chiavi di storage, trasporto,
// intervalli, limiti e tabelle di riferimento. I valori di partenza stanno in defaults.ts,
// quelli di aspetto in styles.ts. Questo file non importa nulla: è la base della catena.

/* ---------- chiavi localStorage ---------- */

export const STORAGE_KEYS = {
  theme: "rdt-theme",
  lang: "rdt-lang",
  page: "rdt-page",
  deviceId: "rdt-device-id",
  deviceName: "rdt-device-name",
  deviceSecret: "rdt-device-secret",
  portsGrouping: "rdt-ports-group",
  portScanHistory: "rdt-portscan-history",
  metricsHiddenSeries: "rdt-metrics-hidden",
  comparePaths: "rdt-compare-paths",
  aiThreads: "rdt-rickyai-threads",
  aiModel: "rdt-rickyai-model",
} as const;

/* ---------- server locale e trasporto ---------- */

export const SERVER_PORT = 6969;
// il server prova le porte successive se la prima è occupata: la UI serve dalla porta
// effettiva, quindi il controllo è su un intervallo e non su un numero singolo.
export const SERVER_PORT_FALLBACK_RANGE = 10;
export const LOOPBACK_HOST = "127.0.0.1";
export const WS_PATH = "/ws";
export const WS_BACKOFF_START_MS = 500;
export const WS_BACKOFF_MAX_MS = 10_000;

export const TAURI_GLOBAL_KEY = "__TAURI_INTERNALS__";
export const DEVICE_SECRET_HEADER = "X-RickyDev-Device-Secret";

/* ---------- lingue e temi ---------- */

export const LANGS = ["it", "en"] as const;
export type Lang = (typeof LANGS)[number];

export const LANG_LABELS: Record<Lang, string> = { it: "Italiano", en: "English" };

export const THEMES = ["auto", "light", "dark"] as const;
export type Theme = (typeof THEMES)[number];

/* ---------- navigazione ---------- */

export const PAGES = [
  "dashboard",
  "projects",
  "rickyai",
  "net",
  "tool",
  "log",
  "snippets",
  "ssh",
  "drop",
  "tasks",
  "about",
  "settings",
] as const;
export type Page = (typeof PAGES)[number];

export const NAV_SECTIONS: { id: Page; icon: string; position: "top" | "bottom" }[] = [
  { id: "dashboard", icon: "🖥", position: "top" },
  { id: "projects", icon: "📁", position: "top" },
  { id: "net", icon: "🌐", position: "top" },
  { id: "tool", icon: "🧰", position: "top" },
  { id: "log", icon: "📜", position: "top" },
  { id: "snippets", icon: "⌨️", position: "top" },
  { id: "ssh", icon: "🔑", position: "top" },
  { id: "drop", icon: "📤", position: "top" },
  { id: "tasks", icon: "🧾", position: "bottom" },
  { id: "rickyai", icon: "🤖", position: "bottom" },
  { id: "about", icon: "ℹ️", position: "bottom" },
  { id: "settings", icon: "⚙️", position: "bottom" },
];

export type QuickNavId =
  | "ports"
  | "docker"
  | "clipboard"
  | "launch"
  | "calc"
  | "color"
  | "compare"
  | "services";

export const QUICK_NAV: { id: QuickNavId; icon: string }[] = [
  { id: "ports", icon: "🔌" },
  { id: "docker", icon: "🐳" },
  { id: "clipboard", icon: "📋" },
  { id: "launch", icon: "🚀" },
  { id: "calc", icon: "🧮" },
  { id: "color", icon: "🎨" },
  { id: "compare", icon: "🔀" },
  { id: "services", icon: "📡" },
];

// gli id storici (tray, deep-link salvati) restano validi e vanno tradotti nella coppia
// pagina/tab di oggi.
export const LEGACY_NAV_TARGETS: Record<string, { page: Page; tab: string }> = {
  ports: { page: "net", tab: "listen" },
  services: { page: "net", tab: "services" },
  docker: { page: "net", tab: "docker" },
  launch: { page: "tool", tab: "launch" },
  calc: { page: "tool", tab: "calc" },
  color: { page: "tool", tab: "color" },
  clipboard: { page: "tool", tab: "clipboard" },
  compare: { page: "tool", tab: "compare" },
};

export const TOOL_TAB_IDS = [
  "clipboard",
  "launch",
  "calc",
  "color",
  "cron",
  "compare",
  "tools",
] as const;

export const NET_TAB_IDS = [
  "listen",
  "services",
  "ping",
  "dns",
  "portcheck",
  "traceroute",
  "scan",
  "docker",
] as const;

/* ---------- intervalli di polling ---------- */

export const POLL_MS = {
  dockerVitals: 60_000,
  dockerImages: 5_000,
  dockerContainers: 5_000,
  clipboard: 2_000,
  metricsHistory: 30_000,
  dropHello: 15_000,
} as const;

/* ---------- durate dei riscontri a schermo ---------- */

export const FLASH_SHORT_MS = 1200;
export const FLASH_MS = 1500;
export const TOAST_MS = 4000;

/* ---------- limiti ---------- */

export const STATS_HISTORY_POINTS = 60;
export const LOG_VIEWER_MAX_LINES = 2000;
export const COMMITS_PAGE_SIZE = 50;
export const CLIPBOARD_PREVIEW_CHARS = 280;
export const DEVICE_NAME_MAX_CHARS = 40;
export const DEVICE_SECRET_BYTES = 16;
export const DROP_MAX_TOASTS = 5;
export const CERT_WARN_DAYS = 21;

export const AI_MAX_THREADS = 30;
export const AI_CONTEXT_MESSAGES = 40;
export const AI_TITLE_MAX_CHARS = 40;

export const METRICS_MAX_POINTS = 400;
// buco nella serie: oltre questa distanza fra due campioni la linea si spezza.
export const METRICS_GAP_MS = 5 * 60_000;

/* ---------- porte ---------- */

export const PORT_MIN = 1;
export const PORT_MAX = 65535;
export const AI_PORT_MIN = 1024;

export const PING_ATTEMPTS = 10;
export const PORT_SCAN_BATCH_SIZE = 500;
export const PORT_SCAN_HISTORY_MAX = 5;
export const PORT_SCAN_HISTORY_LABEL_CHARS = 28;
// oltre questa soglia si mostrano solo le porte aperte: l'elenco completo sarebbe illeggibile.
export const PORT_SCAN_SHOW_CLOSED_MAX = 100;

/* ---------- soglie di alert: limiti dei campi ---------- */

export const ALERT_INPUT_LIMITS = {
  cpuPct: { min: 10, max: 100 },
  memPct: { min: 10, max: 100 },
  tempC: { min: 30, max: 120 },
  batteryPct: { min: 1, max: 100 },
} as const;

/* ---------- opzioni selezionabili ---------- */

export const STATS_INTERVAL_OPTIONS_MS = [500, 1000, 2000, 5000, 10_000];
export const DOCKER_STATS_INTERVAL_OPTIONS_MS = [2000, 3000, 5000, 10_000];
export const AI_STRATEGY_IDS = ["balanced", "fast", "local"] as const;
export const AI_MODE_IDS = ["local", "remote"] as const;
export const NODE_PACKAGE_MANAGERS = ["npm", "yarn", "pnpm"] as const;
export const SSH_COMMAND_PRESETS = [
  "uptime",
  "df -h",
  "free -h",
  "docker ps",
  "systemctl --failed",
];
export const LAUNCHABLE_TOOL_IDS = new Set(["vscode", "visualstudio", "terminal"]);

/* ---------- tabelle di riferimento ---------- */

// nome breve degli applicativi riconosciuti dal backend, condiviso da porte e processi.
export const KNOWN_APP_LABELS: Record<string, string> = {
  node: "node",
  dotnet: ".NET",
  docker: "docker",
  ssh: "ssh",
  plex: "plex",
  samba: "samba",
  iisexpress: "IIS",
  visualstudio: "VS",
  vscode: "VS Code",
  postgres: "pg",
  mysql: "mysql",
  redis: "redis",
  nginx: "nginx",
  python: "py",
  java: "java",
  chrome: "chrome",
};

export const DEVICE_NAME_BY_UA: { pattern: RegExp; name: string }[] = [
  { pattern: /iPhone/, name: "iPhone" },
  { pattern: /iPad/, name: "iPad" },
  { pattern: /Android/, name: "Android" },
  { pattern: /Macintosh/, name: "Mac" },
  { pattern: /Windows/, name: "Windows" },
];

export const PORT_SERVICE_NAMES: Record<number, string> = {
  21: "FTP",
  22: "SSH",
  23: "Telnet",
  25: "SMTP",
  53: "DNS",
  67: "DHCP",
  68: "DHCP",
  69: "TFTP",
  80: "HTTP",
  110: "POP3",
  111: "RPCbind",
  123: "NTP",
  135: "RPC (Windows)",
  137: "NetBIOS",
  138: "NetBIOS",
  139: "NetBIOS/SMB",
  143: "IMAP",
  161: "SNMP",
  162: "SNMP trap",
  179: "BGP",
  389: "LDAP",
  443: "HTTPS",
  445: "SMB",
  465: "SMTPS",
  500: "IKE/VPN",
  514: "Syslog",
  515: "LPD",
  587: "SMTP (submission)",
  631: "IPP (CUPS)",
  636: "LDAPS",
  993: "IMAPS",
  995: "POP3S",
  1080: "SOCKS",
  1433: "SQL Server",
  1521: "Oracle DB",
  1723: "PPTP",
  2049: "NFS",
  2181: "ZooKeeper",
  2375: "Docker",
  2376: "Docker (TLS)",
  3000: "dev server",
  3128: "Squid proxy",
  3268: "LDAP GC",
  3306: "MySQL",
  3389: "RDP",
  3690: "Subversion",
  4000: "dev server",
  4369: "Erlang epmd",
  5000: "dev server",
  5060: "SIP",
  5222: "XMPP",
  5432: "PostgreSQL",
  5555: "ADB",
  5601: "Kibana",
  5672: "AMQP",
  5900: "VNC",
  5984: "CouchDB",
  6379: "Redis",
  6443: "Kubernetes API",
  7000: "Cassandra",
  7001: "Cassandra (TLS)",
  7077: "Spark",
  7199: "Cassandra JMX",
  8000: "HTTP-alt",
  8008: "HTTP-alt",
  8080: "HTTP proxy",
  8081: "HTTP-alt",
  8443: "HTTPS-alt",
  8500: "Consul",
  8888: "Jupyter",
  9000: "dev/PHP-FPM",
  9042: "Cassandra CQL",
  9090: "Prometheus",
  9092: "Kafka",
  9200: "Elasticsearch",
  9300: "Elasticsearch (transport)",
  9418: "Git",
  9999: "dev server",
  11211: "Memcached",
  15672: "RabbitMQ mgmt",
  16379: "Redis cluster",
  20000: "dev server",
  25565: "Minecraft",
  27017: "MongoDB",
  27018: "MongoDB",
  28017: "MongoDB HTTP",
  32400: "Plex",
};

export const WELL_KNOWN_PORTS = Object.keys(PORT_SERVICE_NAMES)
  .map(Number)
  .sort((a, b) => a - b);

export const ALL_PORTS = Array.from({ length: PORT_MAX }, (_, i) => i + PORT_MIN);

/* ---------- app ---------- */

export const APP_NAME = "RickyDEVTool";
export const APP_OWNER = "Riccardo Giordano";
export const APP_GITHUB_USER = "RickyLeRoi";
export const APP_GITHUB_PROFILE_URL = `https://github.com/${APP_GITHUB_USER}`;
export const APP_REPO_URL = `https://github.com/${APP_GITHUB_USER}/${APP_NAME}`;
