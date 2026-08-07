// 20260807 RG valori di partenza dell'applicazione: quelli che l'utente può cambiare (lingua,
// tema, intervalli) e quelli che subisce (tab iniziale, bozze vuote, fallback quando il
// backend non ha ancora risposto). I valori fissi stanno in constants.ts.

import type { Lang, Page, Theme } from "./constants";
import { SSH_COMMAND_PRESETS } from "./constants";
import type { LaunchStep } from "./types";

/* ---------- preferenze utente ---------- */

// 20260807 RG #i18n l'italiano è la lingua base; la scelta esplicita è persistita.
// Per l'auto-detect dalla lingua del browser: leggere navigator.language in i18n.ts.
export const DEFAULT_LANG: Lang = "it";
export const DEFAULT_THEME: Theme = "auto";

/* ---------- stato iniziale della navigazione ---------- */

export const DEFAULT_PAGE: Page = "dashboard";
export const DEFAULT_TOOL_TAB = "clipboard";
export const DEFAULT_NET_TAB = "listen";

/* ---------- dashboard e metriche ---------- */

// fallback finché il primo campione non arriva: è anche il valore con cui parte il poller.
export const DEFAULT_STATS_INTERVAL_MS = 10_000;
export const DEFAULT_PORTS_GROUPING = "port";
export const DEFAULT_METRICS_RANGE_HOURS = 24;

/* ---------- valori di comodo nei form ---------- */

export const DEFAULT_PING_HOST = "1.1.1.1";
export const DEFAULT_PORT_SCAN_PORTS = "22, 80, 443, 3000, 8080";
export const DEFAULT_COMPARE_EXCLUDES = ".git, node_modules";
export const DEFAULT_COMPARE_FILTER = "all";
export const DEFAULT_FORMAT_FILESYSTEM = "exfat";
export const DEFAULT_SSH_COMMAND = SSH_COMMAND_PRESETS[0];

/* ---------- bozze vuote ---------- */

export const EMPTY_SNIPPET_DRAFT = { name: "", command: "", cwd: "" };
export const EMPTY_SSH_DRAFT = { name: "", host: "", defaultCommand: DEFAULT_SSH_COMMAND };
export const emptyLaunchStep = (): LaunchStep => ({ label: "", command: "", cwd: "" });

/* ---------- device ---------- */

// nome mostrato agli altri device quando lo user-agent non dice nulla di riconoscibile.
export const DEFAULT_DEVICE_NAME = "Dispositivo";

/* ---------- RickyAI ---------- */

export const DEFAULT_AI_MODEL = "auto";
export const DEFAULT_AI_THREAD_TITLE = "Nuova chat";

/* ---------- fallback in attesa del backend ---------- */

// la config vive nel backend: finché /api/lan non risponde la UI mostra questi valori, che
// sono gli stessi del Default di AppConfig lato Rust.
export const DEFAULT_CLOSE_TO_TRAY = true;
