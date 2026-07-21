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

### Extra post-v1 (17/07/2026, richieste d'uso)
- [x] Monitor dischi nella dashboard: spazio usato/libero con barra colorata, rilevamento removable, aggiornamento a 10s (insert/remove riflessi automaticamente); default refresh dashboard portato a 10s
- [x] Espulsione e formattazione dischi rimovibili (`adapters/disks.rs`): eject + format con opzioni (filesystem, nome, formattazione a basso livello/ripartizione) e conferma digitando il nome; solo da localhost, mai da remoto; il disco di sistema è escluso a livello di guardia. macOS via `diskutil`, Windows best-effort via PowerShell (non testato su HW), Linux non supportato per format
- [x] Anti-inattività (`jiggler.rs`): dopo 5 min di inattività reale muove il mouse di 1px ogni 3 min; se l'utente torna attivo si ferma; toggle in Impostazioni; idle via ioreg (mac)/GetLastInputInfo (win), movimento via enigo (mac/win). Nota: su macOS serve il permesso Accessibilità
- [x] Tema chiaro/scuro/auto (segue il sistema), selettore in Impostazioni, persistito in localStorage
- [x] Toggle switch al posto delle checkbox per controllo remoto e anti-idle; QR di abbinamento in popup centrato
- [x] Cambio tema con transizione morbida di 1s (classe temporanea, così hover/interazioni restano istantanei)
- [x] Anti-idle: se manca il permesso Accessibilità (macOS) l'utente viene avvisato con un banner e uno shortcut "Apri Accessibilità" (deep-link a Impostazioni di Sistema) + "Ricontrolla" (`adapters/accessibility.rs`, FFI AXIsProcessTrusted)
- Nota di verifica: eject/format non sono stati eseguiti su un disco rimovibile reale (nessuno collegato); testate le guardie (rifiuto su disco di sistema/inesistente, blocco remoto) e la costruzione dei comandi. Il jiggler è stato verificato solo nella logica e nel rilevamento idle, non lasciando la macchina ferma 5 min.

### Fix da test reale su Windows (21/07/2026)
- [x] **VS 2026 non rilevata**: `vswhere` senza `-prerelease` esclude le edizioni Preview/Insiders — VS 2026 in preview risultava "non trovato". Aggiunto il flag in `discover_visual_studio` ([adapters/tools.rs](src-tauri/src/adapters/tools.rs)).
- [x] **RAM sottostimata rispetto al Task Manager**: la vista "processi pesanti" filtrava per singolo PID, ma app multi-processo (VS Code, Chrome, Docker Desktop) si spezzano in decine di processi leggeri che sommati pesano molto. Riscritta come aggregazione per nome eseguibile (`ProcessGroup` in [adapters/procs.rs](src-tauri/src/adapters/procs.rs)): la soglia si applica al totale del gruppo, righe espandibili per il dettaglio PID-per-PID. Verificato dal vivo: "Code Helper" ×2/×5/×5 sommati a ~2.5GB, in linea coi "quasi 3GB" osservati.
- [x] **Drop non vede altre istanze**: non era un problema di permessi di rete ma di architettura — ogni processo RickyDEVTool è un hub Drop isolato con registro peer proprio; due computer diversi (o due istanze) non avevano modo di scoprirsi. Aggiunta discovery cross-host via beacon UDP broadcast (porta 51969, ogni 5s, TTL 22s) in [services/hubdiscovery.rs](src-tauri/src/services/hubdiscovery.rs): ogni hub ha un'identità stabile persistita (`drop_hub_id`) sempre riconosciuta come "il desktop" di quella macchina, anche senza browser aperti. Gli hub remoti compaiono nella lista peer con badge "altra rete"; invio file/testo verso un hub remoto passa da un proxy HTTP ([server/mod.rs](src-tauri/src/server/mod.rs) `proxy_send_file`/`proxy_send_text`). Fiducia hub-to-hub basata sulla discovery UDP reciproca (header `X-RickyDev-Hub-Id` verificato contro IP+hub_id già beaconato), non sul pairing dell'utente. Verificato end-to-end simulando due istanze isolate (config/hub_id/Downloads separati): discovery reciproca, testo e file trasferiti con contenuto integro, e rifiuto confermato per IP non verificati o hub_id inventati/mai visti.
- Nota Windows: il firewall potrebbe comunque chiedere conferma alla prima apertura delle porte TCP 6969 e UDP 51969; su reti "pubbliche" senza permessi amministrativi il blocco può restare silenzioso (nessun errore visibile). È un limite del sistema operativo, non risolvibile lato app senza privilegi elevati.

### Tray: menu contestuale (21/07/2026)
- [x] Menu del tray riscritto in un modulo dedicato ([tray/mod.rs](src-tauri/src/tray/mod.rs) + [tray/snapshot.rs](src-tauri/src/tray/snapshot.rs)), con le sezioni: Sistema (CPU/RAM + dischi, eject rimandato all'app), Porte (per porta/processo), Servizi (stato+latenza), Rete (scansione LAN), Drop (per dispositivo: invia file/testo), Abbinamento (QR + toggle controllo remoto), Anti-inattività, Strumenti rilevati (apertura diretta per vscode/visualstudio/terminale, elenco informativo per le CLI).
- [x] I dati vengono da uno snapshot in background (loop indipendente dal `PollerRegistry`, sempre attivo mentre l'app gira: CPU/RAM/porte ogni 3s, dischi ogni 3s, servizi ogni ~21s, strumenti ogni ~60s) — mai uno scan sincrono al click, il check servizi da solo può costare fino a qualche secondo. Il menu si ricostruisce e si riapplica (`set_menu`) ogni 3s.
- [x] Interazioni che un menu nativo non può offrire (testo libero, immagini) portano in primo piano la finestra e la navigano già sulla sezione giusta via evento Tauri `tray-navigate` (frontend: `trayIntentStore` + reazioni in Settings/Drop/NetTools). "Invia file…" usa invece un dialog nativo (`tauri-plugin-dialog`) e invia direttamente, senza aprire l'app.
- [x] Eject disco resta sempre rimandato all'app (mai diretto dal tray); il kill di un processo da "Porte" invece ora ha conferma nativa diretta (vedi fix del 21/07 sotto) — solo i processi protetti (typed-confirm) restano rimandati all'app. Refactor di supporto: `proxy_send_file`/`proxy_send_text` spostati da `server/mod.rs` a metodi di `DropService` ([services/drop.rs](src-tauri/src/services/drop.rs)), così l'invio da tray riusa la stessa logica dell'upload via browser; aggiunto `send_local_file` (copia diretta su disco per i peer locali, proxy HTTP per gli hub remoti).
- Nota di verifica: build + 54 test Rust + typecheck/build frontend verdi; avvio via `cargo tauri dev` verificato senza panic/errori nei log per ~20s (copre più cicli di refresh/rebuild). Non verificato a occhio il contenuto del menu nativo stesso (click sulle voci, nesting, icone dei processi): non ho un modo di interagire con la menu bar di macOS da qui — vale la pena un giro manuale rapido prima di considerarlo definitivo.

### Fix da test reale del tray + rete (21/07/2026)
- [x] **Menu del tray si chiudeva da solo dopo pochi secondi (anche coi sottomenu aperti)**: causa un rebuild periodico su timer che chiamava `set_menu` mentre il menu era aperto, interrompendo il tracking nativo dell'OS. Rimosso il timer: il rebuild ora avviene solo su `TrayIconEvent::Enter` (il mouse entra sempre nell'icona prima del click), mai durante l'apertura stessa.
- [x] **Voci "Porte"/"Rete" del tray non navigavano mai alla sezione giusta**: root cause non erano gli eventi in sé ma la capability Tauri — il contenuto della finestra è sempre caricato da un URL "remote" (`http://127.0.0.1:PORT`, mai `tauri://`), e senza un campo `"remote"` nella capability l'intero bridge IPC (incluso `listen()`) veniva rifiutato in silenzio (la finestra si apriva comunque via chiamata Rust diretta, ma l'evento non arrivava mai). Aggiunto `"remote": { "urls": ["http://127.0.0.1:*", "http://localhost:1420"] }` in [capabilities/default.json](src-tauri/capabilities/default.json).
- [x] **Kill di un processo direttamente dal tray**: sottomenu "Porte" ora elenca ogni processo con "Termina" (dialog nativo di conferma, non il typed-confirm dell'app) per i processi normali; quelli di sistema restano non terminabili, quelli protetti (typed-confirm) rimandano all'app.
- [x] **Scansione LAN: "The string did not match the expected pattern."**: il bottone chiamava l'endpoint con GET ma la route è POST-only → risposta 405 non-JSON → `JSON.parse` falliva con quel messaggio (testo esatto di WebKit/Safari, il motore della webview su macOS). Corretto in `Scan()` ([NetTools.tsx](src/features/nettools/NetTools.tsx)).
- [x] **Dashboard: la lista dischi non si aggiornava dopo un eject** finché non si usciva e rientrava dalla sezione. Ora eject/format rileggono subito `/api/disks` e aggiornano lo store invece di aspettare il prossimo giro del poller (10s) — comunque non sempre sufficiente secondo quanto osservato.
- [x] **Servizi online non modificabili dopo l'aggiunta**: cliccare una riga (non-preset) in "Configurazione" ora precompila il form e cambia "Aggiungi" in "Salva"/"Annulla"; l'update riusa l'`id` esistente (già supportato dal backend) preservando `enabled`.
- [x] **Toolbox di rete**: DNS ora interroga anche SOA/CAA/SRV/TLSA oltre ad A/AAAA/CNAME/MX/TXT/NS (verificato dal vivo che il multi-MX funzionava già: yahoo.com→3 record, i domini con "un solo MX" ne hanno realmente uno). Ping ora fa 10 tentativi mostrati in tempo reale (una richiesta `count:1` alla volta) con barra di progresso invece di aspettare un unico risultato aggregato da 4. Scan porte: risultati precedenti cancellati subito a ogni nuova ricerca, porte duplicate deduplicate, barra di progresso (batch da 500, limite per chiamata alzato da 50 a 1000 con concorrenza interna limitata a 200), bottoni "Porte note" e "Tutte le porte" (1-65535, solo le aperte mostrate come pillole oltre le 100 per non intasare il DOM), cronologia delle ultime 5 ricerche cliccabile. Aggiunto Traceroute (streaming via l'infrastruttura task già usata da Node/.NET, reverse DNS disattivo di default con opzione per riattivarlo).
- Nota di verifica: build + test Rust verdi, `cargo tauri dev` senza panic; endpoint verificati via curl diretto (portcheck con dedupe/limite, traceroute, scan). Il fix del bridge IPC risolve la causa più probabile della mancata navigazione ma non è stato possibile confermarlo cliccando davvero il tray da qui — da riverificare all'uso.

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
