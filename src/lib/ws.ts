import { WS_URL } from "./api";
import type { WsEvent } from "./types";

type Handler = (event: WsEvent) => void;

class WsClient {
  private socket: WebSocket | null = null;
  private handlers = new Map<string, Set<Handler>>();
  private backoffMs = 500;
  private closed = false;

  connect() {
    if (this.socket || this.closed) return;
    const socket = new WebSocket(WS_URL);
    this.socket = socket;

    socket.onopen = () => {
      this.backoffMs = 500;
      for (const topic of this.handlers.keys()) {
        socket.send(JSON.stringify({ type: "subscribe", topic }));
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

  subscribe(topic: string, handler: Handler): () => void {
    this.connect();
    let set = this.handlers.get(topic);
    const isNewTopic = !set;
    if (!set) {
      set = new Set();
      this.handlers.set(topic, set);
    }
    set.add(handler);
    if (isNewTopic && this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify({ type: "subscribe", topic }));
    }
    return () => {
      const handlers = this.handlers.get(topic);
      handlers?.delete(handler);
      if (handlers && handlers.size === 0) {
        this.handlers.delete(topic);
        if (this.socket?.readyState === WebSocket.OPEN) {
          this.socket.send(JSON.stringify({ type: "unsubscribe", topic }));
        }
      }
    };
  }
}

export const ws = new WsClient();
