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

export interface ProcessGroup {
  name: string;
  knownApp: string | null;
  isSystem: boolean;
  cpuPct: number;
  memBytes: number;
  memPct: number;
  count: number;
  members: ProcessInfo[];
}

export interface HeavyProcessesResult {
  groups: ProcessGroup[];
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

export interface GitBranch {
  name: string;
  isCurrent: boolean;
  isRemoteOnly: boolean;
  lastCommit: {
    shortHash: string;
    authorName: string;
    date: number;
    subject: string;
  };
  staleWeeks: number;
}

export interface NodeProject {
  path: string;
  packageName: string | null;
  packageManager: "npm" | "yarn" | "pnpm";
  pmSource: "lockfile" | "packageManagerField" | "userOverride" | "default";
  scripts: Record<string, string>;
  primaryStart: string | null;
  nodeModulesPresent: boolean;
}

export interface TaskInfo {
  id: string;
  label: string;
  cwd: string;
  state: "running" | "exited" | "failed";
  exitCode: number | null;
  startedAt: number;
}

export type TaskEvent =
  | { event: "line"; stream: "out" | "err"; line: string }
  | { event: "exit"; exitCode: number | null; ok: boolean };

export interface LaunchProfile {
  name: string;
  commandName: string;
  applicationUrl: string | null;
  runnableCrossPlatform: boolean;
}

export interface CsProject {
  csprojPath: string;
  name: string;
  isExecutable: boolean;
  targetFrameworks: string[];
  launchProfiles: LaunchProfile[];
}

export interface DotnetProject {
  path: string;
  slnPath: string | null;
  projects: CsProject[];
  startupProjectPath: string | null;
  selectedProfile: string | null;
}

export interface LanInfo {
  urls: string[];
  port: number;
  lanEnabled: boolean;
  remoteControlEnabled: boolean;
  antiIdleEnabled: boolean;
}

// ---------- net tools ----------

export interface PingResult {
  host: string;
  sent: number;
  received: number;
  timesMs: number[];
  avgMs: number | null;
  error: string | null;
}

export interface DnsRecordSet {
  recordType: string;
  values: string[];
}

export interface PortCheckResult {
  port: number;
  open: boolean;
  latencyMs: number | null;
  error: string | null;
}

export interface LanHost {
  ip: string;
  hostname: string | null;
  mac: string | null;
  latencyMs: number | null;
  isSelf: boolean;
}

export interface DropPeer {
  deviceId: string;
  name: string;
  isDesktop: boolean;
  lastSeen: number;
  remote: boolean;
}

export type DropIncoming =
  | {
      kind: "file";
      transferId: string;
      name: string;
      sizeBytes: number;
      fromName: string;
      savedPath: string | null;
    }
  | { kind: "text"; text: string; fromName: string };

export interface ReceivedFile {
  name: string;
  sizeBytes: number;
  modifiedAt: number | null;
}

export interface EnvFile {
  name: string;
  sizeBytes: number;
  modifiedAt: number | null;
  isActive: boolean;
}

export interface EnvEntry {
  key: string;
  value: string;
  raw: string | null;
}

export interface EnvContent {
  file: string;
  entries: EnvEntry[];
}

export interface TailInfo {
  id: string;
  path: string;
  startedAt: number;
}

export interface AccessibilityStatus {
  supported: boolean;
  trusted: boolean;
}

export interface DiskInfo {
  name: string;
  mountPoint: string;
  fileSystem: string;
  totalBytes: number;
  availableBytes: number;
  usedPct: number;
  isRemovable: boolean;
  isSystem: boolean;
}

// ---------- servizi online / alerts ----------

export interface ServiceDef {
  id: string;
  label: string;
  kind: "http" | "tcp";
  target: string;
  expectStatus?: number[] | null;
  timeoutMs: number;
  builtin: boolean;
  enabled: boolean;
}

export type ServiceState = "up" | "degraded" | "down";

export interface ServiceStatus {
  id: string;
  label: string;
  state: ServiceState;
  latencyMs: number | null;
  httpStatus: number | null;
  error: string | null;
  checkedAt: number;
  history: ServiceState[];
  certExpiresAt: number | null;
  certDaysLeft: number | null;
}

export interface PushConfig {
  enabled: boolean;
  server: string;
  topic: string;
  minSeverity: "info" | "warning" | "critical";
}

export interface Alert {
  id: string;
  severity: "info" | "warning" | "critical";
  kind: string;
  title: string;
  detail: string;
  createdAt: number;
  acknowledged: boolean;
}

export interface WsEvent {
  topic: string;
  ts: number;
  payload: unknown;
}
