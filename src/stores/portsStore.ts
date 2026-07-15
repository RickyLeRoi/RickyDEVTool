import { create } from "zustand";
import type { PortScan } from "../lib/types";

interface PortsState {
  scan: PortScan | null;
  error: string | null;
  setScan: (scan: PortScan) => void;
  setError: (message: string) => void;
}

export const usePortsStore = create<PortsState>((set) => ({
  scan: null,
  error: null,
  setScan: (scan) => set({ scan, error: null }),
  setError: (message) => set({ error: message }),
}));
