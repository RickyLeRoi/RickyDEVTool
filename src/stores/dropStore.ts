import { create } from "zustand";
import type { DropIncoming, DropPeer } from "../lib/types";

export interface IncomingItem {
  id: string;
  at: number;
  data: DropIncoming;
}

interface DropState {
  peers: DropPeer[];
  incoming: IncomingItem[];
  setPeers: (peers: DropPeer[]) => void;
  addIncoming: (data: DropIncoming) => void;
  dismiss: (id: string) => void;
}

export const useDropStore = create<DropState>((set) => ({
  peers: [],
  incoming: [],
  setPeers: (peers) => set({ peers }),
  addIncoming: (data) =>
    set((s) => ({
      incoming: [
        { id: Math.random().toString(36).slice(2), at: Date.now(), data },
        ...s.incoming,
      ].slice(0, 20),
    })),
  dismiss: (id) => set((s) => ({ incoming: s.incoming.filter((i) => i.id !== id) })),
}));
