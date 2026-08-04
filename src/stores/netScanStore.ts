import { create } from "zustand";
import type { LanHost } from "../lib/types";

interface NetScanState {
  hosts: LanHost[] | null;
  setHosts: (hosts: LanHost[]) => void;
}

export const useNetScanStore = create<NetScanState>((set) => ({
  hosts: null,
  setHosts: (hosts) => set({ hosts }),
}));
