import { test, expect, type Page } from "@playwright/test";

const stats = {
  ts: Date.now(),
  cpuTotalPct: 42,
  cores: [
    { core: 0, pct: 40 },
    { core: 1, pct: 44 },
  ],
  mem: { totalBytes: 16 * 1024 ** 3, usedBytes: 8 * 1024 ** 3, usedPct: 50 },
  swap: { totalBytes: 2 * 1024 ** 3, usedBytes: 0 },
  intervalMs: 10000,
};

const portScan = {
  ports: [
    {
      port: 3000,
      protocol: "tcp",
      addresses: ["127.0.0.1"],
      processes: [
        {
          pid: 4821,
          name: "node",
          exePath: "/usr/local/bin/node",
          user: "ricky",
          startedAt: null,
          isSystem: false,
          knownApp: "node",
          killProtection: "confirm",
          zombie: true,
        },
      ],
    },
    {
      port: 5432,
      protocol: "tcp",
      addresses: ["127.0.0.1"],
      processes: [
        {
          pid: 900,
          name: "postgres",
          exePath: null,
          user: "ricky",
          startedAt: null,
          isSystem: false,
          knownApp: "postgres",
          killProtection: "typed-confirm",
          zombie: false,
        },
      ],
    },
  ],
  hiddenSystem: 3,
  sampledAt: Date.now(),
};

function cannedFor(topic: string): unknown | null {
  switch (topic) {
    case "stats":
      return stats;
    case "ports":
      return portScan;
    case "disks":
      return { disks: [] };
    default:
      return null;
  }
}

async function mockWs(page: Page) {
  await page.routeWebSocket(/\/ws$/, (ws) => {
    ws.onMessage((raw) => {
      let msg: { type?: string; topic?: string };
      try {
        msg = JSON.parse(typeof raw === "string" ? raw : raw.toString());
      } catch {
        return;
      }
      if (msg.type === "subscribe" && msg.topic) {
        const payload = cannedFor(msg.topic);
        if (payload != null) {
          ws.send(JSON.stringify({ topic: msg.topic, ts: Date.now(), payload }));
        }
      }
    });
  });
}

test.beforeEach(async ({ page }) => {
  await mockWs(page);
});

test("la shell carica e la dashboard mostra la CPU dal push WS", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
  await expect(page.locator(".gauge-value").filter({ hasText: "42%" })).toBeVisible();
});

test("le Porte in ascolto (tab di Rete) mostrano il badge zombie", async ({ page }) => {
  await page.goto("/#/ports");
  await expect(page.getByRole("heading", { name: "Porte in ascolto" })).toBeVisible();
  await expect(page.getByText("3000")).toBeVisible();
  await expect(page.getByText("zombie").first()).toBeVisible();
});

test("il deep-link #/ports apre le Porte in ascolto dentro Rete", async ({ page }) => {
  await page.goto("/#/ports");
  await expect(page.getByRole("heading", { name: "Rete", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Porte in ascolto" })).toBeVisible();
});
