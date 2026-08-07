import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
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

const STATE_KEYS: Record<AiState, "rickyai.stateReady"> = {
  ready: "rickyai.stateReady",
  starting: "rickyai.stateStarting" as "rickyai.stateReady",
  notInstalled: "rickyai.stateNotInstalled" as "rickyai.stateReady",
  failed: "rickyai.stateFailed" as "rickyai.stateReady",
  disabled: "rickyai.stateDisabled" as "rickyai.stateReady",
};

const SUGGESTION_KEYS = [
  "rickyai.suggestion1",
  "rickyai.suggestion2",
  "rickyai.suggestion3",
] as const;

function modelLabel(id: string, t: TFunction): string {
  if (id === "auto") return t("rickyai.modelAuto");
  if (id === "private") return t("rickyai.modelPrivate");
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
  const { t } = useTranslation();
  if (!status) return <div className="ai-status dim">{t("rickyai.verifying")}</div>;

  const ready = status.state === "ready";
  const badgeClass = ready ? "badge-ok" : status.state === "starting" ? "" : "badge-warn";
  const usable = (status.providers ?? []).filter((p) => p.available);

  return (
    <div className="ai-status">
      <div className="ai-status-line">
        <span className={`badge ${badgeClass}`}>{t(STATE_KEYS[status.state])}</span>
        {ready && (
          <span className="dim" title={t("rickyai.endpointTitle", { url: status.baseUrl })}>
            {status.mode === "remote"
              ? t("rickyai.readyRemote", {
                  engine: status.ofFree ? t("rickyai.remoteOfFree") : t("rickyai.remoteOpenai"),
                  url: status.baseUrl,
                })
              : t("rickyai.readyLocal", {
                  engine: status.managed ? t("rickyai.localManaged") : t("rickyai.localExternal"),
                  port: status.port,
                  strategy: status.strategy,
                })}
          </span>
        )}
        {ready && !status.ofFree && (
          <span className="dim">{t("rickyai.noRouting")}</span>
        )}
        {ready && status.next && (
          <span className="dim">
            {t("rickyai.nextRequest")} <strong>{status.next.provider}</strong> {status.next.model}
          </span>
        )}
        <button className="small" onClick={onRestart} disabled={restarting}>
          {restarting ? t("rickyai.restarting") : t("rickyai.restart")}
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
          <div>{status.message ?? t("rickyai.notAvailable")}</div>
          {status.state === "notInstalled" && (
            <div className="hint">
              <Trans i18nKey="rickyai.notInstalledHelp" components={{ c: <code />, b: <strong /> }} />
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
  const { t } = useTranslation();
  const mine = message.role === "user";
  return (
    <div className={`ai-msg ${mine ? "ai-msg-user" : "ai-msg-bot"}`}>
      <div className="ai-msg-body">{message.content}</div>
      {!mine && message.provider && (
        <div className="ai-msg-meta">
          {message.provider}
          {message.model ? ` · ${message.model}` : ""}
          {message.elapsedMs != null ? ` · ${(message.elapsedMs / 1000).toFixed(1)}s` : ""}
          {message.failovers ? ` · ${t("rickyai.failover", { count: message.failovers })}` : ""}
        </div>
      )}
    </div>
  );
}

export function RickyAI() {
  const { t } = useTranslation();
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

  // 20260804 RG il messaggio dell'utente resta nel thread apposta: dopo un 429 si riprova quello.
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
        <h2>{t("nav.rickyai")}</h2>
        <div className="ai-head-actions">
          <select
            className="ai-model"
            value={selectedModel}
            onChange={(e) => setModel(e.target.value)}
            title={t("rickyai.modelTitle")}
          >
            {models.map((m) => (
              <option key={m} value={m}>
                {modelLabel(m, t)}
              </option>
            ))}
          </select>
          <button className="small" onClick={startNewThread} disabled={active.messages.length === 0}>
            {t("rickyai.newChat")}
          </button>
        </div>
      </div>

      <StatusBar status={status} onRestart={restart} restarting={restarting} />

      <div className="ai-body">
        <aside className="ai-threads">
          {threads.map((thread) => (
            <div
              key={thread.id}
              className={`ai-thread ${thread.id === active.id ? "active" : ""}`}
              onClick={() => {
                setActiveId(thread.id);
                setError(null);
              }}
            >
              <span className="ai-thread-title">{thread.title}</span>
              <button
                className="ai-thread-del"
                title={t("rickyai.deleteChatTitle")}
                aria-label={t("rickyai.deleteChatAria")}
                onClick={(e) => {
                  e.stopPropagation();
                  deleteThread(thread.id);
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
                <div className="ai-empty-title">{t("rickyai.askSomething")}</div>
                <div className="hint">
                  <Trans i18nKey="rickyai.emptyHint" components={{ c: <code /> }} />
                </div>
                <div className="ai-suggestions">
                  {SUGGESTION_KEYS.map((key) => {
                    const label = t(key);
                    return (
                      <button key={key} className="small ghost" onClick={() => setInput(label)}>
                        {label}
                      </button>
                    );
                  })}
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
                  <span className="hint">{t("rickyai.retryAfter", { seconds: error.retryAfter })}</span>
                )}
                <button className="small" onClick={retry} disabled={sending}>
                  {t("rickyai.retry")}
                </button>
                <button className="small ghost" onClick={() => setError(null)}>
                  {t("common.close")}
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
                blocked ? t("rickyai.composerBlocked") : t("rickyai.composerPlaceholder")
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
              {sending ? "…" : t("rickyai.send")}
            </button>
          </form>
        </div>
      </div>
    </div>
  );
}
