import { create } from "zustand";

// Intento arrivato dal menu del tray (evento "tray-navigate"): la sezione
// gestisce il proprio state locale, questo store serve solo a comunicare
// "apri X" una volta. `seq` cambia a ogni evento anche se section/extra sono
// identici, così un secondo click sulla stessa voce riattiva l'effetto.
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
