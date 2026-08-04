// Le conversazioni di RickyAI vivono nel browser che le ha scritte, non sul
// server: il backend fa solo da proxy verso of-free e non tiene traccia di
// niente. Conseguenza voluta — il telefono ha le sue chat, il desktop le sue, e
// chiudere il tool non lascia in giro i testi delle conversazioni.

export interface AiChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  at: number;
  /** Provenienza della risposta: chi l'ha servita davvero (solo assistant). */
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

/** Tetto ai thread conservati: oltre, si scartano i più vecchi. */
export const MAX_THREADS = 30;

/** Quanti messaggi rispedire come contesto a ogni turno. Tenerli tutti fa
 *  crescere il prompt fino a escludere i modelli con meno tokens al minuto —
 *  che su un piano gratuito sono la maggioranza. */
export const CONTEXT_MESSAGES = 40;

export function newId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export function newThread(): AiThread {
  return { id: newId(), title: "Nuova chat", messages: [], updatedAt: Date.now() };
}

/** Titolo ricavato dal primo messaggio: la prima riga, accorciata. */
export function titleFor(text: string): string {
  const firstLine = text.trim().split("\n")[0].trim();
  if (!firstLine) return "Nuova chat";
  return firstLine.length > 40 ? `${firstLine.slice(0, 40)}…` : firstLine;
}

/** I messaggi da inviare come contesto, nel formato dell'API. */
export function conversationFor(thread: AiThread): { role: string; content: string }[] {
  return thread.messages
    .slice(-CONTEXT_MESSAGES)
    .map((m) => ({ role: m.role, content: m.content }));
}

// Il contenuto di localStorage è dato esterno: una versione precedente, una
// scrittura interrotta o un altro tab possono lasciarci qualsiasi cosa. Si
// valida invece di fidarsi, altrimenti un JSON storto rende la pagina bianca.
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
    // Quota del browser piena: la chat corrente resta in memoria e funziona,
    // si perde solo la persistenza. Non vale un errore a schermo.
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
    /* vedi saveThreads */
  }
}
