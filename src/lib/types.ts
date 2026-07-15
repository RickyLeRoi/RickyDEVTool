// Modello dati condiviso col backend (vedi PROJECT.md §4).
// Quando i tipi cresceranno (M1+) verranno generati dalle struct Rust con ts-rs.

export type ApiResult<T> =
  | { ok: true; data: T }
  | { ok: false; error: ApiError };

export interface ApiError {
  code: string;
  message: string;
  osHint?: string;
  retryable: boolean;
}

export interface CoreSample {
  core: number;
  pct: number;
}

export interface MachineStats {
  ts: number;
  cpuTotalPct: number;
  cores: CoreSample[];
  mem: { totalBytes: number; usedBytes: number; usedPct: number };
  swap: { totalBytes: number; usedBytes: number } | null;
  intervalMs: number;
}

export interface ProcessInfo {
  pid: number;
  ppid: number | null;
  name: string;
  exePath: string | null;
  user: string | null;
  cpuPct: number;
  memBytes: number;
  memPct: number;
  startedAt: number | null;
  isSystem: boolean;
  knownApp: string | null;
}

export interface HeavyProcessesResult {
  processes: ProcessInfo[];
  sampledAt: number;
  cpuCores: number;
  cpuMinPct: number;
  memMinPct: number;
}

export interface PortProcess {
  pid: number;
  name: string;
  exePath: string | null;
  user: string | null;
  startedAt: number | null;
  isSystem: boolean;
  knownApp: string | null;
  killProtection: "confirm" | "typed-confirm";
}

export interface PortEntry {
  port: number;
  protocol: "tcp";
  addresses: string[];
  processes: PortProcess[];
}

export interface PortScan {
  ports: PortEntry[];
  hiddenSystem: number;
  sampledAt: number;
}

export interface KillRequest {
  pid: number;
  expectedName: string;
  expectedStartedAt: number | null;
  force?: boolean;
  confirmName?: string;
}

export interface KillOutcome {
  killed: boolean;
  forced: boolean;
}

export interface LanInfo {
  urls: string[];
  port: number;
  lanEnabled: boolean;
}

export interface WsEvent {
  topic: string;
  ts: number;
  payload: unknown;
}
