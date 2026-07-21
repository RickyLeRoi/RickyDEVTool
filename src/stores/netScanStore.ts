import { create } from "zustand";
import type { LanHost } from "../lib/types";

// Fuori dall'albero React: i risultati sopravvivono al cambio tool/sezione
// (lo Scan LAN viene smontato quando si lascia "Rete"). Una nuova scansione
// sostituisce semplicemente il contenuto.
interface NetScanState {
  hosts: LanHost[] | null;
  setHosts: (hosts: LanHost[]) => void;
}

export const useNetScanStore = create<NetScanState>((set) => ({
  hosts: null,
  setHosts: (hosts) => set({ hosts }),
}));
