import { test, expect, type Page } from "@playwright/test";

// La pagina RickyAI contro il backend fake: lo stato "pronto" e la chat a eco
// arrivano da e2e/mock-server.mjs. Le varianti (quota esaurita, of-free non
// installato) si ottengono sovrascrivendo la singola rotta con page.route, così
// il mock resta uno solo e sempre nello stato buono.

const READY_STATUS = {
  state: "ready",
  port: 4141,
  baseUrl: "http://127.0.0.1:4141",
  managed: true,
  command: "/usr/local/bin/of-free",
  message: null,
  startedAt: Date.now(),
  restarts: 0,
  log: [],
  enabled: true,
  configuredPort: 4141,
  strategy: "balanced",
  envFile: null,
  systemPrompt: "",
  providers: [
    { name: "groq", label: "Groq", available: true, headroom: 0.82, local: false, limits: [] },
  ],
  next: { provider: "groq", model: "llama-3.3-70b-versatile" },
  models: ["auto", "groq/llama-3.3-70b-versatile"],
};

async function mockWs(page: Page, onSubscribe?: (topic: string, send: (e: unknown) => void) => void) {
  await page.routeWebSocket(/\/ws$/, (ws) => {
    ws.onMessage((raw) => {
      let msg: { type?: string; topic?: string };
      try {
        msg = JSON.parse(typeof raw === "string" ? raw : raw.toString());
      } catch {
        return;
      }
      if (msg.type === "subscribe" && msg.topic) {
        onSubscribe?.(msg.topic, (event) => ws.send(JSON.stringify(event)));
      }
    });
  });
}

/** Sostituisce /api/ai/status con lo stato passato. */
async function withStatus(page: Page, status: Record<string, unknown>) {
  await page.route("**/api/ai/status", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, data: { ...READY_STATUS, ...status } }),
    }),
  );
}

const composer = (page: Page) => page.locator(".ai-composer textarea");

async function open(page: Page) {
  await page.goto("/#/rickyai");
  await expect(page.getByRole("heading", { name: "RickyAI", exact: true })).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await mockWs(page);
});

test("con of-free spento la sezione non esiste", async ({ page }) => {
  await withStatus(page, {
    enabled: false,
    state: "disabled",
    message: "avvio automatico disattivato",
    providers: null,
    models: [],
    next: null,
  });

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
  // Niente voce nella rail: una sezione che non può funzionare non si mostra.
  await expect(page.locator(".rail-btn", { hasText: "RickyAI" })).toHaveCount(0);

  // E nemmeno nella command palette, che è l'altra strada per arrivarci.
  await page.keyboard.press("Control+k");
  await page.locator(".cmdk-input").fill("ricky");
  await expect(page.locator(".cmdk-item")).toHaveCount(0);
  await page.keyboard.press("Escape");

  // Il deep-link diretto non deve lasciare una pagina vuota: si torna indietro.
  await page.goto("/#/rickyai");
  await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
  await expect(page.locator(".rickyai")).toHaveCount(0);
});

test("accendendo of-free la sezione compare senza ricaricare", async ({ page }) => {
  let acceso = false;
  await page.route("**/api/ai/status", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: acceso
          ? READY_STATUS
          : { ...READY_STATUS, enabled: false, state: "disabled", providers: null, models: [] },
      }),
    }),
  );
  let pushAi: ((event: unknown) => void) | null = null;
  await mockWs(page, (topic, send) => {
    if (topic === "ai") pushAi = send;
  });

  await page.goto("/");
  await expect(page.locator(".rail-btn", { hasText: "RickyAI" })).toHaveCount(0);

  // L'interruttore nelle impostazioni fa ripartire il supervisore, che
  // pubblica il nuovo stato: la rail si aggiorna da sola.
  acceso = true;
  await expect.poll(() => pushAi != null).toBe(true);
  pushAi!({ topic: "ai", ts: Date.now(), payload: { state: "starting" } });

  await expect(page.locator(".rail-btn", { hasText: "RickyAI" })).toBeVisible();
  await page.locator(".rail-btn", { hasText: "RickyAI" }).click();
  await expect(page.getByRole("heading", { name: "RickyAI", exact: true })).toBeVisible();
});

test("mostra lo stato del motore e le quote dei provider", async ({ page }) => {
  await open(page);
  await expect(page.locator(".ai-status .badge")).toHaveText("pronto");
  // La provenienza della prossima richiesta è l'informazione che rende
  // comprensibile un router di quote: senza, l'utente non sa chi risponderà.
  await expect(page.locator(".ai-status")).toContainText("Groq");
  await expect(page.locator(".ai-status")).toContainText("llama-3.3-70b-versatile");
  await expect(composer(page)).toBeEnabled();
});

test("invia un messaggio e mostra la risposta con la provenienza", async ({ page }) => {
  await open(page);
  await composer(page).fill("ciao RickyAI");
  await page.getByRole("button", { name: "Invia" }).click();

  await expect(page.locator(".ai-msg-user")).toHaveText("ciao RickyAI");
  // L'eco prova che il messaggio è arrivato davvero al backend.
  await expect(page.locator(".ai-msg-bot")).toContainText("RickyAI dice: ciao RickyAI");
  await expect(page.locator(".ai-msg-meta")).toContainText("groq");
  // Il titolo del thread viene dal primo messaggio.
  await expect(page.locator(".ai-thread.active")).toContainText("ciao RickyAI");
  await expect(composer(page)).toHaveValue("");
});

test("Invio manda, Shift+Invio va a capo", async ({ page }) => {
  await open(page);
  await composer(page).fill("prima riga");
  await composer(page).press("Shift+Enter");
  await composer(page).pressSequentially("seconda riga");
  // Shift+Invio non deve inviare: nessuna bolla, e il testo è ancora nel campo.
  await expect(page.locator(".ai-msg-user")).toHaveCount(0);
  await expect(composer(page)).toHaveValue("prima riga\nseconda riga");

  await composer(page).press("Enter");
  await expect(page.locator(".ai-msg-user")).toHaveText("prima riga\nseconda riga");
});

test("il turno successivo rispedisce la conversazione come contesto", async ({ page }) => {
  const bodies: { role: string; content: string }[][] = [];
  await page.route("**/api/ai/chat", async (route) => {
    const body = route.request().postDataJSON() as {
      messages: { role: string; content: string }[];
      model: string;
    };
    bodies.push(body.messages);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: {
          content: `risposta ${bodies.length}`,
          provider: "groq",
          model: "llama",
          failovers: 0,
          repinned: null,
          finishReason: "stop",
          usage: null,
          elapsedMs: 10,
        },
      }),
    });
  });

  await open(page);
  await composer(page).fill("primo");
  await composer(page).press("Enter");
  await expect(page.locator(".ai-msg-bot")).toContainText("risposta 1");

  await composer(page).fill("secondo");
  await composer(page).press("Enter");
  await expect(page.locator(".ai-msg-bot").nth(1)).toContainText("risposta 2");

  // Senza lo storico il modello risponderebbe al secondo turno come se fosse
  // il primo: è la differenza fra una chat e una serie di domande scollegate.
  expect(bodies[0]).toEqual([{ role: "user", content: "primo" }]);
  expect(bodies[1]).toEqual([
    { role: "user", content: "primo" },
    { role: "assistant", content: "risposta 1" },
    { role: "user", content: "secondo" },
  ]);
});

test("più chat: nuova, cambio, eliminazione", async ({ page }) => {
  await open(page);
  await composer(page).fill("chat uno");
  await composer(page).press("Enter");
  await expect(page.locator(".ai-msg-bot")).toBeVisible();

  await page.getByRole("button", { name: "+ Nuova chat" }).click();
  await expect(page.locator(".ai-msg-user")).toHaveCount(0);
  await expect(page.locator(".ai-thread")).toHaveCount(2);

  await composer(page).fill("chat due");
  await composer(page).press("Enter");
  await expect(page.locator(".ai-msg-user")).toHaveText("chat due");

  // Tornando sulla prima si rivedono i suoi messaggi, non quelli della seconda.
  await page.locator(".ai-thread", { hasText: "chat uno" }).click();
  await expect(page.locator(".ai-msg-user")).toHaveText("chat uno");

  await page.locator(".ai-thread", { hasText: "chat due" }).getByLabel("Elimina chat").click();
  await expect(page.locator(".ai-thread")).toHaveCount(1);
});

test("le conversazioni sopravvivono al reload", async ({ page }) => {
  await open(page);
  await composer(page).fill("ricordati di me");
  await composer(page).press("Enter");
  await expect(page.locator(".ai-msg-bot")).toBeVisible();

  await page.reload();
  await expect(page.getByRole("heading", { name: "RickyAI", exact: true })).toBeVisible();
  await expect(page.locator(".ai-msg-user")).toHaveText("ricordati di me");
  await expect(page.locator(".ai-msg-bot")).toContainText("RickyAI dice: ricordati di me");
});

test("quota esaurita: messaggio, attesa e ritenta", async ({ page }) => {
  let esaurita = true;
  await page.route("**/api/ai/chat", async (route) => {
    if (esaurita) {
      esaurita = false;
      await route.fulfill({
        status: 429,
        contentType: "application/json",
        body: JSON.stringify({
          ok: false,
          error: {
            code: "AI_QUOTA",
            message: "Quota esaurita: tutti i provider hanno finito",
            retryAfter: 37,
            retryable: true,
          },
        }),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: {
          content: "eccomi",
          provider: "ollama",
          model: "qwen2.5",
          failovers: 1,
          repinned: null,
          finishReason: "stop",
          usage: null,
          elapsedMs: 900,
        },
      }),
    });
  });

  await open(page);
  await composer(page).fill("domanda cara");
  await composer(page).press("Enter");

  await expect(page.locator(".ai-error")).toContainText("Quota esaurita");
  // Il tempo d'attesa arriva dall'header Retry-After di of-free: è la
  // differenza fra "riprova fra poco" e "riprova a caso".
  await expect(page.locator(".ai-error")).toContainText("~37s");
  // Il messaggio dell'utente resta a schermo: si ritenta quello, non si
  // costringe a riscriverlo.
  await expect(page.locator(".ai-msg-user")).toHaveText("domanda cara");

  await page.getByRole("button", { name: "Riprova" }).click();
  await expect(page.locator(".ai-msg-bot")).toContainText("eccomi");
  await expect(page.locator(".ai-error")).toHaveCount(0);
});

test("of-free non installato: composer bloccato e istruzioni", async ({ page }) => {
  await withStatus(page, {
    state: "notInstalled",
    message: "`of-free` non trovato: installalo o indica il percorso",
    providers: null,
    models: [],
    next: null,
  });
  await open(page);

  await expect(page.locator(".ai-status .badge")).toHaveText("of-free non installato");
  await expect(page.locator(".ai-status")).toContainText("pip install -e .");
  // Senza motore il campo è disabilitato: meglio di un invio che fallisce.
  await expect(composer(page)).toBeDisabled();
  await expect(composer(page)).toHaveAttribute("placeholder", "RickyAI non è disponibile");
});

test("avvio fallito: mostra il perché e le ultime righe di of-free", async ({ page }) => {
  await withStatus(page, {
    state: "failed",
    message: "of-free è uscito (codice 1)",
    log: ["error: cannot bind 127.0.0.1:4141 — Address already in use"],
    providers: null,
    models: [],
    next: null,
  });
  await open(page);

  await expect(page.locator(".ai-status .badge")).toHaveText("non disponibile");
  await expect(page.locator(".ai-status")).toContainText("of-free è uscito (codice 1)");
  // Le righe del processo sono l'unico posto in cui il motivo vero è scritto.
  await expect(page.locator(".ai-log")).toContainText("Address already in use");
});

test("un evento WS del supervisore aggiorna lo stato senza ricaricare", async ({ page }) => {
  let caduto = false;
  await page.route("**/api/ai/status", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: caduto
          ? { ...READY_STATUS, state: "failed", message: "of-free è uscito (codice 1)" }
          : READY_STATUS,
      }),
    }),
  );
  let pushAi: ((event: unknown) => void) | null = null;
  await mockWs(page, (topic, send) => {
    if (topic === "ai") pushAi = send;
  });

  await open(page);
  await expect(page.locator(".ai-status .badge")).toHaveText("pronto");

  caduto = true;
  await expect.poll(() => pushAi != null).toBe(true);
  pushAi!({ topic: "ai", ts: Date.now(), payload: { state: "failed" } });

  await expect(page.locator(".ai-status .badge")).toHaveText("non disponibile");
  await expect(page.locator(".ai-status")).toContainText("of-free è uscito");
});

test("impostazioni: la strategia si salva con un click", async ({ page }) => {
  const salvati: Record<string, unknown>[] = [];
  await page.route("**/api/ai/config", async (route) => {
    const body = route.request().postDataJSON() as Record<string, unknown>;
    salvati.push(body);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, data: { ...READY_STATUS, strategy: "local" } }),
    });
  });

  await page.goto("/#/settings");
  const panel = page.locator(".ai-settings");
  // La funzione è dichiarata beta accanto al suo interruttore.
  await expect(panel.locator(".badge-beta")).toHaveText("beta");
  await expect(panel.locator(".segmented button.active")).toHaveText("Bilanciata");

  await panel.locator(".segmented button", { hasText: "Locale" }).click();
  expect(salvati).toEqual([{ strategy: "local" }]);
  // Lo stato mostrato viene dalla risposta del server, non dal click: se il
  // salvataggio fallisse, il pulsante non resterebbe acceso a mentire.
  await expect(panel.locator(".segmented button.active")).toHaveText("Locale");
});

test("impostazioni: binario, chiavi e prompt si salvano insieme", async ({ page }) => {
  const salvati: Record<string, unknown>[] = [];
  await page.route("**/api/ai/config", async (route) => {
    salvati.push(route.request().postDataJSON() as Record<string, unknown>);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, data: READY_STATUS }),
    });
  });

  await page.goto("/#/settings");
  const panel = page.locator(".ai-settings");
  const salva = panel.getByRole("button", { name: "Salva e riavvia" });
  // Niente da salvare finché non si tocca niente.
  await expect(salva).toBeDisabled();

  await panel.locator("input[placeholder='vuoto = ~/.onfeather/.env']").fill("/home/ricky/keys.env");
  await panel.locator("textarea").fill("Sei RickyAI, rispondi in italiano.");
  await expect(salva).toBeEnabled();
  await salva.click();

  expect(salvati).toHaveLength(1);
  expect(salvati[0]).toMatchObject({
    envFile: "/home/ricky/keys.env",
    systemPrompt: "Sei RickyAI, rispondi in italiano.",
    port: 4141,
  });
});

test("impostazioni: un percorso sbagliato viene rifiutato e detto", async ({ page }) => {
  await page.route("**/api/ai/config", (route) =>
    route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({
        ok: false,
        error: {
          code: "INTERNAL",
          message: "binario of-free non trovato: /bin/inesistente",
          retryable: true,
        },
      }),
    }),
  );

  await page.goto("/#/settings");
  const panel = page.locator(".ai-settings");
  await panel.locator("input[placeholder='vuoto = cercato nel PATH']").fill("/bin/inesistente");
  await panel.getByRole("button", { name: "Salva e riavvia" }).click();

  // Il backend verifica che il file esista davvero: senza, il supervisore
  // riproverebbe a vuoto e la pagina direbbe solo "non installato".
  await expect(panel.locator(".banner-error")).toContainText("non trovato");
});

test("il selettore modello propone automatico, privato e i modelli disponibili", async ({
  page,
}) => {
  await open(page);
  const select = page.locator(".ai-model");
  await expect(select).toHaveValue("auto");
  await expect(select.locator("option")).toHaveText([
    "Automatico (miglior quota)",
    "Solo locale (privato)",
    "groq/llama-3.3-70b-versatile",
    "ollama/qwen2.5:7b",
  ]);

  // La scelta è per dispositivo e sopravvive al reload: chi vuole restare in
  // locale non deve riselezionarlo ogni volta.
  await select.selectOption("private");
  await page.reload();
  await expect(page.locator(".ai-model")).toHaveValue("private");
});
