export type ApiResult<T> =
  | { ok: true; data: T }
  | { ok: false; error: ApiError };

export interface ApiError {
  code: string;
  message: string;
  osHint?: string;
  retryable: boolean;
  retryAfter?: number | null;
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

export interface MetricSample {
  ts: number;
  cpuPct: number;
  memPct: number;
  diskPct: number | null;
}

export interface DockerContainer {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  ports: string[];
}

export interface DockerState {
  available: boolean;
  daemonDown: boolean;
  containers: DockerContainer[];
  error?: string;
  host?: string | null;
}

export interface ContainerStat {
  id: string;
  name: string;
  cpuPct: number;
  memPct: number;
  memUsage: string;
}

export interface DockerImage {
  id: string;
  repository: string;
  tag: string;
  size: string;
  created: string;
  unused: boolean;
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
  zombie: boolean;
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

export interface DirEntryInfo {
  name: string;
  path: string;
}

export interface DirListing {
  path: string;
  parent: string | null;
  dirs: DirEntryInfo[];
}

export type ProjectKind =
  | "git"
  | "node"
  | "dotnet"
  | "python"
  | "rust"
  | "tauri"
  | "flutter";

export type RunnerCategory = "env" | "install" | "build" | "run" | "test" | "clean";

export interface RunnerAction {
  id: string;
  label: string;
  category: RunnerCategory;
  program: string;
  args: string[];
}

export interface RunnerInfo {
  kind: string;
  path: string;
  tool: string;
  notes: string[];
  actions: RunnerAction[];
}

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
  remoteRef?: string | null;
  lastCommit: {
    shortHash: string;
    authorName: string;
    date: number;
    subject: string;
  };
  staleWeeks: number;
}

export interface GitCommit {
  hash: string;
  shortHash: string;
  authorName: string;
  authorEmail: string;
  date: number;
  subject: string;
  refs: string[];
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
  remote: boolean;
}

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

export type DiffStatus = "onlyLeft" | "onlyRight" | "different";

export interface DiffEntry {
  relPath: string;
  status: DiffStatus;
  isDir: boolean;
  leftSize: number | null;
  rightSize: number | null;
  leftMtime: number | null;
  rightMtime: number | null;
}

export interface CompareResult {
  left: string;
  right: string;
  entries: DiffEntry[];
  compared: number;
  identical: number;
  truncated: boolean;
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
  | { kind: "text"; text: string; fromName: string }
  | { kind: "clipboard"; text: string; fromName: string };

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

export interface LaunchStep {
  label: string;
  command: string;
  cwd: string;
}

export interface LaunchBundle {
  id: string;
  name: string;
  steps: LaunchStep[];
}

export interface Snippet {
  id: string;
  name: string;
  command: string;
  cwd: string;
}

export interface SshHost {
  id: string;
  name: string;
  host: string;
  defaultCommand: string;
}

export type ClipKind = "text" | "image" | "files";

export interface ClipFile {
  name: string;
  size: number;
  hasBlob: boolean;
}

export interface ClipImage {
  mime: string;
  width: number;
  height: number;
}

export interface ClipEntry {
  id: number;
  kind: ClipKind;
  text: string;
  bytes: number;
  copiedAt: number;
  pinned: boolean;
  files?: ClipFile[];
  image?: ClipImage;
}

export interface ClipboardHistory {
  entries: ClipEntry[];
  enabled: boolean;
  supported: boolean;
}

export interface AccessibilityStatus {
  supported: boolean;
  trusted: boolean;
}

export interface LocalNetworkStatus {
  supported: boolean;
  granted: boolean;
}

export interface TempReading {
  label: string;
  celsius: number;
}

export interface Battery {
  percent: number;
  charging: boolean;
  state: string;
}

export interface GpuInfo {
  name: string;
  utilizationPct: number | null;
  memUsedMb: number | null;
  memTotalMb: number | null;
  tempC: number | null;
  source: string;
}

export interface SensorsSnapshot {
  temps: TempReading[];
  battery: Battery | null;
  gpus: GpuInfo[];
  maxTempC: number | null;
}

export interface AlertThresholds {
  cpuPct: number;
  memPct: number;
  tempC: number;
  batteryPct: number;
  tempEnabled: boolean;
  batteryEnabled: boolean;
}

export interface SchedEntry {
  schedule: string;
  command: string;
  source: string;
  detail: string | null;
}

export interface SchedListing {
  supported: boolean;
  entries: SchedEntry[];
  note: string | null;
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

export type AiState = "disabled" | "notInstalled" | "starting" | "ready" | "failed";

export interface AiQuotaLimit {
  unit: string;
  window: string;
  remaining: number | null;
  limit: number | null;
  authoritative: boolean;
}

export interface AiProvider {
  name: string;
  label: string;
  available: boolean;
  headroom: number;
  local: boolean;
  limits: AiQuotaLimit[];
}

export type AiMode = "local" | "remote";

export interface AiProviderKey {
  id: string;
  label: string;
  env: string;
}

export interface AiStatus {
  state: AiState;
  port: number;
  baseUrl: string;
  managed: boolean;
  command: string | null;
  message: string | null;
  startedAt: number | null;
  restarts: number;
  log: string[];
  ofFree: boolean;
  enabled: boolean;
  mode: AiMode;
  remoteUrl: string | null;
  remoteKeySet: boolean;
  configuredPort: number;
  strategy: string;
  systemPrompt: string;
  keysSet: string[];
  providerKeys: AiProviderKey[];
  providers: AiProvider[] | null;
  next: { provider: string; model: string } | null;
  models: string[];
}

export interface AiUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface AiReply {
  content: string;
  provider: string | null;
  model: string | null;
  failovers: number | null;
  repinned: string | null;
  finishReason: string | null;
  usage: AiUsage | null;
  elapsedMs: number;
}

export interface WsEvent {
  topic: string;
  ts: number;
  payload: unknown;
}
