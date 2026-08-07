import { create } from "zustand";
import { LEGACY_NAV_TARGETS, PAGES, STORAGE_KEYS, type Page } from "../lib/constants";
import { DEFAULT_PAGE } from "../lib/defaults";

export type { Page };

const PAGE_SET = new Set<string>(PAGES);

export function resolveTarget(id: string): { page: Page; tab: string | null } {
  // 20260704 RG gli id storici (tray, deep-link salvati) vanno tradotti, non fatti
  // ricadere sulla dashboard.
  if (LEGACY_NAV_TARGETS[id]) return LEGACY_NAV_TARGETS[id];
  if (PAGE_SET.has(id)) return { page: id as Page, tab: null };
  return { page: DEFAULT_PAGE, tab: null };
}

function initialPage(): Page {
  const stored = localStorage.getItem(STORAGE_KEYS.page);
  if (!stored) return DEFAULT_PAGE;
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
    localStorage.setItem(STORAGE_KEYS.page, page);
    set((s) => ({ page, tab: target.tab ?? tab ?? null, seq: s.seq + 1 }));
  },
}));
