# RickyDEVTool

Developer operations console locale per macOS e Windows (Linux best-effort). Un solo binario: finestra desktop + web server su **:6969** che serve la stessa UI anche allo smartphone in LAN (pairing via QR).

Documentazione di progetto: [PROJECT.md](PROJECT.md) · piano e stato: [PLAN.md](PLAN.md) · decisioni aperte: [QUESTIONS.md](QUESTIONS.md).

## Funzionalità

- **Dashboard**: CPU totale + per core, RAM/swap, sparkline, intervallo 0.5–10s, processi pesanti on-demand con soglie.
- **Porte**: TCP in ascolto di processi non di sistema, raggruppate per porta, kill con conferma (typed-confirm per i processi protetti). Attiva solo a sezione aperta.
- **Progetti**: browser cartelle con pin, riconoscimento git/Node/.NET; git (stato, fetch, pull --ff-only, branch con checkout, stale evidenziati), Node (install/start/script con package manager rilevato), .NET (startup project, profili launchSettings, run/rebuild/clean), log dei task in streaming, apri in VS Code/terminale.
- **Servizi online**: health check HTTP/TCP dei preset (Google, Cloudflare, WhatsApp, …) e di servizi personali, solo a sezione aperta.
- **Alert**: CPU sostenuta, RAM alta, servizio down, task falliti — nel pannello laterale.
- **Mobile**: stessa UI responsive da `http://<ip>:6969`, in sola lettura finché il "Controllo remoto" non viene attivato dal desktop.

## Sviluppo

Prerequisiti: Rust stable, Node 20+, (macOS) Xcode CLT.

```bash
npm install
npm run tauri dev      # dev con HMR (finestra su Vite :1420, API su :6969)
npm run build          # build SPA (necessaria prima di cargo build/test)
cd src-tauri && cargo test   # unit + contract test
npm run tauri build    # bundle release (.app/.dmg, .exe, .deb)
```

Config e log: `~/Library/Application Support/RickyDEVTool` (macOS) / `%APPDATA%\RickyDEVTool` (Windows).

## Architettura in breve

Tauri 2 come shell (finestra, tray, autostart); tutto il resto è un core Rust con axum: REST + WebSocket per topic, `PollerRegistry` che accende i collector solo quando qualche client è sottoscritto, adapter OS-specifici isolati in `src-tauri/src/adapters/`, task runner con stream dei log. La SPA React è embedded nel binario e servita anche alla LAN dietro pairing token. Dettagli in [PROJECT.md](PROJECT.md) §3.

## Sicurezza

- Localhost: accesso libero. LAN: cookie di pairing (QR dalle Impostazioni), bind su 0.0.0.0 solo se l'accesso LAN è attivo.
- Azioni di scrittura (kill, run, git, launch) solo da localhost, salvo toggle "Controllo remoto" (attivabile solo dal desktop).
- Processi di sistema mai killabili; processi critici (sshd, docker, …) richiedono conferma digitata.
