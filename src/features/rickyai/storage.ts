export interface AiChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  at: number;
  provider?: string | null;
  model?: string | null;
  failovers?: number | null;
  elapsedMs?: number | null;
}

export interface AiThread {
  id: string;
  title: string;
  messages: AiChatMessage[];
  updatedAt: number;
}

const THREADS_KEY = "rdt-rickyai-threads";
const MODEL_KEY = "rdt-rickyai-model";

export const MAX_THREADS = 30;

export const CONTEXT_MESSAGES = 40;

export function newId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export function newThread(): AiThread {
  return { id: newId(), title: "Nuova chat", messages: [], updatedAt: Date.now() };
}

export function titleFor(text: string): string {
  const firstLine = text.trim().split("\n")[0].trim();
  if (!firstLine) return "Nuova chat";
  return firstLine.length > 40 ? `${firstLine.slice(0, 40)}…` : firstLine;
}

export function conversationFor(thread: AiThread): { role: string; content: string }[] {
  return thread.messages
    .slice(-CONTEXT_MESSAGES)
    .map((m) => ({ role: m.role, content: m.content }));
}

function isThread(value: unknown): value is AiThread {
  if (typeof value !== "object" || value === null) return false;
  const t = value as Partial<AiThread>;
  return (
    typeof t.id === "string" &&
    typeof t.title === "string" &&
    Array.isArray(t.messages) &&
    t.messages.every(
      (m) =>
        typeof m?.content === "string" && (m?.role === "user" || m?.role === "assistant"),
    )
  );
}

export function loadThreads(): AiThread[] {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(THREADS_KEY);
  } catch {
    return [];
  }
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isThread).slice(0, MAX_THREADS);
  } catch {
    return [];
  }
}

export function saveThreads(threads: AiThread[]): void {
  const trimmed = [...threads]
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .slice(0, MAX_THREADS);
  try {
    localStorage.setItem(THREADS_KEY, JSON.stringify(trimmed));
  } catch {
  }
}

export function loadModel(): string {
  try {
    return localStorage.getItem(MODEL_KEY) || "auto";
  } catch {
    return "auto";
  }
}

export function saveModel(model: string): void {
  try {
    localStorage.setItem(MODEL_KEY, model);
  } catch {
  }
}
