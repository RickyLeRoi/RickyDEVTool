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

### Fase 2 — primo blocco (22/07/2026)
Sviluppati 3 dei 6 item v2 (i due infra-dipendenti — auto-updater e Debug VS DTE — rimandati perché richiedono firma/remote e Windows; il log viewer persistente non prioritizzato in questo giro).
- [x] **Git: lista commit + checkout detached HEAD**. Backend ([services/git.rs](src-tauri/src/services/git.rs)): `commits(path, limit, skip)` via `git log --format` con campi separati da `\x1f` (hash pieno/corto, autore, email, data, subject, decorazioni `%D` ripulite da "HEAD ->"); `checkout_commit(path, hash)` valida l'hash come esadecimale, rifiuta se dirty, un hash grezzo mette git in detached HEAD. Endpoint `GET /api/git/commits` e `POST /api/git/checkout-commit` (guardia `write_allowed`). UI: [CommitList.tsx](src/features/projects/CommitList.tsx) sotto il BranchPicker in GitPanel — lista paginata ("Carica altri", 50/pagina), badge per le decorazioni branch/tag, Checkout per-commit disabilitato se dirty. Verificato end-to-end su repo temporaneo: detached HEAD reale confermato da `git status`.
- [x] **Storico metriche 24h su SQLite**. Nuova dip `rusqlite` (feature `bundled`: compila la sua SQLite, niente lib di sistema). [services/metrics.rs](src-tauri/src/services/metrics.rs): campionamento **sempre attivo** (thread OS dedicato, non il PollerRegistry che gira solo a UI aperta) ogni 30s di CPU/RAM/disco-di-sistema in `data_dir/metrics.db` (WAL), retention 25h con DELETE a ogni insert; query dagli handler async via `spawn_blocking` sullo stesso `Mutex<Connection>`. Endpoint `GET /api/metrics/history?hours=N`. UI: [MetricsHistory.tsx](src/features/dashboard/MetricsHistory.tsx) in dashboard — grafico SVG a 3 linee (CPU/RAM/disco) con selettore 1h/6h/24h, downsampling a media-bucket (max 400 punti), colori dal tema esistente (`--accent`/`--accent2`/`--ok`), refresh 30s. Verificato dal vivo: campioni scritti a intervalli di 30s esatti, query per finestra funzionante.
- [x] **Docker awareness**. [adapters/docker.rs](src-tauri/src/adapters/docker.rs): rileva la CLI `docker`, `state()` distingue non-installato / demone-giù (euristica multi-runtime su stderr: Docker Desktop, colima/rootless via `docker.sock`) / lista container da `docker ps -a --format '{{json .}}'` (parser puro testato su fixture: id, nome, immagine, stato, status umano, porte); `action(id, start|stop|restart)` e `logs_command` validano l'id container (niente flag/metacaratteri). Aggiunto anche `images()` (`docker images`, parser testato) → endpoint read-only `GET /api/docker/images`. Endpoint `GET /api/docker`, `POST /api/docker/{id}/action` (write), `POST /api/docker/{id}/logs` (stream via task runner, come traceroute). UI: nuova sezione [Docker.tsx](src/features/docker/Docker.tsx) nel rail (🐳) — tabella container con pallino di stato, azioni start/stop/restart, log in streaming (fermati alla chiusura del pannello), pannello immagini collassabile, refresh 5s. Verificato dal vivo: `available:true`/`daemonDown:true` corretto con Docker Desktop spento su questa macchina, `/api/docker/images` risponde `[]` col demone giù.
- [x] **Log viewer persistente dei task** (aggiunto nello stesso blocco, dopo i 3 sopra). Prima l'output di un task (npm/dotnet/traceroute/docker logs) viveva solo nello stato React del pannello aperto: chiuso quello, perso. Ora [tasks.rs](src-tauri/src/tasks.rs) bufferizza ogni riga per task (`LogLine`, ring buffer 5000 righe) riempito da `stream_lines`; `TaskRegistry::log(id)` lo espone, endpoint `GET /api/tasks/{id}/log` (404 tipizzato se ripulito). Nuova sezione "Task" nel rail (🧾, [tasks/Tasks.tsx](src/features/tasks/Tasks.tsx)): lista dei task attivi+terminati aggiornata live dal topic WS `tasks`, click per aprire il log (con "Pulisci terminati"). [TaskLog.tsx](src/components/TaskLog.tsx) fa backfill dal buffer all'apertura, così si rivede tutto l'output anche di un task già finito, riaperto da qualsiasi punto. Beneficia retroattivamente Node/.NET/traceroute/docker-logs (tutti passano dal task runner). Verificato dal vivo: traceroute bufferizzato e riletto via `/api/tasks/{id}/log` dopo la fine, 404 corretto per id inesistente.
- Nota di verifica: 65 test Rust verdi (12 nuovi tra i 3 blocchi + log buffer), typecheck+build frontend verdi, binario reale avviato e tutti i nuovi endpoint interrogati via curl (commits reali di questo repo, checkout detached su repo temp confermato da `git status`, campioni metriche a 30s esatti, stato docker con demone giù, buffer log task dopo la fine). Non verificate a occhio le UI nella webview (grafico metriche, tabella docker con container reali, sezione Task) né le azioni docker su container attivi (demone spento qui).

### Progetti multi-ecosistema (22/07/2026)
Estesa la pagina Progetti oltre Node.js/.NET: ora riconosce e opera anche **Python, Rust, Tauri, Flutter** (richiesta d'uso).
- [x] **Detection** ([services/projects.rs](src-tauri/src/services/projects.rs)): nuovi `ProjectKind` Python/Rust/Tauri/Flutter. Marker: Python = uno tra `pyproject.toml`/`requirements.txt`/`setup.py`/`Pipfile`/`manage.py`; Rust = `Cargo.toml`; Tauri = `src-tauri/tauri.conf.{json,json5,toml}`; Flutter = `pubspec.yaml`. Il crate `src-tauri` di un progetto Tauri non viene ri-scoperto come progetto Rust a sé (skip della ricorsione in quella sottocartella).
- [x] **Runner generico** ([services/runners.rs](src-tauri/src/services/runners.rs)): invece di duplicare il pattern node/dotnet ×4, un solo modulo genera per `(kind, path)` la lista di azioni già risolte in `(program, args)` verificati. Python: crea/ricrea venv, install (uv sync / poetry / pipenv / pip su `.venv` se presente, con requirements o `-e .`), run (entrypoint `manage.py runserver`/`main.py`/`app.py`, sotto `poetry|uv|pipenv run` quando serve), build. Rust: fetch/build/build --release/run (solo se c'è un binario)/test/clean. Tauri: install frontend + `tauri dev`/`build` via pm del frontend (npx/pnpm/yarn) o `cargo tauri` senza package.json. Flutter/Dart: pub get, run, build apk/web + desktop host, test, clean.
- [x] **Sicurezza**: endpoint `GET /api/runner/info?kind=&path=` e `POST /api/runner/run` (write-guarded). Il client manda solo l'`actionId`; il server rigenera lo spec e ritrova l'azione — mai un comando arbitrario dal chiamante (verificato: un `actionId` con metacaratteri viene rifiutato). UI: pannello generico [RunnerPanel.tsx](src/features/projects/RunnerPanel.tsx) montato in [Projects.tsx](src/features/projects/Projects.tsx) per ogni kind rilevato, con badge, note (tool/venv/Django/workspace) e TaskLog.
- Verifica: 77 test Rust verdi (11 nuovi), build frontend verde, binario reale interrogato via curl su fixture Python(venv+Django)/Rust/Flutter e sul repo stesso (rilevato git+node+tauri, nessun `src-tauri` duplicato); `cargo clean` reale spawnato e output bufferizzato; injection su `actionId` respinta. Non verificate a occhio le UI nella webview.

### Utility v2: calcolatrice, color picker, storico appunti (22/07/2026)
Tre utility auto-contenute della lista v2 (le altre due — auto-updater e Debug VS DTE — restano bloccate da firma/remote e Windows).
- [x] **Storico appunti** (stile Windows+V). Adapter OS via CLI ([adapters/clipboard.rs](src-tauri/src/adapters/clipboard.rs)): lettura/scrittura con `pbpaste`/`pbcopy` (macOS) e PowerShell `Get-Clipboard -Raw`/`Set-Clipboard` (Windows); la scrittura passa il testo via **stdin**, mai come argomento (niente injection). Servizio ([services/clipboard.rs](src-tauri/src/services/clipboard.rs)): thread campionatore sempre attivo ogni 1.5s, storico **solo in memoria** (mai su disco: contiene password/token), dedup consecutivo, ri-copia che risale in cima senza duplicare, pin che protegge dall'eviction (max 100 non fissate), pausa/ripresa cattura, svuota (con/senza fissati). Endpoint `GET /api/clipboard/history`, `POST .../copy` (write-guarded), `/pin`, `/delete`, `/clear`, `/enabled`. UI: sezione 📋 [Clipboard.tsx](src/features/clipboard/Clipboard.tsx) con lista, copia, pin, elimina, pausa, note privacy. Verificato dal vivo: copie esterne catturate, ri-copia riscrive la clipboard di sistema, pin/clear/pausa/delete corretti (pausa non cattura).
- [x] **Calcolatrice scientifica**. Motore puro ([features/calc/engine.ts](src/features/calc/engine.ts)): parser a discesa ricorsiva senza `eval`, precedenza convenzionale (`-2^2 = -4`, `^` associativo a destra, `2^-3` valido), funzioni trig/log/√/exp/abs/round in gradi o radianti, costanti π/e/τ, fattoriale, notazione esponenziale. Convertitore basi DEC/HEX/OCT/BIN bidirezionale a precisione arbitraria (BigInt, prefissi 0x/0o/0b). UI: sezione 🧮 [Calc.tsx](src/features/calc/Calc.tsx) con tastiera scientifica, anteprima live, cronologia. Verificato: 36 asserzioni pure via esbuild+node (aritmetica, precedenza, funzioni, errori, conversioni basi, valori >64 bit).
- [x] **Color picker**. Modulo conversioni puro ([features/color/convert.ts](src/features/color/convert.ts)): RGB↔HSV↔HSL↔HEX(+alpha), `parseColor` per #hex(3/4/6/8)/rgb()/rgba()/hsl(). UI: sezione 🎨 [Color.tsx](src/features/color/Color.tsx) con quadrato saturazione/valore + slider tinta e alpha (trascinamento pointer), anteprima su scacchiera, input libero e convertitore RGB/RGBA/HEX/HSL con copia. Eyedropper a bersaglio via `EyeDropper` API **feature-detected**: attivo su WebView2 (Windows), nascosto con nota su WKWebView (macOS, che non la espone). Verificato: 14 asserzioni pure via esbuild+node, round-trip HSV/HSL con errore massimo 0.
- Verifica complessiva: 83 test Rust verdi (6 nuovi clipboard), build+typecheck frontend verdi, logica pura frontend verificata fuori dalla webview (50 asserzioni via esbuild+node), endpoint clipboard interrogati dal vivo. Non pilotabili da qui le UI nella webview (tastiera calcolatrice, trascinamento picker, lista appunti): da provare a mano.

### Git: azioni di riga + fix layout (22/07/2026)
Richiesta d'uso: nella lista branch/commit il pulsante checkout era troppo a destra e su finestra stretta spariva senza scroll. Spostati **tutti i pulsanti all'inizio della riga** (gruppo a larghezza fissa, mai nascosto; il testo che segue si tronca con ellissi invece di spingerli via). Aggiunte nuove azioni.
- [x] **Backend** ([services/git.rs](src-tauri/src/services/git.rs)): `delete_branch(path, branch, force)` (`git branch -d`/`-D`, rifiuta il branch corrente e i nomi che sembrano flag, `--` prima del nome, ritorna la lista aggiornata); `revert_commit` (`git revert --no-edit`, su conflitto `--abort` + messaggio); `cherry_pick_commit` (`git cherry-pick`, idem). Tutte rifiutano il working tree dirty; hash validato esadecimale. Endpoint write-guarded `POST /api/git/delete-branch`, `/revert`, `/cherry-pick`.
- [x] **UI**: [BranchPicker.tsx](src/features/projects/BranchPicker.tsx) — a inizio riga Checkout (⤓) e Elimina (🗑, solo branch locali non correnti); delete con fallback forza se git segnala "not fully merged" (secondo confirm). [CommitList.tsx](src/features/projects/CommitList.tsx) — a inizio riga Checkout (⤓), Revert (↩), Cherry-pick (🍒); revert/cherry-pick ricaricano la lista dall'alto. Nuovo stile `.git-act`/`.git-row-actions` (min-width fissa) e righe con testo troncato.
- Verifica: 86 test Rust verdi (3 nuovi git), build frontend verde. Endpoint provati dal vivo su repo temporaneo: delete di branch mergiato ok, delete del corrente rifiutato, delete non-mergiato → "not fully merged" (poi force ok), cherry-pick applica il file su main, revert lo rimuove con un commit "Revert". Non verificato a occhio il nuovo layout nella webview.

### Git: delete locale/remoto, commit per-branch, vetustà dal remoto (22/07/2026)
Tre affinamenti alle azioni git (richieste d'uso).
- [x] **Delete locale vs remoto con doppia conferma**. `GitBranch` espone ora `remote_ref` (il ref remoto corrispondente, es. "origin/feature") calcolato in `branches()`. `delete_branch` accetta `remote: Option<&str>`: dopo l'eliminazione locale, se richiesto elimina anche dal remoto (`git push <remote> --delete`, timeout di rete, nome remote validato). UI: nuovo [DeleteBranchDialog.tsx](src/features/projects/DeleteBranchDialog.tsx) (overlay/dialog) — se il branch ha un remoto chiede se eliminare solo il locale o anche il remoto; scegliendo il remoto serve una **conferma extra** (checkbox "irreversibile") prima di abilitare il pulsante; gestisce anche il fallback `-D` per i branch non uniti. Il 🗑 in [BranchPicker.tsx](src/features/projects/BranchPicker.tsx) apre il dialog invece di un `confirm()`.
- [x] **Commit del branch cliccato, non di HEAD**. `commits()` accetta `git_ref: Option<&str>` (validato: niente flag/range/caratteri illeciti) e logga quel ref invece di HEAD; endpoint `GET /api/git/commits?...&ref=`. UI: cliccando la riga di un branch ([BranchPicker.tsx](src/features/projects/BranchPicker.tsx) `onSelectBranch`) [GitPanel.tsx](src/features/projects/GitPanel.tsx) imposta `commitsRef` e [CommitList.tsx](src/features/projects/CommitList.tsx) apre e mostra i commit di quel branch (header col nome del ref); il checkout/cambio progetto riazzera a HEAD.
- [x] **Vetustà dal remoto**. In `branches()` lo `stale_weeks` di un branch locale si calcola sulla data dell'ultimo commit **remoto** (`origin/<name>`) quando esiste, non sul tip locale; il commit mostrato resta il tip locale.
- Verifica: 89 test Rust verdi (3 nuovi git: commit per-ref, remote_ref+delete remoto via bare-remote, vetustà dal remoto con tip locale fresco), build frontend verde. Endpoint provati dal vivo con un remote "bare": `remoteRef` popolato, `?ref=feature` mostra i commit di feature (non di main), ref invalido rifiutato, delete locale+remoto rimuove il branch da entrambi. Non verificate a occhio le UI (dialog, selezione branch) nella webview.

### v2 — rimanenti (dopo il primo blocco)
- Start/Stop Debug via VS DTE (Windows-only, fragile — accettato)
- Workflow per git per poter compilare una volta pushata la commit e attivare l'auto-updater
- Auto-updater (richiede chiavi di firma + remote GitHub)
- Profili di avvio composito, clipboard di rete

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
