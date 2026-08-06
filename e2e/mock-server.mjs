import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const DIST = join(ROOT, "..", "dist");
const PORT = Number(process.env.PORT ?? 6970);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".json": "application/json; charset=utf-8",
  ".woff2": "font/woff2",
};

function metricSamples() {
  const now = Date.now();
  return Array.from({ length: 20 }, (_, i) => ({
    ts: now - (19 - i) * 30_000,
    cpuPct: 30 + i,
    memPct: 50 + (i % 5),
    diskPct: 70,
  }));
}

function aiStatus() {
  return {
    state: "ready",
    port: 4141,
    baseUrl: "http://127.0.0.1:4141",
    managed: true,
    command: "/usr/local/bin/of-free",
    message: null,
    startedAt: Date.now() - 60_000,
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
      {
        name: "groq",
        label: "Groq",
        available: true,
        headroom: 0.82,
        local: false,
        limits: [
          { unit: "req", window: "day", remaining: 11800, limit: 14400, authoritative: true },
        ],
      },
      {
        name: "ollama",
        label: "Ollama (local)",
        available: true,
        headroom: 1,
        local: true,
        limits: [],
      },
    ],
    next: { provider: "groq", model: "llama-3.3-70b-versatile" },
    models: ["auto", "groq/llama-3.3-70b-versatile", "ollama/qwen2.5:7b"],
  };
}

function readBody(req) {
  return new Promise((resolve) => {
    let raw = "";
    req.on("data", (chunk) => (raw += chunk));
    req.on("end", () => {
      try {
        resolve(JSON.parse(raw || "{}"));
      } catch {
        resolve({});
      }
    });
  });
}

async function aiChat(req, res) {
  const body = await readBody(req);
  const messages = Array.isArray(body.messages) ? body.messages : [];
  const last = [...messages].reverse().find((m) => m?.role === "user");
  return sendJson(res, {
    ok: true,
    data: {
      content: `RickyAI dice: ${last?.content ?? "(niente)"}`,
      provider: "groq",
      model: "llama-3.3-70b-versatile",
      failovers: 0,
      repinned: null,
      finishReason: "stop",
      usage: { promptTokens: 12, completionTokens: 8, totalTokens: 20 },
      elapsedMs: 420,
    },
  });
}

function sendJson(res, obj, status = 200) {
  const body = JSON.stringify(obj);
  res.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  res.end(body);
}

function handleApi(req, res, url) {
  switch (url.pathname) {
    case "/api/health":
      return sendJson(res, {
        ok: true,
        data: { name: "RickyDEVTool", version: "0.0.0-mock", port: PORT, os: "mock" },
      });
    case "/api/alerts":
      return sendJson(res, { ok: true, data: { alerts: [] } });
    case "/api/tasks":
      return sendJson(res, { ok: true, data: { tasks: [] } });
    case "/api/launch/bundles":
      return sendJson(res, { ok: true, data: { bundles: [] } });
    case "/api/docker":
      return sendJson(res, { ok: true, data: { available: false, daemonDown: false, containers: [] } });
    case "/api/metrics/history":
      return sendJson(res, {
        ok: true,
        data: { samples: metricSamples(), hours: Number(url.searchParams.get("hours") ?? 24) },
      });
    case "/api/fs/compare":
      return sendJson(res, {
        ok: true,
        data: {
          left: "/mock/a",
          right: "/mock/b",
          compared: 5,
          identical: 2,
          truncated: false,
          entries: [
            { relPath: "diverso.txt", status: "different", isDir: false, leftSize: 2048, rightSize: 1024, leftMtime: null, rightMtime: null },
            { relPath: "solo-sx", status: "onlyLeft", isDir: true, leftSize: 0, rightSize: null, leftMtime: null, rightMtime: null },
          ],
        },
      });
    case "/api/fs/compare/children":
      return sendJson(res, {
        ok: true,
        data: {
          entries: [
            { relPath: "solo-sx/uno.txt", status: "onlyLeft", isDir: false, leftSize: 10, rightSize: null, leftMtime: null, rightMtime: null },
          ],
        },
      });
    case "/api/drop/self":
      return sendJson(res, { ok: true, data: { hubId: "mock-hub", isDesktop: true } });
    case "/api/config/hub-code":
      return sendJson(res, { ok: true, data: { code: "" } });
    case "/api/pair/sessions":
      return sendJson(res, { ok: true, data: { sessions: [] } });
    case "/api/drop/hello":
      return sendJson(res, { ok: true, data: { peers: [] } });
    case "/api/clipboard/history":
      return sendJson(res, { ok: true, data: { entries: [], enabled: true, supported: true } });
    case "/api/snippets":
      return sendJson(res, { ok: true, data: { snippets: [] } });
    case "/api/ssh/hosts":
      return sendJson(res, { ok: true, data: { hosts: [] } });
    case "/api/logtail":
      return sendJson(res, { ok: true, data: { tails: [] } });
    case "/api/scheduler":
      return sendJson(res, { ok: true, data: { supported: true, entries: [], note: null } });
    case "/api/alerts/config":
      return sendJson(res, {
        ok: true,
        data: { cpuPct: 90, memPct: 92, tempC: 85, batteryPct: 15, tempEnabled: true, batteryEnabled: true },
      });
    case "/api/lan":
      return sendJson(res, {
        ok: true,
        data: { urls: [], port: PORT, lanEnabled: false, remoteControlEnabled: false, antiIdleEnabled: false, remote: false },
      });
    case "/api/system/accessibility":
      return sendJson(res, { ok: true, data: { supported: false, trusted: false } });
    case "/api/tools":
      return sendJson(res, { ok: true, data: { tools: [] } });
    case "/api/ai/status":
      return sendJson(res, { ok: true, data: aiStatus() });
    case "/api/ai/chat":
      return aiChat(req, res);
    case "/api/ai/config":
      return sendJson(res, { ok: true, data: aiStatus() });
    default:
      return sendJson(res, { ok: true, data: {} });
  }
}

async function serveStatic(req, res, url) {
  const rel = normalize(decodeURIComponent(url.pathname)).replace(/^(\.\.[/\\])+/, "");
  let filePath = join(DIST, rel);
  if (!filePath.startsWith(DIST)) filePath = join(DIST, "index.html");
  try {
    const data = await readFile(filePath);
    res.writeHead(200, { "Content-Type": MIME[extname(filePath)] ?? "application/octet-stream" });
    res.end(data);
  } catch {
    try {
      const html = await readFile(join(DIST, "index.html"));
      res.writeHead(200, { "Content-Type": MIME[".html"] });
      res.end(html);
    } catch {
      res.writeHead(500);
      res.end("dist/ mancante: esegui `npm run build` prima degli e2e");
    }
  }
}

const server = createServer((req, res) => {
  const url = new URL(req.url ?? "/", `http://localhost:${PORT}`);
  if (url.pathname.startsWith("/api/")) return handleApi(req, res, url);
  return serveStatic(req, res, url);
});

server.listen(PORT, () => {
  console.log(`[mock] RickyDEVTool fake server su http://localhost:${PORT}`);
});
