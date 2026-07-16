# PLAN — piano di sviluppo

Riferimento architettura e feature: [PROJECT.md](PROJECT.md). Dubbi aperti: [QUESTIONS.md](QUESTIONS.md).

Stime in giornate effettive di lavoro, con margine per l'apprendimento di Rust incluso nelle voci più grosse.

## Milestone

### M0 — Scheletro (3–5 gg)  ← COMPLETATA (15/07/2026, verificata end-to-end)
- [x] Progetto Tauri 2 + workspace Rust (`src-tauri`) + frontend Vite/React/TS
- [x] Server axum su :6969 (fallback 6970-6978), SPA embedded con `rust-embed`
- [x] WebSocket con multiplexing per topic + subscribe/unsubscribe
- [x] PollerRegistry (start/stop per subscriber count, intervallo runtime, skip post-sleep)
- [x] Config file (`dirs::config_dir()/RickyDEVTool/config.json`), scrittura atomica
- [x] Auth middleware: localhost libero, LAN con pairing token; QR + lista IP (statiche libere, /api e QR protetti)
- [x] Tray icon con "Apri", indirizzo LAN, "Esci" (chiudere la finestra nasconde, il server resta su)
- [x] Logging `tracing` su file rotante + endpoint /api/log per errori frontend
- [x] Envelope errori `ApiResult<T>` (tipi TS manuali in `src/lib/types.ts`; ts-rs rimandato a M1 quando i modelli crescono)
- [x] PATH fix per app GUI macOS (login-shell PATH all'avvio)
- [x] Bonus M1 anticipato: collector `stats` reale (sysinfo: CPU totale/per-core, RAM, swap) per validare la pipeline poller→bus→WS→UI

### M1 — Dashboard risorse (3–4 gg) ← COMPLETATA (15/07/2026)
- [x] Collector stats con `sysinfo` (CPU totale, per core, RAM, swap) — anticipato in M0
- [x] Normalizzazione CPU uniforme Win/mac (per-processo: /num core); scarto campione post-wake nel poller
- [x] Ring buffer 60 campioni per sparkline (lato client, topic `stats`)
- [x] UI: gauge CPU/RAM, griglia core, sparkline, selettore intervallo 0.5–10s — anticipato in M0
- [x] "Processi pesanti" on-demand con soglie configurabili (default >20% CPU, >10% RAM, filtro OR), doppio campionamento a 300ms, badge app note e "sistema" (`adapters/procs.rs`, riusabile in M2)
- [x] Pannello destro "vital signs" sempre visibile + versione mobile — anticipato in M0
- Nota: kill dalla tabella arriva con la M2 (userà il ProcessKiller della sezione porte). ts-rs rimandato a M4, quando i modelli git/progetti crescono davvero.

### M2 — Porte + kill (4–6 gg) ← COMPLETATA (15/07/2026)
- [x] Adapter `PortScanner`: macOS `lsof -FpnP` (parser testato su fixture), Windows `netstat -ano` (parser testato su fixture; scelto al posto di GetExtendedTcpTable: niente FFI non testabile da mac)
- [x] Classificazione system/non-system (euristica per OS, riusata da M1)
- [x] Adapter `ProcessKiller`: SIGTERM→SIGKILL dopo 5s su mac, taskkill (/F /T per force) su Win; verifica PID+nome+startTime contro il riuso dei PID
- [x] Lista protetti → typed-confirm lato server (non solo UI); system → mai killabili; kill da LAN → REMOTE_FORBIDDEN finché non esiste il toggle remote control (v1)
- [x] UI: tabella per porta, riga espandibile con processi/azioni (copia, apri nel browser), dialog kill con confirm/typed-confirm e force
- [x] Icone/etichette processi noti (16 regole statiche, condivise con M1)
- [x] Polling attivo solo a sezione aperta (verificato: topic `ports` via WS)
- Nota: hover-submenu del context menu rimandato a rifinitura v1 (le righe espandibili funzionano anche su mobile). UDP non incluso (solo TCP LISTEN).

### M3 — Launcher + discovery tool (2–3 gg) ← COMPLETATA (15/07/2026)
- [x] `AppLocator` (`adapters/tools.rs`): VS Code (bundle mac / path noti win / PATH), Visual Studio via vswhere con edizioni (Win-only, nota esplicita su mac), git, node, npm, yarn, pnpm, dotnet, docker, terminale (iTerm/Terminal, wt/cmd)
- [x] Versioni via `--version` con timeout; cache in memoria + refresh manuale; override path persistito in config (vince sulla discovery, invalida la cache)
- [x] Launch di vscode/visualstudio/terminale con target opzionale; negato dalla LAN (REMOTE_FORBIDDEN)
- [x] UI: pannello "Strumenti rilevati" in Impostazioni con badge trovato/assente, fonte, versione, bottoni Apri e Path…

### M4 — Cartelle + Git base (4–5 gg) ← COMPLETATA (16/07/2026) — **fine MVP**
- [x] Selezione cartella: browser lato backend (`/api/fs/dirs`) invece del dialog nativo Tauri — funziona identico da desktop e da telefono; cartelle pinnate persistite in config (max 12)
- [x] Scanner progetti: `.git` (dir o file), `package.json`, `.sln`/`.slnx`/`.csproj`; depth 3, ignore list, limite 5000 dir con flag `truncated`; dentro un repo git non si cercano altri repo
- [x] Azioni comuni: apri in VS Code, apri terminale, copia path (file manager rimandato: le prime tre coprono l'uso reale)
- [x] GitService via CLI: stato da `status --porcelain=v2 --branch` (branch/detached, dirty, ahead/behind), warnings (no-upstream, diverged, merge-in-progress, stale-fetch), Fetch `--prune`, Pull `--ff-only` (mai merge automatici); `GIT_TERMINAL_PROMPT=0` + SSH BatchMode → mai prompt appesi, errori auth mappati su GIT_AUTH_FAILED; azioni di rete loopback-only
- [x] UI: lista progetti + pannello dettaglio con badge tipo, stato git, Fetch/Pull (pull disabilitato se dirty)

### M5 — Git completo + Node (4–5 gg) ← COMPLETATA (16/07/2026)
- [x] Dropdown branch con `for-each-ref` (hash/data/autore/subject), locali + remote-only (origin/x senza locale), stale ≥4 settimane colorati ambra
- [x] Checkout (bloccato se dirty con spiegazione; remote-only → nome corto, git crea il tracking), loopback-only
- [x] NodeService: detection npm/yarn/pnpm (override > lockfile > packageManager > npm "assunto"), override per progetto persistito in config
- [x] Task runner generico (`tasks.rs`, riusato in M6): stream stdout/stderr riga per riga sul topic WS `task:{id}`, process group su unix / cmd+taskkill su Windows, Stop con SIGTERM→SIGKILL 3s, eventi exit
- [x] UI: BranchPicker a due righe, NodePanel (pm badge cliccabile, Install, Start su start/dev/serve, dropdown altri script), TaskLog inline con autoscroll e Stop
- Nota: capacità bus eventi portata a 1024; il WS ora accetta topic non-poller (task:*).

### M6 — .NET (4–5 gg) ← COMPLETATA (16/07/2026)
- [x] Parser `.sln` classico **e** `.slnx` (nuovo default di dotnet 10, verificato su output reale) + `.csproj` (OutputType/Sdk Web/Worker, TFM singolo e multipli) + `launchSettings.json` (commandName, applicationUrl)
- [x] Startup project persistito in config (auto-selezionato se un solo eseguibile), profilo persistito (auto: primo commandName==Project); profili IISExpress selezionabili solo su VS/Windows (disabled con nota)
- [x] Run (`dotnet run --project X --launch-profile P`), Rebuild (`-t:Rebuild`), Clean via task runner con log stream e Stop
- [x] Open in VS sulla solution (Win-only; disabilitato su mac con tooltip, OS letto da /api/health)
- Verificato end-to-end su solution reale (console + classlib): info, run con Hello World streammato, rebuild.

### M7 — Servizi online + alerts + remote control (3–4 gg) ← COMPLETATA (16/07/2026)
- [x] ServicesMonitor: check HTTP (GET con UA browser, expectStatus o 200-399, degraded su status inatteso o latenza >2.5s) e TCP connect, paralleli via JoinSet, history ultimi 20 esiti
- [x] 8 preset pubblici integrati in config al primo avvio (nuovi preset futuri compaiono da soli); custom add/toggle/delete da UI, con nota cloudflared (tunnel+origine)
- [x] Attivo solo a sezione aperta (topic `services`, 15s)
- [x] AlertService su eventi bus: service-down (solo transizione), cpu-sustained >90% per 60s, mem-high >92%, task-failed; dedup con cooldown 10 min; lista nel pannello vitals con ack al click (toast/tray rimandati a rifinitura)
- [x] Toggle "Remote control" (default OFF, attivabile solo da localhost): tutte le azioni di scrittura passano da un'unica guardia `write_allowed`

### M8 — Packaging/release (3–5 gg) ← COMPLETATA in locale (16/07/2026); CI da attivare con un remote
- [x] tauri-bundler verificato in locale su mac: `RickyDEVTool.app` (18 MB) + `.dmg` (5.9 MB); NSIS/.deb prodotti dalla CI sui rispettivi runner
- [x] Workflow GitHub Actions (`.github/workflows/build.yml`): test matrix 3 OS su ogni push, bundle + release draft su tag `v*` (mac universal); serve creare il repo remoto GitHub e pushare per attivarlo
- [ ] Firma: rimandata (Q9: uso personale, niente account Apple Dev / certificato) — su mac Gatekeeper mostrerà l'avviso "sviluppatore non verificato" (aggirabile con click destro > Apri), su Windows SmartScreen
- [ ] Chiavi updater: rimandate a v2 insieme all'updater

### v2 (dopo rilascio v1)
- Lista commit selezionabile + checkout detached HEAD
- Start/Stop Debug via VS DTE (Windows-only, fragile — accettato)
- Log viewer persistente dei task
- Auto-updater
- Storico metriche 24h su SQLite
- Docker awareness, profili di avvio composito, clipboard di rete

## Ordine e razionale
M0→M1→M2 anticipano tutta l'infrastruttura e il rischio più alto (permessi kill, parsing porte). M3 è piccola e sblocca M4. MVP = fine M4: il tool è già utile ogni giorno. M5–M7 completano la v1. M8 solo quando c'è qualcosa che vale la pena distribuire.

## Rischi tecnici principali
1. **Permessi kill/enumerazione** (M2): exe path illeggibili su mac, processi elevati su Win → errori tipizzati con `osHint`, mai crash; test manuali con processi root/elevati.
2. **Precisione CPU per processo**: primo campione da scartare; normalizzazione da validare su entrambi gli OS.
3. **Parsing lsof/sln/launchSettings**: fixture reali in repo, test puri.
4. **PATH mancante nelle app GUI macOS**: workaround in M0, non dopo.
5. **Notarizzazione macOS**: dipende da account Apple Developer (Q9).

## Test strategy
- **Unit (Rust)**: servizi contro adapter mock (trait objects); parser contro fixture in `fixtures/`.
- **Contract test per OS**: suite `#[ignore]` in CI su runner reali (windows-latest, macos-latest): PortScanner trova un listener aperto dal test, kill di un processo figlio, vswhere parse.
- **Frontend**: vitest sugli store (riduzione eventi WS); Playwright smoke su SPA con backend fake (dashboard, context menu porte, checkout, kill confirm).
- **Checklist manuale pre-release** (~10 min): kill di processo elevato, pairing da telefono reale, sleep/wake, repo con auth SSH.

## Packaging/release

| Target | Formato | Note |
|---|---|---|
| Windows | NSIS `.exe` (+ `.msi` opz.) | Firma Authenticode consigliata; senza, avviso SmartScreen documentato |
| macOS | `.dmg` universal (aarch64+x86_64) | Notarizzazione per distribuzione; codesign ad-hoc per uso personale |
| Linux | `.deb` + AppImage | Best-effort, solo CI |
