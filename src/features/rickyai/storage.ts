import {
  AI_CONTEXT_MESSAGES,
  AI_MAX_THREADS,
  AI_TITLE_MAX_CHARS,
  STORAGE_KEYS,
} from "../../lib/constants";
import { DEFAULT_AI_MODEL, DEFAULT_AI_THREAD_TITLE } from "../../lib/defaults";

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

export function newId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export function newThread(): AiThread {
  return { id: newId(), title: DEFAULT_AI_THREAD_TITLE, messages: [], updatedAt: Date.now() };
}

export function titleFor(text: string): string {
  const firstLine = text.trim().split("\n")[0].trim();
  if (!firstLine) return DEFAULT_AI_THREAD_TITLE;
  return firstLine.length > AI_TITLE_MAX_CHARS
    ? `${firstLine.slice(0, AI_TITLE_MAX_CHARS)}…`
    : firstLine;
}

export function conversationFor(thread: AiThread): { role: string; content: string }[] {
  return thread.messages
    .slice(-AI_CONTEXT_MESSAGES)
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
    raw = localStorage.getItem(STORAGE_KEYS.aiThreads);
  } catch {
    return [];
  }
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isThread).slice(0, AI_MAX_THREADS);
  } catch {
    return [];
  }
}

export function saveThreads(threads: AiThread[]): void {
  const trimmed = [...threads]
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .slice(0, AI_MAX_THREADS);
  try {
    localStorage.setItem(STORAGE_KEYS.aiThreads, JSON.stringify(trimmed));
  } catch {
  }
}

export function loadModel(): string {
  try {
    return localStorage.getItem(STORAGE_KEYS.aiModel) || DEFAULT_AI_MODEL;
  } catch {
    return DEFAULT_AI_MODEL;
  }
}

export function saveModel(model: string): void {
  try {
    localStorage.setItem(STORAGE_KEYS.aiModel, model);
  } catch {
  }
}
