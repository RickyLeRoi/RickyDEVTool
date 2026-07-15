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
