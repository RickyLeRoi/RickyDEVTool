import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import {
  conversationFor,
  loadModel,
  loadThreads,
  newId,
  newThread,
  saveModel,
  saveThreads,
  titleFor,
  type AiChatMessage,
  type AiThread,
} from "./storage";
import type { AiReply, AiState, AiStatus, ApiError } from "../../lib/types";

const STATE_LABEL: Record<AiState, string> = {
  ready: "pronto",
  starting: "in avvio…",
  notInstalled: "of-free non installato",
  failed: "non disponibile",
  disabled: "disattivato",
};

const SUGGESTIONS = [
  "Spiegami questo errore di build",
  "Scrivimi uno script bash che…",
  "Come funziona un process group su unix?",
];

function modelLabel(id: string): string {
  if (id === "auto") return "Automatico (miglior quota)";
  if (id === "private") return "Solo locale (privato)";
  return id;
}

function StatusBar({
  status,
  onRestart,
  restarting,
}: {
  status: AiStatus | null;
  onRestart: () => void;
  restarting: boolean;
}) {
  if (!status) return <div className="ai-status dim">Verifico RickyAI…</div>;

  const ready = status.state === "ready";
  const badgeClass = ready ? "badge-ok" : status.state === "starting" ? "" : "badge-warn";
  const usable = (status.providers ?? []).filter((p) => p.available);

  return (
    <div className="ai-status">
      <div className="ai-status-line">
        <span className={`badge ${badgeClass}`}>{STATE_LABEL[status.state]}</span>
        {ready && (
          <span className="dim" title={`endpoint OpenAI-compatibile su ${status.baseUrl}/v1`}>
            {status.mode === "remote"
              ? `${status.ofFree ? "of-free in rete" : "endpoint OpenAI"} · ${status.baseUrl}`
              : `${status.managed ? "of-free" : "of-free (istanza esterna)"} · porta ${
                  status.port
                } · strategia ${status.strategy}`}
          </span>
        )}
        {ready && !status.ofFree && (
          <span className="dim">nessun routing fra provider</span>
        )}
        {ready && status.next && (
          <span className="dim">
            prossima richiesta → <strong>{status.next.provider}</strong> {status.next.model}
          </span>
        )}
        <button className="small" onClick={onRestart} disabled={restarting}>
          {restarting ? "Riavvio…" : "Riavvia"}
        </button>
      </div>

      {ready && usable.length > 0 && (
        <div className="ai-providers">
          {usable.map((p) => (
            <span
              key={p.name}
              className="ai-provider"
              title={p.limits
                .map((l) => `${l.unit}/${l.window}: ${l.remaining ?? "?"}/${l.limit ?? "?"}`)
                .join(" · ")}
            >
              {p.label}
              <span className="ai-provider-bar">
                <span
                  className="ai-provider-fill"
                  style={{ width: `${Math.round(Math.max(0, Math.min(1, p.headroom)) * 100)}%` }}
                />
              </span>
              {Math.round(p.headroom * 100)}%
            </span>
          ))}
        </div>
      )}

      {!ready && (
        <div className="banner banner-warn ai-status-detail">
          <div>{status.message ?? "RickyAI non è disponibile."}</div>
          {status.state === "notInstalled" && (
            <div className="hint">
              RickyAI usa <code>of-free</code> (OnFeather Free): installalo con{" "}
              <code>pip install -e .</code> dal repo <code>onfeather-free</code>, indica il
              percorso del binario dalle impostazioni, oppure — se lo fai già girare altrove —
              passa a <strong>Servizio in rete</strong> e mettine l'indirizzo. Le chiavi dei
              provider si incollano nelle impostazioni; senza nessuna chiave restano i soli
              modelli locali via Ollama.
            </div>
          )}
          {status.log.length > 0 && (
            <pre className="ai-log">{status.log.slice(-8).join("\n")}</pre>
          )}
        </div>
      )}
    </div>
  );
}

function MessageBubble({ message }: { message: AiChatMessage }) {
  const mine = message.role === "user";
  return (
    <div className={`ai-msg ${mine ? "ai-msg-user" : "ai-msg-bot"}`}>
      <div className="ai-msg-body">{message.content}</div>
      {!mine && message.provider && (
        <div className="ai-msg-meta">
          {message.provider}
          {message.model ? ` · ${message.model}` : ""}
          {message.elapsedMs != null ? ` · ${(message.elapsedMs / 1000).toFixed(1)}s` : ""}
          {message.failovers ? ` · ${message.failovers} failover` : ""}
        </div>
      )}
    </div>
  );
}

export function RickyAI() {
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [threads, setThreads] = useState<AiThread[]>(() => {
    const stored = loadThreads();
    return stored.length > 0 ? stored : [newThread()];
  });
  const [activeId, setActiveId] = useState<string>(() => threads[0].id);
  const [input, setInput] = useState("");
  const [model, setModel] = useState<string>(() => loadModel());
  const [sending, setSending] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const active = useMemo(
    () => threads.find((t) => t.id === activeId) ?? threads[0],
    [threads, activeId],
  );

  useEffect(() => saveThreads(threads), [threads]);
  useEffect(() => saveModel(model), [model]);

  const refresh = useCallback(async () => {
    const r = await api<AiStatus>("/api/ai/status");
    if (r.ok) setStatus(r.data);
  }, []);

  useEffect(() => {
    refresh();
    // Il supervisore pubblica su "ai" a ogni cambio di stato (avvio, caduta,
    // riavvio): la pagina si aggiorna da sola invece di fare polling.
    return ws.subscribe("ai", () => {
      refresh();
    });
  }, [refresh]);

  // La chat scorre in fondo a ogni messaggio nuovo, come ci si aspetta.
  useEffect(() => {
    const list = listRef.current;
    if (list) list.scrollTop = list.scrollHeight;
  }, [active.messages, sending]);

  const patchThread = (id: string, f: (t: AiThread) => AiThread) =>
    setThreads((prev) => prev.map((t) => (t.id === id ? f(t) : t)));

  /// Invia `conversation` e appende la risposta al thread. Usata sia dal primo
  /// invio sia dal "Riprova": il messaggio dell'utente è già nel thread in
  const ask = async (threadId: string, conversation: { role: string; content: string }[]) => {
    setSending(true);
    setError(null);
    const r = await post<AiReply>("/api/ai/chat", { messages: conversation, model: selectedModel });
    setSending(false);
    if (!r.ok) {
      setError(r.error);
      return;
    }
    const reply: AiChatMessage = {
      id: newId(),
      role: "assistant",
      content: r.data.content,
      at: Date.now(),
      provider: r.data.provider,
      model: r.data.model,
      failovers: r.data.failovers,
      elapsedMs: r.data.elapsedMs,
    };
    patchThread(threadId, (t) => ({
      ...t,
      messages: [...t.messages, reply],
      updatedAt: Date.now(),
    }));
  };

  const send = async () => {
    const text = input.trim();
    if (!text || sending) return;
    const message: AiChatMessage = { id: newId(), role: "user", content: text, at: Date.now() };
    const thread = active;
    patchThread(thread.id, (t) => ({
      ...t,
      title: t.messages.length === 0 ? titleFor(text) : t.title,
      messages: [...t.messages, message],
      updatedAt: Date.now(),
    }));
    setInput("");
    await ask(thread.id, [...conversationFor(thread), { role: "user", content: text }]);
  };

  // 20260804 ++ RG #RickyAI il messaggio dell'utente resta nel thread apposta: dopo un 429 si
  // riprova quello, non lo si fa riscrivere.
  const retry = () => {
    if (sending) return;
    const conversation = conversationFor(active);
    if (conversation[conversation.length - 1]?.role !== "user") return;
    ask(active.id, conversation);
  };

  const restart = async () => {
    setRestarting(true);
    await post("/api/ai/restart", {});
    setRestarting(false);
    refresh();
  };

  const startNewThread = () => {
    if (active.messages.length === 0) return;
    const thread = newThread();
    setThreads((prev) => [thread, ...prev]);
    setActiveId(thread.id);
    setError(null);
    setInput("");
  };

  const deleteThread = (id: string) => {
    const left = threads.filter((t) => t.id !== id);
    const next = left.length > 0 ? left : [newThread()];
    setThreads(next);
    if (id === activeId) setActiveId(next[0].id);
  };

  const models = useMemo(() => {
    const listed = (status?.models ?? []).filter((m) => m !== "auto");
    return status?.ofFree === false ? listed : ["auto", "private", ...listed];
  }, [status?.models, status?.ofFree]);

  const selectedModel = models.includes(model) ? model : models[0] ?? "auto";

  const blocked = status != null && status.state !== "ready";

  return (
    <div className="rickyai">
      <div className="section-header">
        <h2>RickyAI</h2>
        <div className="ai-head-actions">
          <select
            className="ai-model"
            value={selectedModel}
            onChange={(e) => setModel(e.target.value)}
            title="Quale modello usare"
          >
            {models.map((m) => (
              <option key={m} value={m}>
                {modelLabel(m)}
              </option>
            ))}
          </select>
          <button className="small" onClick={startNewThread} disabled={active.messages.length === 0}>
            + Nuova chat
          </button>
        </div>
      </div>

      <StatusBar status={status} onRestart={restart} restarting={restarting} />

      <div className="ai-body">
        <aside className="ai-threads">
          {threads.map((t) => (
            <div
              key={t.id}
              className={`ai-thread ${t.id === active.id ? "active" : ""}`}
              onClick={() => {
                setActiveId(t.id);
                setError(null);
              }}
            >
              <span className="ai-thread-title">{t.title}</span>
              <button
                className="ai-thread-del"
                title="Elimina questa chat"
                aria-label="Elimina chat"
                onClick={(e) => {
                  e.stopPropagation();
                  deleteThread(t.id);
                }}
              >
                ×
              </button>
            </div>
          ))}
        </aside>

        <div className="ai-chat">
          <div className="ai-messages" ref={listRef}>
            {active.messages.length === 0 && (
              <div className="ai-empty">
                <div className="ai-empty-title">Chiedi qualcosa a RickyAI</div>
                <div className="hint">
                  Le richieste passano da <code>of-free</code>, che le smista sul provider gratuito
                  con più quota rimasta. La conversazione resta su questo dispositivo.
                </div>
                <div className="ai-suggestions">
                  {SUGGESTIONS.map((s) => (
                    <button key={s} className="small ghost" onClick={() => setInput(s)}>
                      {s}
                    </button>
                  ))}
                </div>
              </div>
            )}
            {active.messages.map((m) => (
              <MessageBubble key={m.id} message={m} />
            ))}
            {sending && (
              <div className="ai-msg ai-msg-bot">
                <div className="ai-typing">
                  {
}
                  <span />
                  <span />
                  <span />
                </div>
              </div>
            )}
          </div>

          {error && (
            <div className="banner banner-error ai-error">
              <div>{error.message}</div>
              <div className="ai-error-actions">
                {error.retryAfter != null && (
                  <span className="hint">riprovabile tra ~{error.retryAfter}s</span>
                )}
                <button className="small" onClick={retry} disabled={sending}>
                  Riprova
                </button>
                <button className="small ghost" onClick={() => setError(null)}>
                  Chiudi
                </button>
              </div>
            </div>
          )}

          <form
            className="ai-composer"
            onSubmit={(e) => {
              e.preventDefault();
              send();
            }}
          >
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={
                blocked ? "RickyAI non è disponibile" : "Scrivi un messaggio… (Invio per inviare)"
              }
              rows={2}
              disabled={blocked}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  send();
                }
              }}
            />
            <button className="primary" disabled={sending || blocked || !input.trim()}>
              {sending ? "…" : "Invia"}
            </button>
          </form>
        </div>
      </div>
    </div>
  );
}
