import { TAURI_GLOBAL_KEY } from "./constants";

export const isTauri = TAURI_GLOBAL_KEY in window;

// 20260807 ++ RG #CloseToTray hide() e non minimize()
export async function hideToTray() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().hide();
}
