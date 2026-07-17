import { create } from "zustand";
import type { DiskInfo } from "../lib/types";

interface DisksState {
  disks: DiskInfo[] | null;
  setDisks: (disks: DiskInfo[]) => void;
}

export const useDisksStore = create<DisksState>((set) => ({
  disks: null,
  setDisks: (disks) => set({ disks }),
}));
