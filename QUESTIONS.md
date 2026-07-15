# QUESTIONS — dubbi aperti

Legenda: 🔴 bloccante (serve risposta prima di procedere sulla parte interessata) · 🟡 non bloccante (assunta una default, cambiabile dopo).

## 🔴 Bloccanti — tutte risolte il 15/07/2026

### Q1 — Toolchain Rust mancante → RISOLTA
Deciso: installato rustup (Rust 1.97 stable, profilo minimal). Stack confermato: Tauri 2.

### Q2 — Nome dell'app → RISOLTA
Deciso: **RickyDEVTool**, identifier `com.riccardogiordano.rickydevtool`.

### Q3 — Lingua della UI → RISOLTA
Deciso: italiano, hardcoded (i18n eventualmente in v2).

## 🟡 Non bloccanti (default scelte, rivedibili)

### Q4 — Accesso LAN di default
Default scelta: **ON al primo avvio** (bind su 127.0.0.1); si attiva da Impostazioni e appare QR+IP. 

### Q5 — Porta 6969 occupata
Default scelta: se 6969 è occupata, il tool prova 6970-6979 e mostra la porta effettiva (tray + finestra). Alternativa: errore bloccante con messaggio. La porta resta configurabile.

### Q6 — Normalizzazione CPU per processo
Default scelta: CPU% sempre normalizzata sul totale core su entrambi gli OS (quindi la soglia ">20%" ha lo stesso significato ovunque; su un M-series a 10 core un processo single-thread al massimo mostra ~10%). Alternativa: stile "top" per-core. Da validare all'uso.

### Q7 — Lista processi protetti (typed-confirm)
Bozza: `sshd, dockerd, com.docker.backend, smbd, Plex Media Server, postgres, mysqld, redis-server`. Da confermare/estendere in base a cosa gira sul tuo server/macchine.

### Q8 — Preset servizi personali cloudflared
Servono gli hostname dei tuoi servizi dietro tunnel per i preset "personali" (con eventuale endpoint /healthz). Non bloccante: la sezione parte con i soli preset pubblici, i tuoi si aggiungono da Impostazioni.

### Q9 — Firma e distribuzione
- macOS: hai un account Apple Developer (99€/anno) per la notarizzazione? No, è per uso personale.
- Windows: certificato di code signing? Uno autosigned?.

### Q10 — Versioni minime OS
Default assunta: Windows 10 1809+ (WebView2), macOS 12+. Se hai macchine più vecchie da supportare, dillo prima di M8. No.

### Q11 — Visual Studio: quale versione usi
La detection via vswhere copre VS ≥2022.

### Q12 — Grafo commit (v2)
Confermata la versione semplificata: lista ultimi ~100 commit del branch selezionato, checkout in detached HEAD con warning. Il grafo disegnato con lane non si fa. Se per te il grafo visuale è irrinunciabile, va rivalutato il costo (~1-2 settimane in più).
