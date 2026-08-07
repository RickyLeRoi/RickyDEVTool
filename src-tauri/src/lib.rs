mod adapters;
mod alerts;
mod collectors;
mod config;
mod events;
mod exec;
mod jiggler;
mod netinfo;
mod notify;
mod poller;
mod server;
mod services;
mod tasks;
mod tray;
mod window;

use std::sync::{Arc, OnceLock};

use tauri::{WebviewUrl, WebviewWindowBuilder, WindowEvent};

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

    let config_for_close = config.clone();

    let builder = tauri::Builder::default();

    // 20260807 ++ RG va registrato per primo: la seconda istanza esce prima che il server provi a legare la porta.
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        window::show_main(app);
    }));

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            let info = tauri::async_runtime::block_on(server::start(
                config.clone(),
                bus.clone(),
                pollers.clone(),
            ))?;
            tracing::info!(port = info.port, "RickyDEVTool avviato");

            let window_url = if cfg!(debug_assertions) {
                "http://localhost:1420".to_string()
            } else {
                format!("http://127.0.0.1:{}", info.port)
            };
            WebviewWindowBuilder::new(
                app,
                window::MAIN_LABEL,
                WebviewUrl::External(window_url.parse()?),
            )
            .title("RickyDEVTool")
            .inner_size(1100.0, 720.0)
            .min_inner_size(900.0, 600.0)
            .build()?;

            window::show_main(app.handle());

            tray::setup(app.handle(), info)?;

            #[cfg(target_os = "macos")]
            {
                let granted = adapters::localnetwork::status().granted;
                tracing::info!(granted, "permesso rete locale");
            }

            Ok(())
        })
        // 20260807 ++ RG #CloseToTray la X nasconde nel tray o chiude davvero, a scelta dell'utente.
        .on_window_event(move |window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if config_for_close.get().close_to_tray {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("errore in avvio dell'applicazione tauri");

    app.run(|_handle, event| match event {
        // 20260807 ++ RG #WindowFocus click sull'icona nel Dock
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => window::show_main(_handle),
        tauri::RunEvent::Exit => services::rickyai::shutdown_all(),
        _ => {}
    });
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

#[cfg(target_os = "macos")]
fn fix_gui_path() {
    let output = crate::exec::sync_cmd("/bin/zsh")
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
