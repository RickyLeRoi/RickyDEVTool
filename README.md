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
- **Docker**: lista container e immagini, start/stop/restart, log in streaming, immagini non usate con **prune**, e connessione a un **host Docker remoto** via `ssh://` o `tcp://` (vedi [Docker remoto](#docker-remoto)).
- **RickyAI** (beta, spenta di default): chatbot servito da [`of-free`](#rickyai-of-free) — avviato in automatico su questa macchina, oppure puntato a un servizio già in rete (es. Docker su un altro PC). Routing sui piani gratuiti degli LLM, quote residue per provider, scelta del modello (automatico / solo locale / uno preciso) e conversazioni multiple.
- **Task**: log persistente (ring buffer) di ogni task avviato dal tool, riapribile anche dopo la fine.
- **Utility**: avvii compositi, calcolatrice scientifica, color picker (eyedropper su Windows/WebView2, Colorimetro digitale su macOS), storico appunti (in memoria) con clipboard di rete.
- **Drop**: invio file/testo tra dispositivi in LAN (e tra host via discovery UDP), toast in ricezione.
- **About**: versione corrente con "verifica aggiornamenti", autore e contatti.
- **Barra laterale**: CPU/RAM in miniatura, avvii rapidi (se hai profili), shortcut Docker se attivo, e gli alert (CPU sostenuta, RAM alta, servizio down, task falliti).
- **Mobile**: stessa UI responsive da `http://<ip>:6969`, in sola lettura finché il "Controllo remoto" non viene attivato dal desktop. Deep-link `#/<sezione>` per gli shortcut sulla home.

## Sviluppo

Prerequisiti: Rust stable, Node 20+, (macOS) Xcode CLT.

```bash
npm install
npm run tauri:dev      # dev con HMR (finestra su Vite :1420, API su :6969) — sincronizza la versione
npm run build          # build SPA (necessaria prima di cargo build/test)
cd src-tauri && cargo test              # unit + contract test
cd src-tauri && cargo test -- --ignored # contract test per-OS (kill, port scan, discovery)
npm run test:e2e       # Playwright smoke (builda la SPA e avvia il server fake)
npm run tauri:build    # bundle release (.app/.dmg, .exe, .deb) — sincronizza la versione
```

Config e log: `~/Library/Application Support/RickyDEVTool` (macOS) / `%APPDATA%\RickyDEVTool` (Windows).

## Architettura in breve

Tauri 2 come shell (finestra, tray, autostart); tutto il resto è un core Rust con axum: REST + WebSocket per topic, `PollerRegistry` che accende i collector solo quando qualche client è sottoscritto, adapter OS-specifici isolati in `src-tauri/src/adapters/`, task runner con stream dei log. La SPA React è embedded nel binario e servita anche alla LAN dietro pairing token.

## Docker remoto

La sezione Docker può puntare a un daemon su un'altra macchina (es. una VM sul server di casa) invece che a quello locale. Nel campo **Host Docker** della sezione inserisci un endpoint con schema:

- `ssh://utente@host` — consigliato: passa dal tuo SSH, nessuna porta Docker esposta in rete;
- `tcp://ip:2375` — solo se il daemon espone l'API TCP (in chiaro: reti fidate).

La CLI `docker` resta locale (dev'essere installata su questo computer): cambia solo il daemon a cui si connette (`docker -H <host> …`). L'host si configura solo dal desktop; le azioni su container remoti seguono il "Controllo remoto" come le altre scritture.

### Prerequisiti SSH

`docker -H ssh://…` lancia `ssh` in modo **non interattivo**: serve accesso **a chiave, senza password**.

```bash
ssh-copy-id utente@host        # installa la tua chiave (chiede la password una volta)
ssh utente@host 'echo ok'      # deve stampare "ok" senza chiedere nulla
```

Se hai **ricreato la VM**, la sua host key cambia e SSH blocca la connessione (`Host key verification failed`): rimuovi la vecchia e riaccetta la nuova.

```bash
ssh-keygen -R host
ssh utente@host                # accetta il nuovo fingerprint (yes)
```

Per ricreazioni frequenti, in `~/.ssh/config`:

```
Host host
  User utente
  StrictHostKeyChecking accept-new
```

Se la sezione mostra un **errore** invece dei container, è lo stderr reale di `docker -H …`: lì trovi il motivo (risoluzione nome, chiave, daemon spento).

## RickyAI (of-free)

La sezione **RickyAI** è una chat che parla con [`of-free`](https://github.com/RickyLeRoi/onfeather-free) (OnFeather Free), il router che aggrega i piani gratuiti di Groq, Google AI Studio, Mistral, OpenRouter, GitHub Models e Ollama dietro un endpoint OpenAI-compatibile.

La funzione è in **beta e nasce spenta**: finché non la si attiva da *Impostazioni → RickyAI*, il tool non avvia nessun processo e **la sezione non compare nemmeno** nella barra laterale (né nella command palette). Spegnendola torna a sparire, senza riavviare niente.

Accesa, si sceglie **dove gira il motore**:

### Su questo computer

**A ogni accensione il tool fa partire `of-free serve` da solo**, in ascolto su `127.0.0.1:4141` (fallback sulle porte successive se occupata). Se un `of-free serve` è già in ascolto — perché lo hai lanciato tu da terminale — quello viene **adottato** invece di avviarne un secondo: due router sullo stesso ledger SQLite si pesterebbero i piedi sul conteggio delle quote. Un'istanza adottata non viene mai spenta dal tool.

Il binario si risolve nel PATH. Se manca, la pagina lo dice e spiega come installarlo, e il supervisore riprova da solo quando compare (nessun riavvio del tool).

```bash
git clone https://github.com/RickyLeRoi/onfeather-free && cd onfeather-free && pip install -e .
```

**Chiavi dei provider**: si incollano nelle Impostazioni, una per provider, e da lì in poi non sono più rileggibili — la UI mostra solo *impostata* e un campo mascherato. Vengono passate a `of-free` nel suo **environment**: niente file di chiavi da gestire, e niente chiavi sulla riga di comando (`ps` le mostrerebbe a tutti). Sono facoltative: senza nessuna, of-free ripiega sui modelli locali via Ollama. Chi le tiene già in `~/.onfeather/.env` non deve toccare niente — of-free continua a leggerle da lì, e quelle delle Impostazioni hanno la precedenza.

> Le chiavi finiscono in `config.json` in chiaro, come il token di pairing. Il file viene scritto `0600` (leggibile solo dal tuo utente).

### Servizio in rete

Un endpoint **OpenAI-compatibile** che gira altrove. Si indica l'indirizzo (`192.168.1.50:4141`; schema e porta di default sottintesi, `/v1` finale tollerato, path conservato per chi sta dietro un reverse proxy) più — se il servizio la richiede — una **chiave API**, mandata come `Authorization: Bearer`. Il tool non avvia né spegne niente: verifica che dall'altra parte risponda qualcosa di sensato, tiene d'occhio che continui a farlo, e ci inoltra le chat.

Funziona con tre famiglie di destinatari, provate una per una:

| Destinatario | Indirizzo | Chiave | Cosa cambia |
|---|---|---|---|
| `of-free` (anche in Docker) | `192.168.1.50:4141` | no | tutto come in locale: routing fra provider, quote, `auto`/`private` |
| Ollama, LM Studio, vLLM… | `192.168.1.50:11434` | no | si sceglie il modello dalla lista del servizio |
| OpenRouter e API hosted | `https://openrouter.ai/api/v1` | sì | idem, e la chiave viaggia solo verso quel servizio |

Quando dall'altra parte **non** c'è of-free il tool lo dice e si comporta di conseguenza: niente pannello quote (`/v1/status` è suo e altrove è un 404), e niente `auto`/`private` nel selettore — sono modelli virtuali del router, che su Ollama sarebbero nomi inesistenti. La liveness usa `/v1/models`, che è standard: `/health` esiste solo in of-free e su Ollama risponde 404, cioè "caduto" per un servizio che sta benissimo.

Un servizio in LAN deve essere in ascolto su `0.0.0.0` e non solo su `127.0.0.1`, altrimenti da fuori non risponde: è il motivo per cui un container "non si vede", ed è scritto nel messaggio d'errore.

> L'**adozione automatica** in locale resta invece stretta: lì nessuno ha indicato niente, e adottare per sbaglio un servizio qualsiasi in ascolto sulla 4141 significherebbe mandargli le conversazioni. Solo un endpoint che espone il modello `auto` viene adottato.

### In comune

La SPA **non parla mai con of-free**: passa da `/api/ai/chat`, che fa da proxy. Così la chat funziona anche dal telefono e l'endpoint locale — che non ha autenticazione, `api_key: unused` — non ha bisogno di uscire da localhost. Per lo stesso motivo l'of-free avviato dal tool usa sempre `--host 127.0.0.1`, mai `0.0.0.0`.

Le conversazioni **restano nel browser** che le ha scritte (localStorage): il backend non ne conserva nessuna. Desktop e telefono hanno quindi chat separate.

Configurazione dalle **Impostazioni** (`POST /api/ai/config`, solo dal desktop — decide quale binario il tool avvia da solo): abilitazione, modalità, indirizzo del servizio remoto, porta, percorso del binario, chiavi, strategia (`balanced` | `fast` | `local`) e prompt di sistema. `GET /api/ai/status` è in lettura e riporta **quali** chiavi sono impostate, mai il loro valore.

> Streaming non ancora supportato da `of-free`: la risposta arriva intera e la UI mostra l'attesa. Quando lo aggiungerà, il proxy andrà esteso di conseguenza.

## Sicurezza

- Localhost: accesso libero. LAN: cookie di pairing (QR dalle Impostazioni), bind su 0.0.0.0 solo se l'accesso LAN è attivo.
- Azioni di scrittura (kill, run, git, launch) solo da localhost, salvo toggle "Controllo remoto" (attivabile solo dal desktop). Anche la chat di RickyAI è una scrittura: il testo esce dalla macchina e consuma quote condivise.
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

### Versione (single source)

La versione vive in **un solo file**, `VERSION` nella root. Uno script la propaga a `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` e alla voce del crate in `Cargo.lock`:

```bash
node scripts/set-version.mjs 1.0.0   # imposta VERSION e sincronizza tutti i file
npm run version:sync                 # (senza argomento) risincronizza dal file VERSION
npm run version:check                # verifica l'allineamento (usato anche in CI)
```

I wrapper `npm run tauri:dev` / `npm run tauri:build` eseguono `version:sync` **prima** del CLI Tauri. La CI non scrive versioni: fa solo `version:check` (job `test`) e controlla che il tag combaci con `VERSION` (job `bundle`) — quindi committa sempre i file sincronizzati.

### Pubblicare una release

1. Bump: `node scripts/set-version.mjs 0.2.0`, poi commit di `VERSION` + i file sincronizzati;
2. tag e push: `git tag v0.2.0 && git push origin v0.2.0` (il tag **deve** essere `v` + il contenuto di `VERSION`);
3. la CI builda, firma e carica gli installer + `latest.json` su una release **draft**;
4. su GitHub premi **Publish** lasciandola come release normale — **non** spuntare "Set as a pre-release": l'endpoint `releases/latest/download/latest.json` risolve solo alla più recente release *non* prerelease, quindi una prerelease darebbe 404 e l'updater non scatterebbe.

### Ri-eseguire i job falliti

I secret vengono letti a runtime, quindi dopo aver corretto un secret **non serve ri-taggare**: basta ri-eseguire i job falliti della stessa run.

```bash
gh run list --repo <owner>/<repo> --limit 5          # trova il RUN_ID
gh run rerun --failed --repo <owner>/<repo> <RUN_ID>  # rilancia solo i job falliti
```

In alternativa, dalla pagina della run su GitHub: **Re-run failed jobs**.
