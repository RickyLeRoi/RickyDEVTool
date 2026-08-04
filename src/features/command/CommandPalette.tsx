import { useEffect, useMemo, useRef, useState } from "react";

export interface Command {
  id: string;
  title: string;
  hint?: string;
  keywords?: string;
  icon?: string;
  run: () => void;
}

function subsequence(query: string, target: string): boolean {
  if (!query) return true;
  let i = 0;
  for (const ch of target) {
    if (ch === query[i]) i++;
    if (i === query.length) return true;
  }
  return i === query.length;
}

function score(query: string, cmd: Command): number | null {
  const q = query.toLowerCase().trim();
  const title = cmd.title.toLowerCase();
  const hay = `${title} ${(cmd.keywords ?? "").toLowerCase()}`;
  if (!q) return 0;
  if (title.startsWith(q)) return 3;
  if (title.includes(q)) return 2;
  if (subsequence(q, hay)) return 1;
  return null;
}

export function CommandPalette({
  open,
  onClose,
  commands,
}: {
  open: boolean;
  onClose: () => void;
  commands: Command[];
}) {
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const results = useMemo(() => {
    return commands
      .map((c) => ({ c, s: score(query, c) }))
      .filter((r): r is { c: Command; s: number } => r.s !== null)
      .sort((a, b) => b.s - a.s)
      .map((r) => r.c);
  }, [commands, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSel(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  useEffect(() => {
    setSel(0);
  }, [query]);

  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-idx="${sel}"]`)?.scrollIntoView({
      block: "nearest",
    });
  }, [sel]);

  if (!open) return null;

  const runAt = (idx: number) => {
    const cmd = results[idx];
    if (!cmd) return;
    onClose();
    cmd.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      runAt(sel);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  return (
    <div className="cmdk-overlay" onClick={onClose}>
      <div className="cmdk" onClick={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="cmdk-input"
          placeholder="Vai a… o cerca un'azione"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="cmdk-list" ref={listRef}>
          {results.length === 0 && <div className="cmdk-empty">Nessun risultato</div>}
          {results.map((c, idx) => (
            <button
              key={c.id}
              data-idx={idx}
              className={`cmdk-item ${idx === sel ? "active" : ""}`}
              onMouseMove={() => setSel(idx)}
              onClick={() => runAt(idx)}
            >
              <span className="cmdk-icon">{c.icon ?? "›"}</span>
              <span className="cmdk-title">{c.title}</span>
              {c.hint && <span className="cmdk-hint">{c.hint}</span>}
            </button>
          ))}
        </div>
        <div className="cmdk-footer">
          <span>↑↓ per muoverti</span>
          <span>↵ per aprire</span>
          <span>esc per chiudere</span>
        </div>
      </div>
    </div>
  );
}
