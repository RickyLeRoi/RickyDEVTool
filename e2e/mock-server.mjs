// Backend fake per gli smoke test Playwright: serve la SPA buildata (dist/) e
// risponde alle REST minime che servono al primo render (health, alerts,
// metriche, drop). Il WebSocket NON è gestito qui: lo intercetta e lo mocka
// Playwright con page.routeWebSocket (vedi e2e/smoke.spec.ts), così i dati di
// dashboard/porte arrivano dal test senza un vero canale WS.
//
// Nessuna dipendenza esterna: solo i moduli built-in di Node.
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

function sendJson(res, obj, status = 200) {
  const body = JSON.stringify(obj);
  res.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  res.end(body);
}

// Risposte REST canned. Le poche che ritornano array mappati dalla UI vanno
// esplicitate, altrimenti un `data` vuoto farebbe crashare il render.
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
    case "/api/metrics/history":
      return sendJson(res, { ok: true, data: { samples: [], hours: Number(url.searchParams.get("hours") ?? 24) } });
    case "/api/drop/self":
      return sendJson(res, { ok: true, data: { hubId: "mock-hub" } });
    case "/api/drop/hello":
      return sendJson(res, { ok: true, data: { peers: [] } });
    default:
      // Default permissivo: ok con data vuoto. Copre i POST idempotenti (ack,
      // interval…) toccati dagli smoke test senza bisogno di logica dedicata.
      return sendJson(res, { ok: true, data: {} });
  }
}

async function serveStatic(req, res, url) {
  // Path traversal guard: risolvi dentro DIST, fallback su index.html (SPA).
  const rel = normalize(decodeURIComponent(url.pathname)).replace(/^(\.\.[/\\])+/, "");
  let filePath = join(DIST, rel);
  if (!filePath.startsWith(DIST)) filePath = join(DIST, "index.html");
  try {
    const data = await readFile(filePath);
    res.writeHead(200, { "Content-Type": MIME[extname(filePath)] ?? "application/octet-stream" });
    res.end(data);
  } catch {
    // File non trovato → è una route SPA: servi index.html.
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
