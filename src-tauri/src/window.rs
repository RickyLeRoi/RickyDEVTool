use tauri::{AppHandle, Manager};

use crate::constants::MAIN_WINDOW_LABEL;

// 20260807 ++ RG #WindowFocus unminimize prima di show
pub fn show_main(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.show();

    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}
