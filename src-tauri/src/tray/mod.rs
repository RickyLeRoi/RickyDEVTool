//! Menu contestuale dell'icona nella barra applicazioni.
//! I dati mostrati (CPU/RAM/porte/servizi/strumenti) vengono da uno snapshot
//! rinfrescato in background (vedi `snapshot.rs`): il click sul tray non
//! aspetta mai uno scan o un check servizi dal vivo.

mod snapshot;

use std::sync::Arc;

use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, Submenu, SubmenuBuilder};
use tauri::tray::TrayIconEvent;
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_dialog::DialogExt;

use crate::config::ConfigHandle;
use crate::server::ServerInfo;
use crate::services::drop::DropService;

use snapshot::{SharedSnapshot, TraySnapshot};

const TRAY_ID: &str = "main-tray";
/// Tool discovery id → può essere avviato direttamente (gli altri sono CLI
/// senza una "apertura" diretta: si usano dai pannelli Node/.NET).
const LAUNCHABLE_TOOLS: [&str; 3] = ["vscode", "visualstudio", "terminal"];

pub fn setup(app: &AppHandle, info: ServerInfo) -> tauri::Result<()> {
    let config = info.state.config.clone();
    let drop = info.state.drop.clone();
    let port = info.port;
    let lan_enabled = info.lan_enabled;

    let shared_snapshot = snapshot::start(config.clone());

    let menu = build_menu(app, &shared_snapshot, &config, &drop, port, lan_enabled)?;

    let config_for_menu = config.clone();
    let drop_for_menu = drop.clone();
    let snapshot_for_menu = shared_snapshot.clone();

    let app_for_rebuild = app.clone();
    let config_for_rebuild = config.clone();
    let drop_for_rebuild = drop.clone();
    let snapshot_for_rebuild = shared_snapshot.clone();

    tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().expect("icona app").clone())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref(), &config_for_menu, &drop_for_menu, &snapshot_for_menu, port)
        })
        .on_tray_icon_event(move |tray, event| {
            // Rinfresca SOLO al passaggio del mouse sull'icona (Enter), mai
            // su un timer né al click stesso: un `set_menu` mentre il menu
            // è aperto (o mentre l'OS lo sta aprendo, sul click) lo fa
            // chiudere di scatto — bug osservato con entrambi gli approcci.
            // Il mouse entra sempre nell'icona PRIMA del click, quindi
            // arriviamo comunque con dati freschi.
            if !matches!(event, TrayIconEvent::Enter { .. }) {
                return;
            }
            match build_menu(&app_for_rebuild, &snapshot_for_rebuild, &config_for_rebuild, &drop_for_rebuild, port, lan_enabled) {
                Ok(menu) => {
                    let _ = tray.set_menu(Some(menu));
                }
                Err(e) => tracing::warn!(%e, "tray: rebuild del menu fallito"),
            }
        })
        .build(app)?;

    Ok(())
}

fn build_menu(
    app: &AppHandle,
    snapshot: &SharedSnapshot,
    config: &ConfigHandle,
    drop: &DropService,
    port: u16,
    lan_enabled: bool,
) -> tauri::Result<Menu<Wry>> {
    let snap = snapshot.read().expect("tray snapshot lock").clone();
    let cfg = config.get();

    let system_submenu = build_system_submenu(app, &snap)?;
    let ports_submenu = build_ports_submenu(app, &snap)?;
    let services_submenu = build_services_submenu(app, &snap)?;
    let net_submenu = build_net_submenu(app)?;
    let drop_submenu = build_drop_submenu(app, drop)?;
    let pairing_submenu = build_pairing_submenu(app, &cfg)?;
    let anti_idle_item = CheckMenuItemBuilder::with_id("toggle:anti-idle", "Anti-inattività")
        .checked(cfg.anti_idle_enabled)
        .build(app)?;
    let sections_submenu = build_sections_submenu(app)?;
    let tools_submenu = build_tools_submenu(app, &snap)?;

    let lan_label = match (lan_enabled, crate::netinfo::lan_ips().first()) {
        (true, Some(ip)) => format!("LAN: http://{ip}:{port}"),
        (true, None) => "LAN: nessun IP trovato".to_string(),
        (false, _) => "LAN disattivata (solo localhost)".to_string(),
    };
    let open_item = MenuItemBuilder::with_id("open", "Apri RickyDEVTool").build(app)?;
    let lan_item = MenuItemBuilder::with_id("lan", &lan_label).build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Esci").build(app)?;

    MenuBuilder::new(app)
        .item(&system_submenu)
        .item(&ports_submenu)
        .item(&services_submenu)
        .item(&net_submenu)
        .item(&drop_submenu)
        .item(&pairing_submenu)
        .item(&anti_idle_item)
        .separator()
        .item(&sections_submenu)
        .item(&tools_submenu)
        .separator()
        .item(&open_item)
        .item(&lan_item)
        .separator()
        .item(&quit_item)
        .build()
}

// ---------- sezioni ----------

fn build_system_submenu(app: &AppHandle, snap: &TraySnapshot) -> tauri::Result<Submenu<Wry>> {
    let cpu_item = MenuItemBuilder::new(format!("CPU: {:.0}%", snap.cpu_pct))
        .enabled(false)
        .build(app)?;
    let ram_item = MenuItemBuilder::new(format!("RAM: {:.0}%", snap.ram_pct))
        .enabled(false)
        .build(app)?;
    let mut builder = SubmenuBuilder::new(app, "Sistema")
        .item(&cpu_item)
        .item(&ram_item)
        .separator();

    if snap.disks.is_empty() {
        let none = MenuItemBuilder::new("Dischi: in caricamento…").enabled(false).build(app)?;
        builder = builder.item(&none);
    }
    for disk in &snap.disks {
        let label = format!("{} — {:.0}% usato", disk.name, disk.used_pct);
        if disk.is_removable && !disk.is_system {
            let eject_id = format!("nav:dashboard:{}", disk.mount_point);
            let eject = MenuItemBuilder::with_id(eject_id, "Espelli / gestisci…").build(app)?;
            let sub = SubmenuBuilder::new(app, label).item(&eject).build()?;
            builder = builder.item(&sub);
        } else {
            let item = MenuItemBuilder::new(label).enabled(false).build(app)?;
            builder = builder.item(&item);
        }
    }
    builder.build()
}

const MAX_LISTED: usize = 25;

fn build_ports_submenu(app: &AppHandle, snap: &TraySnapshot) -> tauri::Result<Submenu<Wry>> {
    let mut builder = SubmenuBuilder::new(app, "Porte");
    if snap.ports.is_empty() {
        let none = MenuItemBuilder::new("Nessuna porta in ascolto").enabled(false).build(app)?;
        return builder.item(&none).build();
    }
    for entry in snap.ports.iter().take(MAX_LISTED) {
        let names: Vec<&str> = entry.processes.iter().map(|p| p.name.as_str()).collect();
        let label = format!(
            ":{} — {}",
            entry.port,
            if names.is_empty() { "?".to_string() } else { names.join(", ") }
        );
        let open_item = MenuItemBuilder::with_id(format!("nav:ports:{}", entry.port), "Apri nell'app").build(app)?;
        let mut sub = SubmenuBuilder::new(app, label).item(&open_item);
        if !entry.processes.is_empty() {
            sub = sub.separator();
        }
        // Kill diretto dal tray (con conferma nativa) per i processi normali;
        // i protetti (typed-confirm) e quelli di sistema restano solo nell'app.
        for p in &entry.processes {
            if p.is_system {
                let item = MenuItemBuilder::new(format!("{} (pid {}) — di sistema", p.name, p.pid))
                    .enabled(false)
                    .build(app)?;
                sub = sub.item(&item);
            } else if p.kill_protection == "typed-confirm" {
                let item = MenuItemBuilder::with_id(
                    format!("nav:ports:{}", entry.port),
                    format!("Termina {}… (conferma nell'app)", p.name),
                )
                .build(app)?;
                sub = sub.item(&item);
            } else {
                let item = MenuItemBuilder::with_id(
                    format!("port:kill:{}:{}", entry.port, p.pid),
                    format!("Termina {} (pid {})", p.name, p.pid),
                )
                .build(app)?;
                sub = sub.item(&item);
            }
        }
        builder = builder.item(&sub.build()?);
    }
    if snap.ports.len() > MAX_LISTED {
        let more = MenuItemBuilder::with_id("nav:ports", format!("… e altre {}", snap.ports.len() - MAX_LISTED))
            .build(app)?;
        builder = builder.item(&more);
    }
    builder.build()
}

fn build_services_submenu(app: &AppHandle, snap: &TraySnapshot) -> tauri::Result<Submenu<Wry>> {
    let mut builder = SubmenuBuilder::new(app, "Servizi");
    if snap.services.is_empty() {
        let none = MenuItemBuilder::new("Nessun servizio abilitato").enabled(false).build(app)?;
        return builder.item(&none).build();
    }
    for status in &snap.services {
        use crate::services::online::ServiceState;
        let dot = match status.state {
            ServiceState::Up => "🟢",
            ServiceState::Degraded => "🟡",
            ServiceState::Down => "🔴",
        };
        let detail = status.latency_ms.map(|ms| format!("{ms}ms")).unwrap_or_else(|| "down".to_string());
        let id = format!("nav:services:{}", status.id);
        let item = MenuItemBuilder::with_id(id, format!("{dot} {} — {detail}", status.label)).build(app)?;
        builder = builder.item(&item);
    }
    builder.build()
}

fn build_net_submenu(app: &AppHandle) -> tauri::Result<Submenu<Wry>> {
    let scan_item = MenuItemBuilder::with_id("nav:net:scan", "Scansiona rete locale…").build(app)?;
    SubmenuBuilder::new(app, "Rete").item(&scan_item).build()
}

fn build_drop_submenu(app: &AppHandle, drop: &DropService) -> tauri::Result<Submenu<Wry>> {
    let peers = drop.peers_except("");
    let mut builder = SubmenuBuilder::new(app, "Drop");
    if peers.is_empty() {
        let none = MenuItemBuilder::new("Nessun dispositivo online").enabled(false).build(app)?;
        return builder.item(&none).build();
    }
    for peer in peers.iter().take(MAX_LISTED) {
        let icon = if peer.remote {
            "🌐"
        } else if peer.is_desktop {
            "🖥"
        } else {
            "📱"
        };
        let file_item =
            MenuItemBuilder::with_id(format!("drop:file:{}", peer.device_id), "Invia file…").build(app)?;
        let text_item =
            MenuItemBuilder::with_id(format!("nav:drop:{}", peer.device_id), "Invia testo…").build(app)?;
        let sub = SubmenuBuilder::new(app, format!("{icon} {}", peer.name))
            .item(&file_item)
            .item(&text_item)
            .build()?;
        builder = builder.item(&sub);
    }
    builder.build()
}

fn build_pairing_submenu(app: &AppHandle, cfg: &crate::config::AppConfig) -> tauri::Result<Submenu<Wry>> {
    let qr_item = MenuItemBuilder::with_id("nav:settings:qr", "Mostra QR di abbinamento").build(app)?;
    let remote_item = CheckMenuItemBuilder::with_id("toggle:remote-control", "Controllo remoto")
        .checked(cfg.remote_control_enabled)
        .build(app)?;
    SubmenuBuilder::new(app, "Abbinamento")
        .item(&qr_item)
        .item(&remote_item)
        .build()
}

/// Scorciatoie che aprono l'app direttamente sulla sezione scelta: dà al tray
/// accesso rapido anche alle sezioni senza un proprio sottomenu di dati
/// (Progetti, Docker, Avvii, Appunti, Colori, Calcolatrice, Task, Dashboard).
fn build_sections_submenu(app: &AppHandle) -> tauri::Result<Submenu<Wry>> {
    const SECTIONS: &[(&str, &str)] = &[
        ("dashboard", "🖥 Dashboard"),
        ("projects", "📁 Progetti"),
        ("docker", "🐳 Docker"),
        ("tasks", "🧾 Task"),
        ("launch", "🚀 Avvii"),
        ("clipboard", "📋 Appunti"),
        ("color", "🎨 Colori"),
        ("calc", "🧮 Calcolatrice"),
        ("settings", "⚙️ Impostazioni"),
    ];
    let mut builder = SubmenuBuilder::new(app, "Apri sezione");
    for (id, label) in SECTIONS {
        let item = MenuItemBuilder::with_id(format!("nav:{id}"), *label).build(app)?;
        builder = builder.item(&item);
    }
    builder.build()
}

fn build_tools_submenu(app: &AppHandle, snap: &TraySnapshot) -> tauri::Result<Submenu<Wry>> {
    let found: Vec<_> = snap.tools.iter().filter(|t| t.found).collect();
    let mut builder = SubmenuBuilder::new(app, "Strumenti rilevati");
    if found.is_empty() {
        let none = MenuItemBuilder::new("Nessuno strumento rilevato").enabled(false).build(app)?;
        return builder.item(&none).build();
    }
    for tool in found {
        if LAUNCHABLE_TOOLS.contains(&tool.id) {
            let label = format!("Apri {}", tool_label(tool.id));
            let item = MenuItemBuilder::with_id(format!("tool:launch:{}", tool.id), label).build(app)?;
            builder = builder.item(&item);
        } else {
            let label = match &tool.version {
                Some(v) => format!("{} — {v}", tool_label(tool.id)),
                None => tool_label(tool.id).to_string(),
            };
            let item = MenuItemBuilder::new(label).enabled(false).build(app)?;
            builder = builder.item(&item);
        }
    }
    builder.build()
}

fn tool_label(id: &str) -> &'static str {
    match id {
        "vscode" => "VS Code",
        "visualstudio" => "Visual Studio",
        "git" => "Git",
        "node" => "Node",
        "npm" => "npm",
        "yarn" => "Yarn",
        "pnpm" => "pnpm",
        "dotnet" => ".NET",
        "docker" => "Docker",
        "terminal" => "Terminale",
        _ => "Strumento",
    }
}

// ---------- azioni ----------

fn handle_menu_event(
    app: &AppHandle,
    id: &str,
    config: &ConfigHandle,
    drop: &Arc<DropService>,
    snapshot: &SharedSnapshot,
    port: u16,
) {
    match id {
        "open" => focus_window(app),
        "lan" => {
            if let Some(ip) = crate::netinfo::lan_ips().into_iter().next() {
                let _ = tauri_plugin_opener::open_url(format!("http://{ip}:{port}"), None::<String>);
            }
        }
        "quit" => app.exit(0),
        "toggle:remote-control" => {
            let enabled = !config.get().remote_control_enabled;
            config.update(|c| c.remote_control_enabled = enabled);
            tracing::info!(enabled, "controllo remoto aggiornato dal tray");
        }
        "toggle:anti-idle" => {
            let enabled = !config.get().anti_idle_enabled;
            config.update(|c| c.anti_idle_enabled = enabled);
            tracing::info!(enabled, "anti-inattività aggiornato dal tray");
        }
        _ if id.starts_with("nav:") => navigate(app, id),
        _ if id.starts_with("tool:launch:") => {
            let tool_id = id.trim_start_matches("tool:launch:").to_string();
            launch_tool(snapshot, tool_id);
        }
        _ if id.starts_with("drop:file:") => {
            let device_id = id.trim_start_matches("drop:file:").to_string();
            pick_and_send_file(app, drop.clone(), config.clone(), device_id);
        }
        _ if id.starts_with("port:kill:") => {
            if let Some((port_s, pid_s)) = id.trim_start_matches("port:kill:").split_once(':') {
                if let (Ok(port), Ok(pid)) = (port_s.parse::<u16>(), pid_s.parse::<u32>()) {
                    confirm_and_kill_port_process(app, snapshot, port, pid);
                }
            }
        }
        _ => {}
    }
}

fn focus_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Porta in primo piano la finestra e la naviga sulla sezione giusta via un
/// evento ascoltato dal frontend: le voci del tray che servono UI ricca
/// (invio testo, QR) non possono farlo direttamente da un menu nativo.
fn navigate(app: &AppHandle, id: &str) {
    let mut parts = id.splitn(3, ':');
    parts.next(); // "nav"
    let Some(section) = parts.next() else { return };
    let extra = parts.next();

    focus_window(app);
    let _ = app.emit("tray-navigate", serde_json::json!({ "section": section, "extra": extra }));
}

fn launch_tool(snapshot: &SharedSnapshot, tool_id: String) {
    let tool = snapshot
        .read()
        .expect("tray snapshot lock")
        .tools
        .iter()
        .find(|t| t.id == tool_id && t.found)
        .cloned();
    let Some(tool) = tool else { return };
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::adapters::tools::launch(&tool, None).await {
            tracing::warn!(%e, tool = %tool.id, "tray: avvio strumento fallito");
        }
    });
}

/// Kill diretto dal tray: conferma nativa semplice (non il typed-confirm
/// robusto dell'app, riservato ai processi protetti — quelli restano
/// raggiungibili solo da lì). Il processo va ri-cercato nello snapshot
/// corrente: l'id del menu porta solo (porta, pid), non nome/orario di
/// avvio, che servono a `kill_process` per rifiutare un PID riusato.
fn confirm_and_kill_port_process(app: &AppHandle, snapshot: &SharedSnapshot, port: u16, pid: u32) {
    let proc = snapshot
        .read()
        .expect("tray snapshot lock")
        .ports
        .iter()
        .find(|e| e.port == port)
        .and_then(|entry| entry.processes.iter().find(|p| p.pid == pid))
        .cloned();
    let Some(proc) = proc else { return };

    let app_for_kill = app.clone();
    app.dialog()
        .message(format!("Terminare \"{}\" (pid {}) sulla porta {port}?", proc.name, proc.pid))
        .title("Termina processo")
        .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
        .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancel)
        .show(move |confirmed| {
            if !confirmed {
                return;
            }
            let app_for_error = app_for_kill.clone();
            tauri::async_runtime::spawn(async move {
                let req = crate::adapters::kill::KillRequest {
                    pid: proc.pid,
                    expected_name: proc.name.clone(),
                    expected_started_at: proc.started_at,
                    force: false,
                    confirm_name: None,
                };
                match crate::adapters::kill::kill_process(req).await {
                    Ok(_) => tracing::info!(pid = proc.pid, name = %proc.name, "tray: processo terminato"),
                    Err(e) => {
                        let message = kill_error_message(e);
                        tracing::warn!(pid = proc.pid, %message, "tray: kill fallito");
                        app_for_error
                            .dialog()
                            .message(message)
                            .title("Kill fallito")
                            .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                            .show(|_| {});
                    }
                }
            });
        });
}

fn kill_error_message(e: crate::adapters::kill::KillError) -> String {
    use crate::adapters::kill::KillError;
    match e {
        KillError::ProcessGone => "Il processo non esiste più o il PID è stato riusato".to_string(),
        KillError::SystemProtected => "Processo di sistema: non terminabile".to_string(),
        KillError::TypedConfirmRequired { name } => format!("\"{name}\" richiede conferma scritta: usa l'app"),
        KillError::Failed { message, .. } => message,
    }
}

/// Dialog nativo di selezione file → invio diretto (peer locale: copia su
/// disco; hub remoto: proxy HTTP). Nessuna conferma nel tray: un eventuale
/// errore viene solo loggato e mostrato in un avviso non bloccante.
fn pick_and_send_file(app: &AppHandle, drop: Arc<DropService>, config: ConfigHandle, device_id: String) {
    let app_for_error = app.clone();
    app.dialog().file().pick_file(move |file_path| {
        let Some(file_path) = file_path else { return };
        let Ok(path) = file_path.into_path() else { return };
        let drop = drop.clone();
        let config = config.clone();
        let app_for_error = app_for_error.clone();
        tauri::async_runtime::spawn(async move {
            let from_name = crate::services::hubdiscovery::hub_name(&config);
            if let Err(e) = drop.send_local_file(&device_id, &from_name, &path).await {
                tracing::warn!(%e, device_id, "tray: invio file fallito");
                app_for_error
                    .dialog()
                    .message(e)
                    .title("Invio fallito")
                    .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                    .show(|_| {});
            }
        });
    });
}
