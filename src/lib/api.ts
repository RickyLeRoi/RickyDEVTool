import type { ApiResult } from "./types";

// Se la pagina è servita dal backend (6969-6978, anche via IP LAN) l'API è same-origin.
// Altrimenti (Vite in dev sulla 1420, o webview dev) si punta al default locale.
const port = Number(window.location.port);
export const API_BASE =
  port >= 6969 && port < 6979 ? "" : "http://127.0.0.1:6969";

export const WS_URL = API_BASE
  ? API_BASE.replace(/^http/, "ws") + "/ws"
  : `ws://${window.location.host}/ws`;

export async function api<T>(
  path: string,
  init?: RequestInit,
): Promise<ApiResult<T>> {
  try {
    const res = await fetch(API_BASE + path, {
      headers: { "Content-Type": "application/json" },
      ...init,
    });
    return (await res.json()) as ApiResult<T>;
  } catch (e) {
    return {
      ok: false,
      error: {
        code: "NETWORK",
        message: e instanceof Error ? e.message : "errore di rete",
        retryable: true,
      },
    };
  }
}

export function post<T>(path: string, body: unknown): Promise<ApiResult<T>> {
  return api<T>(path, { method: "POST", body: JSON.stringify(body) });
}
