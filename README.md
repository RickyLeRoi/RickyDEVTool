# RickyDEVTool

Developer operations console locale per macOS e Windows (Linux best-effort). Un solo binario: finestra desktop + web server su **:6969** che serve la stessa UI anche allo smartphone in LAN (pairing via QR).

## Funzionalità

- **Dashboard**: CPU totale + per core, RAM/swap, sparkline, intervallo 0.5–10s, processi pesanti on-demand (aggregati per eseguibile) con soglie; monitor dischi con eject/format dei removibili; storico metriche 24h (CPU/RAM/disco) su SQLite.
- **Porte**: TCP in ascolto di processi non di sistema, raggruppate per porta, kill con conferma (typed-confirm per i processi protetti), badge "zombie" per i listener orfani. Attiva solo a sezione aperta.
- **Progetti**: browser cartelle con pin, riconoscimento git/Node/.NET/Python/Rust/Tauri/Flutter.
  - **Git**: stato, fetch, pull --ff-only, branch con checkout/delete (locale+remoto), lista commit con checkout detached/revert/cherry-pick, stale evidenziati.
  - **Runner**: install/run/build/test per ogni ecosistema con package manager/tool rilevato, log dei task in streaming.
  - Apri in VS Code / terminale, copia path.
- **Servizi online**: health check HTTP/TCP dei preset (Google, Cloudflare, WhatsApp, …) e di servizi personali, solo a sezione aperta.
- **Rete**: scan LAN, port scan, ping, traceroute, DNS/DoH, certificati TLS.
- **Docker**: lista container e immagini, start/stop/restart, log in streaming.
- **Task**: log persistente (ring buffer) di ogni task avviato dal tool, riapribile anche dopo la fine.
- **Utility**: avvii compositi, calcolatrice scientifica, color picker, storico appunti (in memoria) con clipboard di rete.
- **Drop**: invio file/testo tra dispositivi in LAN (e tra host via discovery UDP), toast in ricezione.
- **Alert**: CPU sostenuta, RAM alta, servizio down, task falliti — nel pannello laterale.
- **Mobile**: stessa UI responsive da `http://<ip>:6969`, in sola lettura finché il "Controllo remoto" non viene attivato dal desktop. Deep-link `#/<sezione>` per gli shortcut sulla home.

## Sviluppo

Prerequisiti: Rust stable, Node 20+, (macOS) Xcode CLT.

```bash
npm install
npm run tauri dev      # dev con HMR (finestra su Vite :1420, API su :6969)
npm run build          # build SPA (necessaria prima di cargo build/test)
cd src-tauri && cargo test              # unit + contract test
cd src-tauri && cargo test -- --ignored # contract test per-OS (kill, port scan, discovery)
npm run test:e2e       # Playwright smoke (builda la SPA e avvia il server fake)
npm run tauri build    # bundle release (.app/.dmg, .exe, .deb)
```

Config e log: `~/Library/Application Support/RickyDEVTool` (macOS) / `%APPDATA%\RickyDEVTool` (Windows).

## Architettura in breve

Tauri 2 come shell (finestra, tray, autostart); tutto il resto è un core Rust con axum: REST + WebSocket per topic, `PollerRegistry` che accende i collector solo quando qualche client è sottoscritto, adapter OS-specifici isolati in `src-tauri/src/adapters/`, task runner con stream dei log. La SPA React è embedded nel binario e servita anche alla LAN dietro pairing token.

## Sicurezza

- Localhost: accesso libero. LAN: cookie di pairing (QR dalle Impostazioni), bind su 0.0.0.0 solo se l'accesso LAN è attivo.
- Azioni di scrittura (kill, run, git, launch) solo da localhost, salvo toggle "Controllo remoto" (attivabile solo dal desktop).
- Processi di sistema mai killabili; processi critici (sshd, docker, …) richiedono conferma digitata. Eject/format dischi mai da remoto; disco di sistema escluso.

## CI / release

GitHub Actions ([.github/workflows/build.yml](.github/workflows/build.yml)): test matrix sui 3 OS a ogni push; su tag `v*` bundla gli installer e crea una release draft. Firma codice e auto-updater non ancora attivi (uso personale) — su macOS Gatekeeper e su Windows SmartScreen mostrano l'avviso "sviluppatore non verificato".
