import { test, expect, type Page } from "@playwright/test";

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
  ofFree: true,
  mode: "local",
  remoteUrl: null,
  remoteKeySet: false,
  configuredPort: 4141,
  strategy: "balanced",
  systemPrompt: "",
  keysSet: ["GROQ_API_KEY"],
  providerKeys: [
    { id: "groq", label: "Groq", env: "GROQ_API_KEY" },
    { id: "google", label: "Google AI Studio", env: "GEMINI_API_KEY" },
    { id: "mistral", label: "Mistral La Plateforme", env: "MISTRAL_API_KEY" },
  ],
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
  await expect(page.locator(".rail-btn", { hasText: "RickyAI" })).toHaveCount(0);

  await page.keyboard.press("Control+k");
  await page.locator(".cmdk-input").fill("ricky");
  await expect(page.locator(".cmdk-item")).toHaveCount(0);
  await page.keyboard.press("Escape");

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
  await expect(page.locator(".ai-status")).toContainText("Groq");
  await expect(page.locator(".ai-status")).toContainText("llama-3.3-70b-versatile");
  await expect(composer(page)).toBeEnabled();
});

test("invia un messaggio e mostra la risposta con la provenienza", async ({ page }) => {
  await open(page);
  await composer(page).fill("ciao RickyAI");
  await page.getByRole("button", { name: "Invia" }).click();

  await expect(page.locator(".ai-msg-user")).toHaveText("ciao RickyAI");
  await expect(page.locator(".ai-msg-bot")).toContainText("RickyAI dice: ciao RickyAI");
  await expect(page.locator(".ai-msg-meta")).toContainText("groq");
  await expect(page.locator(".ai-thread.active")).toContainText("ciao RickyAI");
  await expect(composer(page)).toHaveValue("");
});

test("Invio manda, Shift+Invio va a capo", async ({ page }) => {
  await open(page);
  await composer(page).fill("prima riga");
  await composer(page).press("Shift+Enter");
  await composer(page).pressSequentially("seconda riga");
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
  await expect(page.locator(".ai-error")).toContainText("~37s");
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
  await expect(panel.locator(".badge-beta")).toHaveText("beta");
  const strategia = panel.locator(".form-row", { hasText: "Strategia di routing" });
  await expect(strategia.locator("button.active")).toHaveText("Bilanciata");

  await strategia.locator("button", { hasText: "Locale" }).click();
  expect(salvati).toEqual([{ strategy: "local" }]);
  await expect(strategia.locator("button.active")).toHaveText("Locale");
});

test("impostazioni: binario e prompt si salvano insieme", async ({ page }) => {
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
  await expect(salva).toBeDisabled();

  await panel.locator("input[placeholder='vuoto = cercato nel PATH']").fill("/opt/of-free");
  await panel.locator("textarea").fill("Sei RickyAI, rispondi in italiano.");
  await expect(salva).toBeEnabled();
  await salva.click();

  expect(salvati).toHaveLength(1);
  expect(salvati[0]).toMatchObject({
    command: "/opt/of-free",
    systemPrompt: "Sei RickyAI, rispondi in italiano.",
    port: 4141,
  });
});

test("impostazioni: le chiavi si incollano una alla volta e non tornano più indietro", async ({
  page,
}) => {
  const salvati: Record<string, unknown>[] = [];
  await page.route("**/api/ai/config", async (route) => {
    const body = route.request().postDataJSON() as { keys?: Record<string, string> };
    salvati.push(body);
    const keys = body.keys ?? {};
    const set = new Set(READY_STATUS.keysSet);
    for (const [name, value] of Object.entries(keys)) {
      if (value) set.add(name);
      else set.delete(name);
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, data: { ...READY_STATUS, keysSet: [...set] } }),
    });
  });

  await page.goto("/#/settings");
  const groq = page.locator(".ai-key-row", { hasText: "Groq" });
  const gemini = page.locator(".ai-key-row", { hasText: "Google AI Studio" });

  await expect(groq.locator(".badge-ok")).toHaveText("impostata");
  await expect(groq.locator("input")).toHaveValue("");
  await expect(groq.locator("input")).toHaveAttribute("type", "password");
  await expect(gemini.locator(".badge-ok")).toHaveCount(0);

  await gemini.locator("input").fill("AIzaSyTOPSECRET");
  await gemini.getByRole("button", { name: "Salva" }).click();

  expect(salvati).toEqual([{ keys: { GEMINI_API_KEY: "AIzaSyTOPSECRET" } }]);
  await expect(gemini.locator(".badge-ok")).toHaveText("impostata");
  await expect(gemini.locator("input")).toHaveValue("");

  await groq.getByRole("button", { name: "Rimuovi" }).click();
  expect(salvati[1]).toEqual({ keys: { GROQ_API_KEY: "" } });
  await expect(groq.locator(".badge-ok")).toHaveCount(0);
});

test("impostazioni: modalità servizio in rete", async ({ page }) => {
  const salvati: Record<string, unknown>[] = [];
  await page.route("**/api/ai/config", async (route) => {
    const body = route.request().postDataJSON() as Record<string, unknown>;
    salvati.push(body);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: {
          ...READY_STATUS,
          mode: "remote",
          remoteUrl: "http://192.168.1.50:4141",
          baseUrl: "http://192.168.1.50:4141",
          managed: false,
        },
      }),
    });
  });

  await page.goto("/#/settings");
  const panel = page.locator(".ai-settings");
  await panel.locator(".segmented button", { hasText: "Servizio in rete" }).click();

  const indirizzo = panel.locator("input[placeholder='es. 192.168.1.50:4141']");
  await expect(indirizzo).toBeVisible();
  await expect(panel.locator(".ai-key-row", { hasText: "Groq" })).toHaveCount(0);
  await expect(panel.locator(".ai-key-row")).toHaveCount(1);
  await expect(panel.locator(".ai-key-row", { hasText: "Chiave API" })).toBeVisible();
  await expect(panel.locator("input[placeholder='vuoto = cercato nel PATH']")).toHaveCount(0);

  await indirizzo.fill("192.168.1.50:4141");
  await panel.getByRole("button", { name: "Salva e riavvia" }).click();
  expect(salvati.at(-1)).toMatchObject({ remoteUrl: "192.168.1.50:4141" });
});

test("endpoint OpenAI generico: niente auto/private, niente quote", async ({ page }) => {
  await withStatus(page, {
    mode: "remote",
    remoteUrl: "http://192.168.1.50:11434",
    baseUrl: "http://192.168.1.50:11434",
    managed: false,
    ofFree: false,
    models: ["qwen2.5:7b", "llama3.2:3b"],
    providers: null,
    next: null,
    message: "endpoint OpenAI-compatibile (non of-free): niente routing fra provider né quote",
  });
  await open(page);

  const select = page.locator(".ai-model");
  await expect(select.locator("option")).toHaveText(["qwen2.5:7b", "llama3.2:3b"]);
  await expect(select).toHaveValue("qwen2.5:7b");
  await expect(page.locator(".ai-status")).toContainText("endpoint OpenAI");
  await expect(page.locator(".ai-status")).toContainText("nessun routing fra provider");
  await expect(page.locator(".ai-provider")).toHaveCount(0);
});

test("endpoint generico: il modello inviato è uno di quelli che esistono", async ({ page }) => {
  await withStatus(page, {
    ofFree: false,
    mode: "remote",
    baseUrl: "http://192.168.1.50:11434",
    models: ["qwen2.5:7b"],
    providers: null,
    next: null,
  });
  let inviato: { model?: string } = {};
  await page.route("**/api/ai/chat", async (route) => {
    inviato = route.request().postDataJSON() as { model?: string };
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: {
          content: "ok",
          provider: null,
          model: "qwen2.5:7b",
          failovers: null,
          repinned: null,
          finishReason: "stop",
          usage: null,
          elapsedMs: 5,
        },
      }),
    });
  });

  await open(page);
  await composer(page).fill("ciao");
  await composer(page).press("Enter");
  await expect(page.locator(".ai-msg-bot")).toContainText("ok");
  expect(inviato.model).toBe("qwen2.5:7b");
});

test("impostazioni: la chiave del servizio remoto si imposta e si toglie", async ({ page }) => {
  const salvati: Record<string, unknown>[] = [];
  let conChiave = false;
  await page.route("**/api/ai/status", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: {
          ...READY_STATUS,
          mode: "remote",
          remoteUrl: "https://openrouter.ai/api",
          remoteKeySet: conChiave,
        },
      }),
    }),
  );
  await page.route("**/api/ai/config", async (route) => {
    const body = route.request().postDataJSON() as { remoteKey?: string };
    salvati.push(body);
    conChiave = !!body.remoteKey;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        data: {
          ...READY_STATUS,
          mode: "remote",
          remoteUrl: "https://openrouter.ai/api",
          remoteKeySet: conChiave,
        },
      }),
    });
  });

  await page.goto("/#/settings");
  const chiave = page.locator(".ai-key-row", { hasText: "Chiave API" });
  await expect(chiave.locator("input")).toHaveAttribute("type", "password");
  await expect(chiave.locator(".badge-ok")).toHaveCount(0);

  await chiave.locator("input").fill("sk-or-v1-segretissima");
  await chiave.getByRole("button", { name: "Salva" }).click();
  expect(salvati).toEqual([{ remoteKey: "sk-or-v1-segretissima" }]);
  await expect(chiave.locator(".badge-ok")).toHaveText("impostata");
  await expect(chiave.locator("input")).toHaveValue("");

  await chiave.getByRole("button", { name: "Rimuovi" }).click();
  expect(salvati[1]).toEqual({ remoteKey: "" });
});

test("modalità remota: la chat dice che il motore è in rete", async ({ page }) => {
  await withStatus(page, {
    mode: "remote",
    remoteUrl: "http://192.168.1.50:4141",
    baseUrl: "http://192.168.1.50:4141",
    managed: false,
  });
  await open(page);

  await expect(page.locator(".ai-status .badge")).toHaveText("pronto");
  await expect(page.locator(".ai-status")).toContainText("of-free in rete");
  await expect(page.locator(".ai-status")).toContainText("192.168.1.50:4141");
  await expect(composer(page)).toBeEnabled();
});

test("modalità remota: servizio irraggiungibile, con il motivo", async ({ page }) => {
  await withStatus(page, {
    mode: "remote",
    remoteUrl: "http://192.168.1.50:4141",
    state: "failed",
    message:
      "nessun of-free raggiungibile su http://192.168.1.50:4141: controlla che il servizio sia acceso e che sia in ascolto su tutte le interfacce, non solo su 127.0.0.1",
    providers: null,
    models: [],
    next: null,
  });
  await open(page);

  await expect(page.locator(".ai-status")).toContainText("192.168.1.50:4141");
  await expect(page.locator(".ai-status")).toContainText("tutte le interfacce");
  await expect(composer(page)).toBeDisabled();
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

  await select.selectOption("private");
  await page.reload();
  await expect(page.locator(".ai-model")).toHaveValue("private");
});
