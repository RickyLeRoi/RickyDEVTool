import { test, expect, type Page } from "@playwright/test";

// Stessa tecnica dello smoke: WS mockato in-browser. Serve perché la SPA
// sottoscrive "stats" all'avvio; gli altri topic senza dati canned non spingono
// nulla (sensors/docker:stats restano vuoti, ok per il test di layout).
const stats = {
  ts: Date.now(),
  cpuTotalPct: 42,
  cores: [{ core: 0, pct: 40 }, { core: 1, pct: 44 }],
  mem: { totalBytes: 16 * 1024 ** 3, usedBytes: 8 * 1024 ** 3, usedPct: 50 },
  swap: null,
  intervalMs: 10000,
};

async function mockWs(page: Page) {
  await page.routeWebSocket(/\/ws$/, (ws) => {
    ws.onMessage((raw) => {
      let msg: { type?: string; topic?: string };
      try {
        msg = JSON.parse(typeof raw === "string" ? raw : raw.toString());
      } catch {
        return;
      }
      if (msg.type === "subscribe" && msg.topic === "stats") {
        ws.send(JSON.stringify({ topic: "stats", ts: Date.now(), payload: stats }));
      }
    });
  });
}

test.beforeEach(async ({ page }) => {
  await mockWs(page);
});

const VIEWPORTS = [
  { name: "mobile", width: 390, height: 844 },
  { name: "tablet", width: 820, height: 1180 },
  { name: "desktop", width: 1440, height: 900 },
];

// Pagine da verificare via deep-link (hash) e heading atteso (null = niente h2
// di pagina, es. Tool che ha solo la barra dei tab).
const PAGES: { hash: string; heading: string | null }[] = [
  { hash: "#/dashboard", heading: "Dashboard" },
  { hash: "#/net", heading: "Rete" },
  { hash: "#/docker", heading: "Docker" },
  { hash: "#/tool", heading: null },
  { hash: "#/log", heading: "Log" },
  { hash: "#/snippets", heading: "Snippet" },
  { hash: "#/ssh", heading: "SSH" },
  { hash: "#/settings", heading: "Impostazioni" },
];

for (const vp of VIEWPORTS) {
  test(`nessun overflow orizzontale su tutte le pagine @ ${vp.name}`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    for (const p of PAGES) {
      await page.goto(`/${p.hash}`);
      if (p.heading) {
        await expect(page.getByRole("heading", { name: p.heading, exact: true }).first()).toBeVisible();
      } else {
        await expect(page.locator(".tool-tabbar")).toBeVisible();
      }
      // Il body non deve MAI scrollare orizzontalmente (regola d'oro responsive).
      const m = await page.evaluate(() => ({
        scrollW: document.documentElement.scrollWidth,
        clientW: document.documentElement.clientWidth,
      }));
      expect(m.scrollW, `overflow su ${p.hash} @ ${vp.name}`).toBeLessThanOrEqual(m.clientW + 1);
    }
  });
}

test("command palette: Ctrl+K apre, filtra e naviga", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Dashboard", exact: true })).toBeVisible();
  await page.keyboard.press("Control+k");
  const input = page.locator(".cmdk-input");
  await expect(input).toBeVisible();
  await input.fill("dock");
  await expect(page.locator(".cmdk-item").first()).toContainText("Docker");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Docker", exact: true })).toBeVisible();
});

test("command palette: Esc chiude", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Dashboard", exact: true })).toBeVisible();
  await page.keyboard.press("Control+k");
  await expect(page.locator(".cmdk-input")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".cmdk")).toBeHidden();
});

test("deep-link storico #/clipboard apre Tool sul tab Appunti", async ({ page }) => {
  await page.goto("/#/clipboard");
  await expect(page.locator(".tool-tabbar")).toBeVisible();
  await expect(page.locator(".tool-tabbar button", { hasText: "Appunti" })).toHaveClass(/active/);
});

test("Rete apre di default sul tab Porte in ascolto", async ({ page }) => {
  await page.goto("/#/net");
  await expect(page.getByRole("heading", { name: "Rete", exact: true })).toBeVisible();
  await expect(
    page.locator(".segmented button", { hasText: "Porte in ascolto" }),
  ).toHaveClass(/active/);
});

test("persistenza: l'ultima pagina è ricordata dopo un reload", async ({ page }) => {
  await page.goto("/#/ssh");
  await expect(page.getByRole("heading", { name: "SSH", exact: true })).toBeVisible();
  // Torna sulla root senza hash: deve ripristinare l'ultima pagina da localStorage.
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "SSH", exact: true })).toBeVisible();
});
