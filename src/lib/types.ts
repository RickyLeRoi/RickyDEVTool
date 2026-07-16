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

export interface ToolEdition {
  label: string;
  path: string;
}

export interface DiscoveredTool {
  id: string;
  found: boolean;
  path: string | null;
  version: string | null;
  source: "wellKnownPath" | "registry" | "PATH" | "userConfig" | "none";
  platformNote?: string;
  editions?: ToolEdition[];
}

// ---------- progetti / git ----------

export interface DirEntryInfo {
  name: string;
  path: string;
}

export interface DirListing {
  path: string;
  parent: string | null;
  dirs: DirEntryInfo[];
}

export type ProjectKind = "git" | "node" | "dotnet";

export interface ProjectRef {
  path: string;
  name: string;
  kinds: ProjectKind[];
}

export interface FolderScan {
  path: string;
  projects: ProjectRef[];
  truncated: boolean;
}

export type GitWarning =
  | { kind: "no-upstream" }
  | { kind: "diverged"; ahead: number; behind: number }
  | { kind: "detached-head" }
  | { kind: "merge-in-progress" }
  | { kind: "stale-fetch"; days: number };

export interface GitRepoInfo {
  root: string;
  currentBranch: string | null;
  detachedAt?: string;
  dirty: boolean;
  dirtyFiles: number;
  ahead: number | null;
  behind: number | null;
  lastFetchAt: number | null;
  warnings: GitWarning[];
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
