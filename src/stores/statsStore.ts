import { create } from "zustand";
import type { MachineStats } from "../lib/types";

const HISTORY_WINDOW = 60;

interface StatsState {
  latest: MachineStats | null;
  cpuHistory: number[];
  memHistory: number[];
  error: string | null;
  push: (stats: MachineStats) => void;
  setError: (message: string) => void;
}

export const useStatsStore = create<StatsState>((set) => ({
  latest: null,
  cpuHistory: [],
  memHistory: [],
  error: null,
  push: (stats) =>
    set((s) => ({
      latest: stats,
      error: null,
      cpuHistory: [...s.cpuHistory, stats.cpuTotalPct].slice(-HISTORY_WINDOW),
      memHistory: [...s.memHistory, stats.mem.usedPct].slice(-HISTORY_WINDOW),
    })),
  setError: (message) => set({ error: message }),
}));
