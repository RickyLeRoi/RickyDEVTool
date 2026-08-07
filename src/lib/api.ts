import { LOOPBACK_HOST, SERVER_PORT, SERVER_PORT_FALLBACK_RANGE, WS_PATH } from "./constants";
import type { ApiResult } from "./types";

const port = Number(window.location.port);
export const API_BASE =
  port >= SERVER_PORT && port < SERVER_PORT + SERVER_PORT_FALLBACK_RANGE
    ? ""
    : `http://${LOOPBACK_HOST}:${SERVER_PORT}`;

export const WS_URL = API_BASE
  ? API_BASE.replace(/^http/, "ws") + WS_PATH
  : `ws://${window.location.host}${WS_PATH}`;

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
