# RickyDEV — Developer Operations Console locale

Proposta tecnica completa. Tool desktop multipiattaforma (Windows, macOS; Linux best-effort) per operazioni quotidiane da sviluppatore: monitor risorse, porte/processi, launcher IDE, gestione progetti (git/Node/.NET), monitor servizi online. Accessibile anche da smartphone via LAN.

**Decisione di fondo**: Tauri 2 (core Rust) + React/TypeScript, con web server axum embedded sulla porta **6969** che serve la stessa SPA sia alla finestra desktop sia allo smartphone. Un solo binario, un solo codice UI, due form factor.

---

## 1. Valutazione critica dell'idea

### Punti forti

- **Locale-first, zero cloud**: niente account, niente sync, niente backend remoto. Riduce enormemente la superficie del progetto.
- **Il requisito "webserver su 6969 + accesso da smartphone" è la scelta architetturale più intelligente del brief**: obbliga a separare UI e servizi dal giorno 1 e regala la UI mobile quasi gratis (stessa SPA, layout responsive).
- Le 5 funzionalità condividono la stessa spina dorsale (poller + adapter OS + event push): il costo marginale di ogni sezione dopo la prima è basso.
- Target realistico: l'utente è lo sviluppatore stesso. Si può tagliare tutto ciò che non si usa davvero.

### Rischi

| Rischio | Gravità | Note |
|---|---|---|
| Antivirus/SmartScreen su Windows | Alta | Un binario non firmato che enumera processi, legge porte e killa processi è un pattern che Defender guarda male. Firma del codice quasi obbligatoria per distribuire; per uso personale basta un'esclusione. |
| Kill di processi altrui / elevati | Alta | Su entrambi gli OS non si possono killare processi di altri utenti o elevati senza privilegi. Va gestito come errore pulito, non nascosto. |
| "Start Debug" per .NET | Alta | Avviare *davvero* il debugger di Visual Studio da un tool esterno significa automazione COM DTE: fragile, Windows-only, dipendente dalla versione di VS. Feature sopravvalutata (dettagli in §4c/discovery). |
| Esposizione LAN di kill/git/exec | Media | Senza autenticazione, chiunque sulla rete può killare processi o lanciare comandi. Serve un pairing token dal giorno 1. |
| Parsing di output CLI (`lsof`, `netstat`) | Media | Fragile tra versioni OS/locale. Mitigabile con API native dove esistono e test di parsing con fixture reali. |
| Grafo commit git | Media | Rabbit hole classico: rinviato in forma semplificata a v2. |

### Complessità nascoste

1. **CPU% per processo**: su macOS il valore è "per core" (può superare 100%), su Windows è già normalizzato. Decisione: sempre normalizzato sul totale core, uniforme sui due OS.
2. **Sleep/wake**: i timer di polling dopo un resume producono delta assurdi (CPU al 400%, trend rotti). Rilevare il gap e scartare il primo campione.
3. **IP da mostrare per l'accesso mobile**: una macchina ha N interfacce (Wi-Fi, Ethernet, VPN, Docker bridge, Tailscale). Serve lista filtrata con euristica (interfaccia con default gateway prima).
4. **Autenticazione git**: fetch/pull su repo privati richiedono credenziali. Con libgit2 bisognerebbe reimplementare credential helper e SSH agent; shellando su `git` CLI funziona tutto gratis. **Scelta: git CLI.**
5. **Monorepo/cartelle grandi**: rilevare git repo annidati può significare scandire node_modules da 200k file. Depth limit + ignore list fin da subito.
6. **PID reuse / race sul kill**: il kill verifica PID + nome + start time, non solo il PID.

### Da ridurre o rinviare

| Cosa | Verdetto |
|---|---|
| Grafo commit interattivo con checkout | **v2**, in forma semplificata (lista commit selezionabile, non grafo disegnato). |
| Start/Stop Debug VS via automazione | **v2 Windows-only**, sostituito in v1 da `dotnet run --launch-profile` (copre l'80% del bisogno reale). |
| Kill multiplo da mobile | Mobile read-only finché non si abilita "controllo remoto" dal desktop. |
| Icone per-processo estese | MVP: set statico di ~15 icone note. |
| Linux | Target best-effort, buildato in CI ma non testato a mano. |

---

## 2. Feature set per fasi

### MVP (usabile ogni giorno entro ~3 settimane)

- Binario unico, tray icon, finestra desktop, server su 6969, pagina mobile con QR code + IP per il pairing.
- Dashboard CPU/RAM: totale + per core, intervallo configurabile (0.5–10s), sparkline ultimi 60 campioni, lista processi sopra soglia **a richiesta**.
- Monitor porte: lista porte LISTEN di processi non-system, raggruppata per porta, kill con conferma, attiva solo a sezione aperta.
- Launcher: rilevamento VS Code + VS (Windows), apertura, path manuale di fallback.
- Cartelle: selezione cartella, riconoscimento git/node/dotnet, azioni comuni (apri in VS Code, apri terminale, copia path).
- Git base: branch corrente, dirty state, ahead/behind, Fetch, Pull.

### v1

- Dropdown branch con sottotitolo (hash/data/autore ultima commit), checkout, colore branch >4 settimane.
- Node: install/start/script dropdown, detection npm/yarn/pnpm.
- .NET: rebuild/clean (`dotnet` CLI, cross-platform), scelta startup project, run con launch profile, Open in VS.
- Monitor servizi online configurabile (attivo solo a sezione aperta), inclusi servizi personali dietro cloudflared.
- Icone processi noti, raggruppamento porte commutabile porta/processo.
- Modalità "controllo remoto" per il mobile (azioni distruttive da telefono, dietro toggle + token).
- Alerts (CPU sostenuta, RAM, servizio down) come toast + badge tray.

### v2

- Lista commit selezionabile con checkout in detached HEAD (il "grafo" onesto).
- Start/Stop Debug via VS DTE (Windows-only, dichiaratamente fragile).
- Log viewer dei processi avviati dal tool (stdout/stderr di `npm start`, `dotnet run`).
- Auto-updater (tauri-plugin-updater).
- Storico metriche persistito (ultime 24h su SQLite).

---

## 3. Architettura

### Stack: perché Tauri 2

| Opzione | Pro | Contro | Verdetto |
|---|---|---|---|
| **Tauri 2 (Rust)** | Binario 10–20 MB, webview di sistema, `sysinfo`/`axum`/`tokio` maturi, bundler MSI/DMG/deb integrato, tray + autostart plugin ufficiali | Curva Rust | **Scelta** |
| Electron | Ecosistema Node, zero attrito per dev JS | 150+ MB, non è "un file", RAM footprint contraddice lo spirito del tool | No |
| Go + Wails / Go headless + browser | Binario statico, `gopsutil` ottimo | Wails meno rifinito su tray/bundling; UI solo browser perde il feel desktop | Seconda scelta valida |
| .NET MAUI / Avalonia | Comodo per dev .NET | Server HTTP embedded + mobile web = più lavoro; bundle non piccolo; MAUI su macOS zoppica | No |

**Decisione chiave**: la finestra Tauri carica la **stessa SPA servita da axum su `http://127.0.0.1:6969`**. Non ci sono due UI né due canali IPC: tutto passa da HTTP + WebSocket, sia in locale sia da LAN. Tauri fornisce solo: finestra, tray, dialog nativo di selezione cartella, notifiche, autostart.

```
┌─────────────────────────── binario unico ───────────────────────────┐
│  Tauri shell (finestra, tray, dialogs)                               │
│  ┌───────────────────────── core Rust ─────────────────────────┐     │
│  │  axum HTTP+WS :6969 ── auth middleware (localhost/LAN token) │     │
│  │  ┌─────────── servizi applicativi (OS-agnostici) ─────────┐  │     │
│  │  │ StatsService · PortsService · ProjectsService ·        │  │     │
│  │  │ GitService · NodeService · DotnetService ·             │  │     │
│  │  │ ServicesMonitor · DiscoveryService · AlertService      │  │     │
│  │  └────────────────────────┬────────────────────────────── ┘  │     │
│  │        PollerRegistry ────┤──── EventBus (tokio broadcast)   │     │
│  │  ┌────────────────────────┴──────────────────────────────┐   │     │
│  │  │ adapter OS (trait + impl win/mac):                    │   │     │
│  │  │ SysStats · PortScanner · ProcessKiller · AppLocator · │   │     │
│  │  │ TerminalLauncher · ShellExec                          │   │     │
│  │  └───────────────────────────────────────────────────────┘   │     │
│  └──────────────────────────────────────────────────────────────┘     │
└───────────────────────────────────────────────────────────────────────┘
        ▲ webview desktop (stessa SPA)      ▲ smartphone via LAN
```

### Moduli frontend (React + TS + Vite)

- `app/` — shell, routing (desktop: layout a pannelli; mobile: tab bar), tema.
- `stores/` — **Zustand**, uno store per dominio (`statsStore`, `portsStore`, `projectsStore`, `servicesStore`, `alertsStore`).
- `ws/` — un solo client WebSocket con multiplexing per topic (`stats`, `ports`, `services`, `alerts`, `taskOutput:{id}`). Reconnect con backoff. È lui che comunica al backend "questa sezione è aperta" (subscribe/unsubscribe → il backend accende/spegne i poller).
- `api/` — client REST tipizzato per le azioni one-shot (kill, checkout, run script…).
- `features/` — un folder per sezione: `dashboard`, `ports`, `launcher`, `projects`, `services`.
- `components/` — primitive compatte: `Sparkline`, `Gauge`, `ProcTable`, `ContextMenu`, `ConfirmDialog`, `Badge`, `Spinner`.

### Moduli backend (Rust)

- `server/` — axum: route REST, WS handler, static serving della SPA (embedded con `rust-embed`), middleware auth.
- `services/` — logica applicativa pura, testabile, che parla solo con i trait degli adapter.
- `adapters/` — `trait SysStatsProvider`, `trait PortScanner`, ecc. con `#[cfg(target_os)]` per le impl. **Regola: nessun `cfg(target_os)` fuori da questa cartella.**
- `poller/` — PollerRegistry (vedi sotto).
- `config/` — `config.json` in `dirs::config_dir()/rickydev/` (Win: `%APPDATA%`, mac: `~/Library/Application Support`), watch + hot reload, scrittura atomica.
- `tasks/` — esecuzione comandi long-running (npm install, dotnet build): registry di task con id, stream stdout/stderr sul topic WS `taskOutput:{id}`, cancel = kill del process tree.

### Event bus e state management

- Backend: `tokio::sync::broadcast` per canale-per-topic. I servizi pubblicano `Event { topic, payload, ts }`; il layer WS inoltra ai client sottoscritti.
- Frontend: il WS client smista negli store Zustand. Le azioni REST rispondono col risultato immediato; gli aggiornamenti di stato conseguenti arrivano comunque via WS (single source of truth = push).

### Scheduling / polling

`PollerRegistry`: ogni collector si registra con `{ topic, interval, collect_fn }`. Il registry tiene il conteggio dei subscriber WS per topic:

- 0 subscriber → collector fermo (requisito esplicito per porte e servizi online; applicato a tutto).
- ≥1 subscriber → tick loop tokio con intervallo corrente (modificabile a runtime via REST).
- Errore nel collect → backoff esponenziale (x2 fino a 60s) + evento `topic:error`, reset al primo successo.
- Rilevamento sleep/wake: se `now - last_tick > 3 * interval`, scarta il campione (delta CPU non affidabile).

### Command execution

Tutto ciò che lancia processi passa da un unico `ShellExec`:

- Niente shell interposta dove possibile (argv diretto) → zero injection da input UI.
- `cwd` esplicito, env ereditato + `PATH` arricchito (su macOS le app GUI non ereditano il PATH della shell: leggerlo con `/bin/zsh -lc 'echo $PATH'` una volta all'avvio).
- Timeout di default, kill del **process tree** su cancel (Windows: Job Objects; macOS: process group + `SIGTERM`→`SIGKILL` dopo 5s).

### Permission model

| Origine richiesta | Lettura (stats, porte, repo) | Azioni (kill, checkout, run) |
|---|---|---|
| Webview desktop / localhost | libera | libera, con confirm UI per azioni distruttive |
| LAN con token valido | libera | **solo se "Remote control" è ON** sul desktop (default OFF) |
| LAN senza token | 401, pagina di pairing | negato |

Pairing: il desktop mostra QR con `http://<ip>:6969/#pair=<token>`; il token (random 128 bit, in config) diventa cookie. Server bindato su `0.0.0.0` solo se "Accesso LAN" è attivo, altrimenti `127.0.0.1`. Le richieste localhost si riconoscono dal peer address. Kill di processi in lista protetta: confirm rafforzato (digitare il nome del processo), mai da remoto.

### Logging e gestione errori

- `tracing` + `tracing-appender`, file rotante giornaliero in `data_dir()/logs/`, livello da config. Ogni azione utente loggata con esito.
- Risposta API uniforme: `{ ok: true, data } | { ok: false, error: { code, message, osHint?, retryable } }`. `code` è un enum stabile (`ACCESS_DENIED`, `PROCESS_GONE`, `GIT_AUTH_FAILED`, `TOOL_NOT_FOUND`…), `osHint` è la spiegazione OS-specifica.
- Endpoint `POST /api/log` per gli errori frontend, nello stesso file di log.

---

## 4. Modello dati TypeScript

Generato dalle struct Rust con `ts-rs` (o `specta`) per non mantenere due verità.

```ts
// ---------- envelope ----------
export type ApiResult<T> =
  | { ok: true; data: T }
  | { ok: false; error: ApiError };

export interface ApiError {
  code: "ACCESS_DENIED" | "PROCESS_GONE" | "TOOL_NOT_FOUND" | "GIT_AUTH_FAILED"
      | "GIT_DIRTY" | "PATH_NOT_FOUND" | "TIMEOUT" | "REMOTE_FORBIDDEN" | "INTERNAL";
  message: string;
  osHint?: string;
  retryable: boolean;
}

// ---------- machine stats ----------
export interface MachineStats {
  ts: number;                     // epoch ms
  cpuTotalPct: number;            // 0..100, normalizzato su tutti i core
  cores: CoreSample[];
  mem: { totalBytes: number; usedBytes: number; usedPct: number };
  swap?: { totalBytes: number; usedBytes: number };
  intervalMs: number;             // intervallo attivo al momento del campione
}
export interface CoreSample { core: number; pct: number }

export interface StatsHistory {           // per sparkline
  window: number;                          // n campioni (ring buffer, es. 60)
  cpu: number[];
  memPct: number[];
}

// ---------- processi ----------
export interface ProcessInfo {
  pid: number;
  ppid: number | null;
  name: string;
  exePath: string | null;         // null se non leggibile (permessi)
  user: string | null;
  cpuPct: number;                 // normalizzato su tutti i core (uniforme Win/mac)
  memBytes: number;
  memPct: number;
  startedAt: number | null;       // usato per validare il kill (PID reuse)
  isSystem: boolean;
  knownApp: KnownAppId | null;    // per icona
}
export type KnownAppId =
  | "node" | "docker" | "ssh" | "plex" | "samba" | "iisexpress"
  | "visualstudio" | "vscode" | "dotnet" | "postgres" | "mysql"
  | "redis" | "nginx" | "python" | "java" | "chrome";

export interface KillRequest {
  pid: number;
  expectedName: string;           // il backend rifiuta se il PID ora è un altro processo
  expectedStartedAt: number | null;
  force: boolean;                 // SIGKILL / TerminateProcess subito
}

// ---------- porte ----------
export interface PortEntry {
  port: number;
  protocol: "tcp" | "udp";
  addresses: string[];            // "127.0.0.1", "0.0.0.0", "::" ...
  processes: PortProcess[];       // >1 se SO_REUSEPORT o worker multipli
}
export interface PortProcess {
  pid: number;
  name: string;
  exePath: string | null;
  isSystem: boolean;
  knownApp: KnownAppId | null;
  killProtection: "none" | "confirm" | "typed-confirm";
}

// ---------- servizi online ----------
export interface ServiceDef {
  id: string;
  label: string;
  kind: "http" | "tcp";           // niente ICMP
  target: string;                 // URL per http, "host:port" per tcp
  expectStatus?: number[];        // default [200..399]; 204 per generate_204
  timeoutMs: number;              // default 4000
  builtin: boolean;               // i preset non sono cancellabili, solo disattivabili
  enabled: boolean;
}
export interface ServiceStatus {
  id: string;
  state: "up" | "degraded" | "down" | "checking" | "unknown";
  latencyMs: number | null;
  httpStatus?: number;
  error?: string;
  checkedAt: number;
  history: ("up" | "down" | "degraded")[];  // ultimi N esiti per la barra
}

// ---------- git ----------
export interface GitRepoInfo {
  root: string;
  currentBranch: string | null;   // null = detached HEAD
  detachedAt?: string;            // short hash se detached
  dirty: boolean;
  dirtyFiles: number;
  ahead: number | null;           // null = nessun upstream
  behind: number | null;
  lastFetchAt: number | null;     // da mtime di .git/FETCH_HEAD
  branches: GitBranch[];
  warnings: GitWarning[];
}
export interface GitBranch {
  name: string;
  isCurrent: boolean;
  isRemoteOnly: boolean;
  upstream: string | null;
  lastCommit: { shortHash: string; authorName: string; date: number; subject: string };
  staleWeeks: number;             // settimane dall'ultima commit; UI colora se >= 4
}
export type GitWarning =
  | { kind: "no-upstream" }
  | { kind: "diverged"; ahead: number; behind: number }
  | { kind: "detached-head" }
  | { kind: "merge-in-progress" }
  | { kind: "stale-fetch"; days: number };

// ---------- progetti ----------
export interface FolderScan {
  path: string;
  entries: FolderEntry[];
  projects: ProjectRef[];         // trovati in questa cartella o sotto (depth-limited)
}
export interface FolderEntry { name: string; isDir: boolean; project?: ProjectRef }
export interface ProjectRef {
  id: string;                     // hash del path
  path: string;
  kinds: ("git" | "node" | "dotnet")[];   // una cartella può essere più cose
}

export interface NodeProject {
  path: string;
  packageName: string | null;
  packageManager: "npm" | "yarn" | "pnpm";
  pmSource: "lockfile" | "packageManagerField" | "userOverride" | "default";
  scripts: Record<string, string>;        // nome -> comando (per tooltip)
  primary: { install: string; start: string | null };  // "start" o "dev" se manca start
  nodeModulesPresent: boolean;
}

export interface DotnetProject {
  path: string;                   // cartella
  slnPath: string | null;
  projects: CsProject[];
  startupProjectPath: string | null;      // scelta utente persistita
  selectedProfile: string | null;
}
export interface CsProject {
  csprojPath: string;
  name: string;
  isExecutable: boolean;          // OutputType Exe o Sdk.Web
  targetFrameworks: string[];
  launchProfiles: LaunchProfile[];
}
export interface LaunchProfile {
  name: string;
  commandName: "Project" | "IISExpress" | "Executable" | string;
  applicationUrl?: string;
  runnableCrossPlatform: boolean; // false per IISExpress → badge Windows-only
}

// ---------- discovered tools ----------
export interface DiscoveredTool {
  id: "vscode" | "visualstudio" | "git" | "node" | "dotnet" | "terminal"
    | "npm" | "yarn" | "pnpm" | "docker";
  found: boolean;
  path: string | null;
  version: string | null;
  source: "wellKnownPath" | "registry" | "PATH" | "spotlight" | "userConfig";
  platformNote?: string;          // es. "Visual Studio: solo Windows"
  editions?: { label: string; path: string }[];  // VS 2022 Community/Professional...
}

// ---------- alerts ----------
export interface Alert {
  id: string;
  severity: "info" | "warning" | "critical";
  kind: "cpu-sustained" | "mem-high" | "service-down" | "task-failed" | "kill-failed";
  title: string;
  detail: string;
  createdAt: number;
  acknowledged: boolean;
  contextRef?: { section: string; targetId: string };  // deep-link alla sezione
}
```

---

## 5. UX/UI

### Layout finestra desktop

Finestra default **1100×720**, minimo 900×600, densità alta (font 13px, righe 28px).

```
┌──┬──────────────────────────────────────────────┬────────────┐
│  │  [contenuto sezione attiva]                  │ CPU ▁▂▅▃▂  │
│🖥│                                              │ 43%  8core │
│🔌│                                              │ RAM ▂▂▃▃▄  │
│📁│                                              │ 11.2/16 GB │
│🌐│                                              ├────────────┤
│  │                                              │ ⚠ alerts   │
│⚙ │                                              │ (compatti) │
└──┴──────────────────────────────────────────────┴────────────┘
 48px rail                                          220px, collassabile
```

- **Rail sinistro** (icone): Dashboard, Porte, Progetti, Servizi, Impostazioni.
- **Pannello destro sempre visibile**: mini CPU/RAM con sparkline + ultimi alert; collassabile a barra di 8px colorata.
- **Tray**: icona con stato (verde/giallo/rosso da CPU/alert), menu con "Apri", "Accesso LAN: 192.168.1.x:6969", "Quit".
- Mobile: tab bar in basso (Dashboard, Porte, Progetti, Servizi), card verticali, niente hover — tutto tap/long-press.

### Dashboard

- Gauge CPU totale + griglia core (barrette verticali, non N gauge), gauge RAM.
- Sparkline 60 campioni sotto ogni gauge (ring buffer, nessuna persistenza in MVP).
- Selettore intervallo inline: `0.5s · 1s · 2s · 5s · 10s` (segmented control, effetto immediato).
- Bottone "Processi pesanti" → tabella on-demand (nome+icona, PID, CPU%, MEM%, utente) con soglie di default >20% CPU / >10% RAM modificabili nell'header. Kill diretto da qui con la stessa logica della sezione porte.

### Sezione porte e context menu

Vista principale: tabella compatta raggruppata **per porta**: `porta · protocollo · bind · [icone processi] · n processi`.

Context menu (right-click su una riga, o bottone "⋮"):

```
┌─ Porta 3000 (tcp, 0.0.0.0) ────────────┐
│ ▸ node — vite dev server    PID 4821 ──┼─▶ ┌──────────────────────────┐
│ ▸ node — esbuild            PID 4830   │   │ ⬛ Kill (SIGTERM)         │
│ ────────────────────────────────────── │   │ ⬛ Force kill             │
│ Copia "localhost:3000"                 │   │ 📋 Copia PID              │
│ Apri nel browser                       │   │ 📂 Mostra eseguibile      │
│ Kill tutti i processi su questa porta  │   └──────────────────────────┘
└────────────────────────────────────────┘      (submenu su hover, 250ms)
```

**Decisione raggruppamento**: primario **per porta** (il caso d'uso reale è "la 3000 è occupata, da chi?"). Toggle "per processo" in header in v1. Non entrambe le viste insieme.

**Conferme kill**: processo normale → conferma singola con nome+PID; processo in lista protetta (sshd, dockerd, smbd, agent di sistema) → dialog che richiede di digitare il nome; processo `isSystem` → non killabile dal tool. Da mobile: solo con Remote control ON, sempre typed-confirm.

### Explorer cartelle/progetti

- Barra superiore: cartelle pinnate (chip) + "Apri cartella…" (dialog nativo via Tauri).
- Albero a sinistra (solo directory, lazy), pannello destro col dettaglio del progetto selezionato.
- Ogni nodo che è un progetto ha badge: ` git` ` node` `.NET` (combinabili).
- Header del dettaglio, per qualunque cartella: `[icona VS Code] [icona terminale] [icona file manager] [copia path]` — le azioni comuni.
- Pannelli per tipo rilevato:
  - **Git**: riga stato (`main · ↑2 ↓1 · ● 3 file modificati · ⚠ diverged`), bottoni `Fetch` `Pull`, dropdown branch. Ogni voce su due righe: nome branch (rosso/ambra se `staleWeeks ≥ 4`) e sottotitolo `a1b2c3d · 12 mag · Riccardo`. Bottone `Checkout` solo sulle voci ≠ corrente; disabilitato con tooltip se dirty.
  - **Node**: badge package manager rilevato (`pnpm ▾`, cliccabile per override), `Install`, `Start`, dropdown altri script. Output in pannello log inline richiudibile.
  - **.NET**: dropdown startup project (solo `isExecutable`), dropdown profilo launchSettings (badge `Win only` sui profili IISExpress), poi `Run` `Stop` `Rebuild` `Clean` `Open in VS` (disabilitato su macOS con tooltip esplicito).

### Stati e linee guida di compattezza

- **Loading**: skeleton solo al primo load; i refresh aggiornano in place senza flicker (mai smontare la tabella durante il polling). Puntino pulsante nell'header = "polling attivo".
- **Empty**: una riga, non illustrazioni ("Nessuna porta non di sistema in ascolto").
- **Error**: banner inline con `message` + `osHint` + Riprova; il polling in errore mostra l'ultimo dato valido in grigio con timestamp.
- Densità: font 13px, una sola famiglia; icone 16px; righe e divisori, niente card annidate; cifre che cambiano spesso in `font-variant-numeric: tabular-nums`; animare solo opacità/colore, mai layout; colori semantici soltanto (verde/ambra/rosso), il resto neutro.

---

## 6. Logica di discovery

### VS Code

Ordine: (1) config utente → (2) path noti → (3) `PATH`.

- **Windows**: `%LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe`, poi `%ProgramFiles%\Microsoft VS Code\Code.exe`, poi registry `HKLM/HKCU ...\Uninstall\{771FD6B0-...}`, poi `where code`.
- **macOS**: `/Applications/Visual Studio Code.app`, `~/Applications/...`; apertura cartella con `open -a "Visual Studio Code" <path>` (funziona anche senza `code` nel PATH). Fallback `which code`.
- Versione: `code --version` (prima riga).

### Visual Studio (Windows-only)

- Metodo canonico: **vswhere** — `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe -all -products * -format json`. Presente se c'è un VS ≥2017. Restituisce edition, versione, `productPath` (devenv.exe). Popola `editions[]` se più di uno.
- Su macOS: `found=false` con `platformNote: "Visual Studio for Mac è stato ritirato (2024); su macOS usa VS Code o Rider"`.
- Apertura soluzione: `devenv.exe <path.sln>`.

### Git repo (anche annidati)

- Una cartella è repo se esiste `.git` (dir **o file** — worktree e submodule hanno `.git` file). Conferma con `git -C <path> rev-parse --show-toplevel`.
- Scan annidato: walk con **depth max 3** (configurabile), pruning hard di `node_modules`, `.git` interni, `bin`, `obj`, `dist`, `target`, `Library`. Se una dir è repo root, non cercare altri repo dentro.
- Dati branch in una invocazione: `git for-each-ref --sort=-committerdate --format='%(refname:short)|%(objectname:short)|%(committerdate:unix)|%(authorname)|%(subject)' refs/heads refs/remotes`. Ahead/behind + dirty: `git status --porcelain=v2 --branch`. Tutto via **git CLI di sistema**: l'auth (SSH agent, credential manager) funziona senza scrivere una riga.

### Node / .NET

- **Node**: `package.json` presente. PM: `pnpm-lock.yaml` → pnpm; `yarn.lock` → yarn; `package-lock.json` → npm; altrimenti campo `packageManager`; altrimenti npm + badge "assunto". Override utente persistito per progetto. Esecuzione sempre via `<pm> run <script>` con cwd = progetto.
- **.NET**: `.sln`/`.slnx` in root o primo livello; altrimenti `.csproj` diretti. Parse `.sln` per la lista progetti (regex sulle righe `Project(...)`). Per ogni `.csproj`: `OutputType`, `Sdk` attribute (`Microsoft.NET.Sdk.Web` ⇒ eseguibile), `TargetFramework(s)`. `Properties/launchSettings.json` per i profili; `commandName=="Project"` runnable con `dotnet run --project <csproj> --launch-profile <nome>` su entrambi gli OS; `IISExpress` Windows-only e non lanciabile via `dotnet run` → badge, uso solo con "Open in VS".

### Processi noti → icone

Tabella statica di regole `KnownAppRule { id, match: { names: string[], pathContains?: string[] } }` su nome eseguibile lowercase e path. Esempi: `node|node.exe`→node; `com.docker.backend|dockerd|docker.exe`→docker; `sshd|ssh`→ssh; `Plex Media Server`→plex; `smbd`→samba; `iisexpress.exe`→iisexpress; `devenv.exe`→visualstudio; `Code|Code.exe|Code Helper*`→vscode; `dotnet`→dotnet. ~15 regole in MVP, estendibili da config. Icone: set SVG embedded (non estrarre icone reali dagli exe).

### Sistema vs non-sistema

Euristica a punteggio, dichiaratamente imperfetta:

| Segnale | Windows | macOS |
|---|---|---|
| Utente | SID noti: SYSTEM, LOCAL SERVICE, NETWORK SERVICE | `uid < 500` |
| Path eseguibile | sotto `C:\Windows\` | sotto `/System/`, `/usr/libexec/`, `/usr/sbin/`, `/sbin/` |
| Sessione/parent | Session 0 | ppid==1 **e** path di sistema |
| Nome | `svchost, csrss, wininit, services, lsass…` | `kernel_task, launchd, WindowServer, mds…` |

`isSystem = true` se (utente di sistema) **oppure** (path di sistema **e** nome in lista). UI: system nascosti di default nella sezione porte con toggle "mostra anche sistema" (visibili ma mai killabili).

### Servizi online: ICMP vs TCP vs HTTP

- **ICMP: no.** Raw socket richiede privilegi; molti servizi filtrano ICMP; "pinga Netflix" non dice se Netflix funziona.
- **HTTP(S) GET/HEAD: default.** Preset: Google `https://www.gstatic.com/generate_204` (expect 204), Cloudflare `https://one.one.one.one/cdn-cgi/trace`, WhatsApp `https://www.whatsapp.com` (HEAD), Telegram `https://core.telegram.org`, Netflix `https://www.netflix.com` (HEAD, follow redirect), Amazon/Prime `https://www.amazon.it` (HEAD), iCloud `https://www.icloud.com`. Timeout 4s, 2 tentativi, `degraded` se latenza > soglia.
- **TCP connect: per servizi non-HTTP** (SSH, porte custom) e come fallback quando un sito risponde 403 ai client non-browser (User-Agent da browser nei preset riduce il problema).
- **Servizi dietro cloudflared**: HTTPS GET sull'hostname pubblico, meglio verso un endpoint `/healthz`. Nota UI: il check attraversa Cloudflare, quindi verifica *tunnel+origine*; per distinguerli, secondo check TCP verso l'IP LAN del server.
- Check in parallelo, attivi solo con sezione aperta (garantito dal PollerRegistry), intervallo default 15s.

---

## 7. Piano di sviluppo

Vedi [PLAN.md](PLAN.md).

---

## 8. Funzionalità utili da aggiungere

In ordine di rapporto utilità/costo:

1. **"Chi occupa la porta che mi serve" come azione**: campo "porta" in alto nella sezione porte → risposta immediata + kill. È il gesto n.1 di un dev.
2. **Process log viewer per i task avviati dal tool**: stdout di `npm start` con ANSI colors e bottone Stop (v2, ma è la killer feature del quotidiano).
3. **Quick actions da tray/menubar**: "kill porta 3000", "apri ultimo progetto", "CPU top 3" senza aprire la finestra.
4. **Deep-link mobile**: home-screen shortcut del telefono che apre direttamente la sezione servizi.
5. **Clipboard di rete**: textbox condivisa desktop↔telefono via WS. Costo ~mezza giornata con questa architettura.
6. **Rilevamento "porta zombie"**: dev server morto male che tiene la porta, badge dedicato.
7. **Env inspector per progetto**: nomi delle chiavi dei file `.env*` presenti (mai i valori) e quale verrebbe caricato.
8. **Docker awareness (v2+)**: container con porte pubblicate nella sezione porte con azione "stop container" invece del kill di com.docker.backend.
9. **Profili di avvio composito**: "avvia progetto X" = `docker compose up -d` + `pnpm dev` + apri browser. Solo composizione di azioni già esistenti nel tool.

Cosa **non** aggiungere mai: metriche storiche elaborate (esiste Grafana), gestione completa di git (esistono i client git), editor di file (esiste VS Code). Il tool vince se resta il pannello che si guarda 20 volte al giorno per 10 secondi.
