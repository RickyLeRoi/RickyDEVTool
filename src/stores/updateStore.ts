import { create } from "zustand";
import type { Update, DownloadEvent } from "@tauri-apps/plugin-updater";

// Stato dell'auto-updater condiviso tra il banner (auto-check all'avvio) e il
// pulsante "Verifica aggiornamenti" della sezione About.
//   idle      → non ancora controllato
//   checking  → controllo in corso
//   available → aggiornamento trovato (mostra il banner)
//   downloading
//   uptodate  → controllato, già all'ultima versione
//   error
export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "uptodate"
  | "error";

const isTauri = "__TAURI_INTERNALS__" in window;

interface UpdateState {
  phase: UpdatePhase;
  update: Update | null;
  progress: number;
  error: string | null;
  dismissed: boolean;
  /** Controlla se c'è un aggiornamento. `manual` distingue il click utente dall'auto-check. */
  check: (manual?: boolean) => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  phase: "idle",
  update: null,
  progress: 0,
  error: null,
  dismissed: false,

  check: async (manual = false) => {
    // L'updater esiste solo nella finestra desktop; da browser LAN non si applica.
    if (!isTauri) {
      if (manual) set({ phase: "uptodate", update: null });
      return;
    }
    // Evita check concorrenti (es. auto-check + click ravvicinati).
    if (get().phase === "checking" || get().phase === "downloading") return;
    set({ phase: "checking", error: null, dismissed: false });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const found = await check();
      if (found) set({ update: found, phase: "available" });
      else set({ phase: "uptodate", update: null });
    } catch (e) {
      set({ phase: "error", error: e instanceof Error ? e.message : String(e) });
    }
  },

  install: async () => {
    const update = get().update;
    if (!update) return;
    set({ phase: "downloading", error: null, progress: 0 });
    try {
      let total = 0;
      let done = 0;
      await update.downloadAndInstall((ev: DownloadEvent) => {
        switch (ev.event) {
          case "Started":
            total = ev.data.contentLength ?? 0;
            break;
          case "Progress":
            done += ev.data.chunkLength;
            if (total > 0) set({ progress: Math.min(100, Math.round((done / total) * 100)) });
            break;
          case "Finished":
            set({ progress: 100 });
            break;
        }
      });
      // Riavvia sulla nuova versione.
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      set({ phase: "error", error: e instanceof Error ? e.message : String(e) });
    }
  },

  dismiss: () => set({ dismissed: true }),
}));
