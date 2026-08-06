import { WS_URL } from "./api";
import type { WsEvent } from "./types";

type Handler = (event: WsEvent) => void;

class WsClient {
  private socket: WebSocket | null = null;
  private handlers = new Map<string, Set<Handler>>();
  // 20260806 ++ RG #Security l'auth va ricordata per topic: alla riconnessione il server rifà il
  // controllo da zero e senza segreto le sottoscrizioni drop: verrebbero negate.
  private auth = new Map<string, string>();
  private backoffMs = 500;
  private closed = false;

  private sendSubscribe(socket: WebSocket, topic: string) {
    const auth = this.auth.get(topic);
    socket.send(JSON.stringify({ type: "subscribe", topic, ...(auth ? { auth } : {}) }));
  }

  connect() {
    if (this.socket || this.closed) return;
    const socket = new WebSocket(WS_URL);
    this.socket = socket;

    socket.onopen = () => {
      this.backoffMs = 500;
      for (const topic of this.handlers.keys()) {
        this.sendSubscribe(socket, topic);
      }
    };
    socket.onmessage = (raw) => {
      let event: WsEvent;
      try {
        event = JSON.parse(raw.data as string);
      } catch {
        return;
      }
      const base = event.topic.split(":")[0];
      for (const topic of [event.topic, base]) {
        this.handlers.get(topic)?.forEach((h) => h(event));
        if (event.topic === base) break;
      }
    };
    socket.onclose = () => {
      this.socket = null;
      if (this.closed) return;
      setTimeout(() => this.connect(), this.backoffMs);
      this.backoffMs = Math.min(this.backoffMs * 2, 10_000);
    };
    socket.onerror = () => socket.close();
  }

  // 20260806 ++ RG #Security le rivendicazioni dei canali drop: vivono in RAM nel server: se
  // riparte, la sottoscrizione va riaffermata o il device smette di ricevere.
  resubscribe(topic: string) {
    if (this.handlers.has(topic) && this.socket?.readyState === WebSocket.OPEN) {
      this.sendSubscribe(this.socket, topic);
    }
  }

  subscribe(topic: string, handler: Handler, auth?: string): () => void {
    this.connect();
    if (auth) this.auth.set(topic, auth);
    let set = this.handlers.get(topic);
    const isNewTopic = !set;
    if (!set) {
      set = new Set();
      this.handlers.set(topic, set);
    }
    set.add(handler);
    if (isNewTopic && this.socket?.readyState === WebSocket.OPEN) {
      this.sendSubscribe(this.socket, topic);
    }
    return () => {
      const handlers = this.handlers.get(topic);
      handlers?.delete(handler);
      if (handlers && handlers.size === 0) {
        this.handlers.delete(topic);
        this.auth.delete(topic);
        if (this.socket?.readyState === WebSocket.OPEN) {
          this.socket.send(JSON.stringify({ type: "unsubscribe", topic }));
        }
      }
    };
  }
}

export const ws = new WsClient();
