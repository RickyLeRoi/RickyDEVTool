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

### M1 — Dashboard risorse (3–4 gg)
- [ ] Adapter `SysStatsProvider` con `sysinfo` (CPU totale, per core, RAM, swap)
- [ ] Normalizzazione CPU uniforme Win/mac; scarto primo campione e post-wake
- [ ] Ring buffer 60 campioni per sparkline (topic `stats`)
- [ ] UI: gauge CPU/RAM, griglia core, sparkline, selettore intervallo 0.5–10s
- [ ] "Processi pesanti" on-demand con soglie configurabili (>20% CPU, >10% RAM)
- [ ] Pannello destro "vital signs" sempre visibile + versione mobile

### M2 — Porte + kill (4–6 gg) — rischio tecnico più alto, per questo presto
- [ ] Adapter `PortScanner`: macOS `lsof -nP -iTCP -sTCP:LISTEN` (parser con fixture), Windows `GetExtendedTcpTable`
- [ ] Classificazione system/non-system (euristica per OS)
- [ ] Adapter `ProcessKiller`: SIGTERM→SIGKILL su mac, TerminateProcess/taskkill su Win; verifica PID+nome+startTime
- [ ] Lista protetti → typed-confirm; system → mai killabili
- [ ] UI: tabella per porta, context menu con submenu processi, conferme
- [ ] Icone processi noti (~15 regole statiche)
- [ ] Polling attivo solo a sezione aperta (già dal PollerRegistry)

### M3 — Launcher + discovery tool (2–3 gg)
- [ ] `AppLocator`: VS Code (path noti/registry/PATH), Visual Studio via vswhere (Win-only), git, node, dotnet, terminale
- [ ] Cache discovery + refresh manuale; override path da Impostazioni
- [ ] UI launcher + badge "non trovato"/"Windows-only"

### M4 — Cartelle + Git base (4–5 gg) — fine MVP
- [ ] Dialog nativo selezione cartella, cartelle pinnate persistite
- [ ] Scanner progetti: `.git` (dir o file), `package.json`, `.sln`/`.csproj`; depth 3, ignore list
- [ ] Azioni comuni: apri in VS Code, apri terminale, apri file manager, copia path
- [ ] GitService via CLI: stato (branch, dirty, ahead/behind), Fetch, Pull
- [ ] UI explorer: albero lazy + pannello dettaglio + badge tipo progetto

### M5 — Git completo + Node (4–5 gg)
- [ ] Dropdown branch con `for-each-ref` (hash/data/autore), stale ≥4 settimane colorati
- [ ] Checkout (bloccato se dirty, con spiegazione), warnings (diverged, no-upstream, detached)
- [ ] NodeService: detection npm/yarn/pnpm (lockfile > packageManager > default), override
- [ ] Task runner: Install/Start/script con stream stdout su WS, Stop = kill tree
- [ ] Pannello log inline nella UI

### M6 — .NET (4–5 gg)
- [ ] Parser `.sln` + `.csproj` (OutputType, Sdk, TFM) + `launchSettings.json`
- [ ] Scelta startup project persistita, dropdown profili (badge Win-only su IISExpress)
- [ ] Run (`dotnet run --launch-profile`), Stop, Rebuild, Clean via task runner
- [ ] Open in VS (`devenv.exe <sln>`, Win-only, disabilitato su mac con tooltip)

### M7 — Servizi online + alerts + remote control (3–4 gg)
- [ ] ServicesMonitor: check HTTP (HEAD/GET, expectStatus) e TCP, paralleli, timeout 4s
- [ ] Preset pubblici + servizi custom da config (inclusi hostname cloudflared)
- [ ] Attivo solo a sezione aperta, intervallo default 15s
- [ ] AlertService: cpu-sustained, mem-high, service-down, task-failed → toast + badge tray
- [ ] Toggle "Remote control" per azioni distruttive da LAN

### M8 — Packaging/release (3–5 gg)
- [ ] tauri-bundler: NSIS .exe (Win), .dmg universal (mac), .deb + AppImage (Linux CI-only)
- [ ] CI GitHub Actions, matrix 3 OS, artifact su tag `v*`
- [ ] Firma: Authenticode (se certificato disponibile), notarizzazione mac (se account Apple Dev) — vedi QUESTIONS Q9
- [ ] Chiavi updater generate ora anche se l'updater arriva in v2

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
