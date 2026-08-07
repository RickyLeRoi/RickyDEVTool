export const isTauri = "__TAURI_INTERNALS__" in window;

// 20260807 ++ RG #CloseToTray hide() e non minimize()
export async function hideToTray() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().hide();
}
