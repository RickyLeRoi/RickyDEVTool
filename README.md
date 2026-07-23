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

GitHub Actions ([.github/workflows/build.yml](.github/workflows/build.yml)): a ogni push gira la test matrix sui 3 OS (`cargo test` + i contract test `--ignored`) e lo smoke test Playwright; su tag `v*` il job `bundle` compila gli installer, firma gli artefatti dell'updater e crea una **release draft**.

Firma codice OS (Apple/Authenticode) **non** attiva (uso personale) → su macOS Gatekeeper e su Windows SmartScreen mostrano "sviluppatore non verificato" (aggirabile: click destro > Apri).

### Auto-updater

L'app controlla all'avvio la release più recente su GitHub (endpoint in [tauri.conf.json](src-tauri/tauri.conf.json)) e, se ce n'è una nuova, mostra un banner per scaricarla e riavviare. La firma usa **minisign** (chiave dedicata, indipendente dalla notarizzazione Apple), quindi funziona anche senza account Apple Developer.

Perché la CI possa firmare gli artefatti servono due **secret** nel repo, corrispondenti alla `pubkey` in [tauri.conf.json](src-tauri/tauri.conf.json):

- `TAURI_SIGNING_PRIVATE_KEY` — la chiave privata (contenuto del file `.key`, o la sua stringa base64);
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — la password scelta al `tauri signer generate` (vuota se non impostata).

**Impostarli da CLI** — usa `printf %s` (NON `echo` né l'incolla nel prompt interattivo: aggiungono un `\n` finale che rompe la decodifica base64 → `Invalid padding` in fase di firma):

```bash
printf %s 'LA_TUA_CHIAVE_PRIVATA' | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo <owner>/<repo>
printf %s 'LA_TUA_PASSWORD'      | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo <owner>/<repo>
# senza password:
printf '' | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo <owner>/<repo>
```

**Oppure dalla UI web**: Settings → Secrets and variables → Actions → New repository secret. Incolla il valore **su una riga sola, senza premere Invio** (un newline finale dà lo stesso `Invalid padding`).

> La chiave privata deve stare **solo** nei secret, mai committata nel repo. Se rigeneri la coppia (`tauri signer generate`), aggiorna anche la `pubkey` nel config con `base64 -i chiave.key.pub`.
>
> ⚠️ La `pubkey` nel config deve essere base64 **completa** (padding `=` incluso): una stringa troncata dà `failed to decode pubkey: Invalid padding` in fase di build. Verifica con `python3 -c "import base64,json;base64.b64decode(json.load(open('src-tauri/tauri.conf.json'))['plugins']['updater']['pubkey'],validate=True)"` (nessun errore = ok).

### Pubblicare una release

1. Bump di `version` in [tauri.conf.json](src-tauri/tauri.conf.json) e [Cargo.toml](src-tauri/Cargo.toml), commit;
2. tag e push: `git tag v0.2.0 && git push origin v0.2.0`;
3. la CI builda, firma e carica gli installer + `latest.json` su una release **draft**;
4. su GitHub premi **Publish**: solo così l'endpoint `releases/latest/download/latest.json` diventa raggiungibile e i client vedono l'aggiornamento.

### Ri-eseguire i job falliti

I secret vengono letti a runtime, quindi dopo aver corretto un secret **non serve ri-taggare**: basta ri-eseguire i job falliti della stessa run.

```bash
gh run list --repo <owner>/<repo> --limit 5          # trova il RUN_ID
gh run rerun --failed --repo <owner>/<repo> <RUN_ID>  # rilancia solo i job falliti
```

In alternativa, dalla pagina della run su GitHub: **Re-run failed jobs**.
