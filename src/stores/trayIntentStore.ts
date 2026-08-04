import { create } from "zustand";

interface TrayIntentState {
  section: string | null;
  extra: string | null;
  seq: number;
  apply: (section: string, extra: string | null) => void;
}

export const useTrayIntentStore = create<TrayIntentState>((set) => ({
  section: null,
  extra: null,
  seq: 0,
  apply: (section, extra) => set((s) => ({ section, extra, seq: s.seq + 1 })),
}));
