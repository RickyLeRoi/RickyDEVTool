import { create } from "zustand";
import { STATS_HISTORY_POINTS } from "../lib/constants";
import type { MachineStats } from "../lib/types";

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
      cpuHistory: [...s.cpuHistory, stats.cpuTotalPct].slice(-STATS_HISTORY_POINTS),
      memHistory: [...s.memHistory, stats.mem.usedPct].slice(-STATS_HISTORY_POINTS),
    })),
  setError: (message) => set({ error: message }),
}));
