import { create } from "zustand";
import type { Update, DownloadEvent } from "@tauri-apps/plugin-updater";

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
    if (!isTauri) {
      if (manual) set({ phase: "uptodate", update: null });
      return;
    }
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
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      set({ phase: "error", error: e instanceof Error ? e.message : String(e) });
    }
  },

  dismiss: () => set({ dismissed: true }),
}));
