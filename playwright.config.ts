import { defineConfig, devices } from "@playwright/test";

// Smoke test della SPA contro un backend fake (e2e/mock-server.mjs) + WebSocket
// mockato in-browser da Playwright. Nessun runtime Rust necessario.
//
// Prerequisito: la SPA deve essere buildata (`npm run build`). Lo script
// `npm run test:e2e` lo fa prima di lanciare i test.
const PORT = 6970;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: "list",
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "node e2e/mock-server.mjs",
    env: { PORT: String(PORT) },
    url: `http://localhost:${PORT}/api/health`,
    // Sempre un server fresco: mai riusare un RickyDEVTool reale eventualmente
    // in ascolto sulla 6969 (falserebbe gli smoke test con dati veri).
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
