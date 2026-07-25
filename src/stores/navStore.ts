import { create } from "zustand";

// Pagine di primo livello (voci della rail laterale). Alcune pagine — "tool" e
// "net" — hanno tab interni; il tab richiesto viaggia in `tab`.
export type Page =
  | "dashboard"
  | "projects"
  | "net"
  | "docker"
  | "tool"
  | "log"
  | "snippets"
  | "ssh"
  | "drop"
  | "tasks"
  | "about"
  | "settings";

export const PAGES: readonly Page[] = [
  "dashboard",
  "projects",
  "net",
  "docker",
  "tool",
  "log",
  "snippets",
  "ssh",
  "drop",
  "tasks",
  "about",
  "settings",
];

const PAGE_SET = new Set<string>(PAGES);
const LAST_PAGE_KEY = "rdt-page";

// Sezioni "storiche" accorpate dentro le pagine con tab: il tray, i deep-link
// (#/clipboard) e la command palette continuano a funzionare mappandole qui.
const LEGACY: Record<string, { page: Page; tab: string }> = {
  ports: { page: "net", tab: "listen" },
  services: { page: "net", tab: "services" },
  launch: { page: "tool", tab: "launch" },
  calc: { page: "tool", tab: "calc" },
  color: { page: "tool", tab: "color" },
  clipboard: { page: "tool", tab: "clipboard" },
};

/** Risolve un id (pagina, sezione storica o alias) in pagina + eventuale tab. */
export function resolveTarget(id: string): { page: Page; tab: string | null } {
  if (LEGACY[id]) return LEGACY[id];
  if (PAGE_SET.has(id)) return { page: id as Page, tab: null };
  return { page: "dashboard", tab: null };
}

function initialPage(): Page {
  const stored = localStorage.getItem(LAST_PAGE_KEY);
  return stored && PAGE_SET.has(stored) ? (stored as Page) : "dashboard";
}

interface NavState {
  page: Page;
  /** Tab richiesto dentro la pagina attiva (tool/net). Consumato dalla pagina. */
  tab: string | null;
  /** Bump a ogni navigazione, anche verso la stessa destinazione: consente alle
   *  pagine di riapplicare il tab e ri-lanciare azioni one-shot (es. scan LAN). */
  seq: number;
  /** Naviga per pagina o per id storico (es. "clipboard", "services"). */
  go: (id: string, tab?: string | null) => void;
}

export const useNavStore = create<NavState>((set) => ({
  page: initialPage(),
  tab: null,
  seq: 0,
  go: (id, tab) => {
    const target = resolveTarget(id);
    const page = target.page;
    localStorage.setItem(LAST_PAGE_KEY, page);
    set((s) => ({ page, tab: tab ?? target.tab, seq: s.seq + 1 }));
  },
}));
