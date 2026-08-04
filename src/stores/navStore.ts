import { create } from "zustand";

export type Page =
  | "dashboard"
  | "projects"
  | "rickyai"
  | "net"
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
  "rickyai",
  "net",
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

const LEGACY: Record<string, { page: Page; tab: string }> = {
  ports: { page: "net", tab: "listen" },
  services: { page: "net", tab: "services" },
  docker: { page: "net", tab: "docker" },
  launch: { page: "tool", tab: "launch" },
  calc: { page: "tool", tab: "calc" },
  color: { page: "tool", tab: "color" },
  clipboard: { page: "tool", tab: "clipboard" },
  compare: { page: "tool", tab: "compare" },
};

export function resolveTarget(id: string): { page: Page; tab: string | null } {
  // 20260704 RG gli id storici (tray, deep-link salvati) vanno tradotti, non fatti
  // ricadere sulla dashboard.
  if (LEGACY[id]) return LEGACY[id];
  if (PAGE_SET.has(id)) return { page: id as Page, tab: null };
  return { page: "dashboard", tab: null };
}

function initialPage(): Page {
  const stored = localStorage.getItem(LAST_PAGE_KEY);
  if (!stored) return "dashboard";
  return PAGE_SET.has(stored) ? (stored as Page) : resolveTarget(stored).page;
}

interface NavState {
  page: Page;
  tab: string | null;
  seq: number;
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
    set((s) => ({ page, tab: target.tab ?? tab ?? null, seq: s.seq + 1 }));
  },
}));
