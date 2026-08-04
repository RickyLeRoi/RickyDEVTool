import { defineConfig, devices } from "@playwright/test";

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
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
