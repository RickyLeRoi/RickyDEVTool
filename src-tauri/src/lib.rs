mod adapters;
mod collectors;
mod config;
mod events;
mod netinfo;
mod poller;
mod server;
mod services;
mod tasks;

use std::sync::{Arc, OnceLock};

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::config::ConfigHandle;
use crate::events::EventBus;
use crate::poller::PollerRegistry;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

    #[cfg(target_os = "macos")]
    fix_gui_path();

    let config = ConfigHandle::load();
    let bus = EventBus::new();
    let pollers = Arc::new(PollerRegistry::new(bus.clone()));
    collectors::register_all(&pollers, &config);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let info = tauri::async_runtime::block_on(server::start(
                config.clone(),
                bus.clone(),
                pollers.clone(),
            ))?;
            tracing::info!(port = info.port, "RickyDEVTool avviato");

            let window_url = if cfg!(debug_assertions) {
                // Dev: Vite con HMR; la SPA parla comunque col server su info.port.
                "http://localhost:1420".to_string()
            } else {
                format!("http://127.0.0.1:{}", info.port)
            };
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(window_url.parse()?))
                .title("RickyDEVTool")
                .inner_size(1100.0, 720.0)
                .min_inner_size(900.0, 600.0)
                .build()?;

            setup_tray(app.handle(), info)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Chiudere la finestra non termina l'app: il server resta su per la LAN.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("errore in avvio dell'applicazione tauri");
}

fn setup_tray(app: &tauri::AppHandle, info: server::ServerInfo) -> tauri::Result<()> {
    let lan_label = match (info.lan_enabled, netinfo::lan_ips().first()) {
        (true, Some(ip)) => format!("LAN: http://{ip}:{}", info.port),
        (true, None) => "LAN: nessun IP trovato".to_string(),
        (false, _) => "LAN disattivata (solo localhost)".to_string(),
    };

    let open_item = MenuItemBuilder::with_id("open", "Apri RickyDEVTool").build(app)?;
    let lan_item = MenuItemBuilder::with_id("lan", &lan_label).build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Esci").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .separator()
        .item(&lan_item)
        .separator()
        .item(&quit_item)
        .build()?;

    let lan_url = netinfo::lan_ips()
        .first()
        .map(|ip| format!("http://{ip}:{}", info.port));

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("icona app").clone())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "lan" => {
                if let Some(url) = &lan_url {
                    let _ = tauri_plugin_opener::open_url(url.clone(), None::<String>);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn init_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let logs_dir = config::data_dir().join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "rickydevtool.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false),
        )
        .init();
}

/// Le app GUI su macOS non ereditano il PATH della shell di login:
/// senza questo fix git/node/dotnet non si troverebbero quando serviranno.
#[cfg(target_os = "macos")]
fn fix_gui_path() {
    let output = std::process::Command::new("/bin/zsh")
        .args(["-lc", "echo -n $PATH"])
        .output();
    if let Ok(output) = output {
        if output.status.success() {
            if let Ok(path) = String::from_utf8(output.stdout) {
                if !path.trim().is_empty() {
                    std::env::set_var("PATH", path.trim());
                }
            }
        }
    }
}
