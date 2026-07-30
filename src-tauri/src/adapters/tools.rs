use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::exec;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredTool {
    pub id: &'static str,
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub source: &'static str, // wellKnownPath | registry | PATH | userConfig | none
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_note: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub editions: Vec<ToolEdition>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEdition {
    pub label: String,
    pub path: String,
}

pub const TOOL_IDS: &[&str] = &[
    "vscode", "visualstudio", "git", "node", "npm", "yarn", "pnpm", "dotnet", "docker", "terminal",
];

/// Discovery completa. `overrides` (da config) vince su tutto.
pub async fn discover_all(overrides: &HashMap<String, String>) -> Vec<DiscoveredTool> {
    let mut tools = Vec::with_capacity(TOOL_IDS.len());
    for id in TOOL_IDS {
        tools.push(discover_one(id, overrides.get(*id).map(String::as_str)).await);
    }
    tools
}

async fn discover_one(id: &'static str, user_override: Option<&str>) -> DiscoveredTool {
    if let Some(path) = user_override {
        let exists = Path::new(path).exists();
        return DiscoveredTool {
            id,
            found: exists,
            path: Some(path.to_string()),
            version: if exists { version_of(id, path).await } else { None },
            source: "userConfig",
            platform_note: (!exists).then(|| "Il path configurato non esiste".to_string()),
            editions: Vec::new(),
        };
    }
    match id {
        "vscode" => discover_vscode().await,
        "visualstudio" => discover_visual_studio().await,
        "terminal" => discover_terminal().await,
        cli => discover_cli(cli).await,
    }
}

fn not_found(id: &'static str, note: Option<String>) -> DiscoveredTool {
    DiscoveredTool {
        id,
        found: false,
        path: None,
        version: None,
        source: "none",
        platform_note: note,
        editions: Vec::new(),
    }
}

fn found_at(id: &'static str, path: String, source: &'static str) -> DiscoveredTool {
    DiscoveredTool {
        id,
        found: true,
        path: Some(path),
        version: None,
        source,
        platform_note: None,
        editions: Vec::new(),
    }
}

// ---------- tool CLI generici (git, node, npm, yarn, pnpm, dotnet, docker) ----------

async fn discover_cli(id: &'static str) -> DiscoveredTool {
    let Some(path) = which(id).await else {
        return not_found(id, None);
    };
    let mut tool = found_at(id, path.clone(), "PATH");
    tool.version = version_of(id, &path).await;
    tool
}

async fn which(name: &str) -> Option<String> {
    #[cfg(windows)]
    let (cmd, arg) = ("where", name);
    #[cfg(not(windows))]
    let (cmd, arg) = ("/usr/bin/which", name);

    let out = exec::text(exec::cmd(cmd).arg(arg)).await?;
    let path = out.lines().next()?.trim().to_string();
    (!path.is_empty()).then_some(path)
}

async fn version_of(id: &str, path: &str) -> Option<String> {
    let arg = match id {
        "dotnet" => "--version",
        "docker" => "--version",
        _ => "--version",
    };
    let out = exec::text_within(exec::cmd(path).arg(arg), Duration::from_secs(4)).await?;
    let first = out.lines().next()?.trim().to_string();
    (!first.is_empty()).then_some(first)
}

// ---------- VS Code ----------

#[cfg(target_os = "macos")]
async fn discover_vscode() -> DiscoveredTool {
    let bundles = vec![
        "/Applications/Visual Studio Code.app".to_string(),
        format!("{}/Applications/Visual Studio Code.app", home()),
    ];
    for bundle in &bundles {
        if Path::new(bundle).exists() {
            let mut tool = found_at("vscode", bundle.to_string(), "wellKnownPath");
            // La CLI `code` dentro il bundle dà la versione senza dipendere dal PATH.
            let cli = format!("{bundle}/Contents/Resources/app/bin/code");
            tool.version = version_of("vscode", &cli).await;
            return tool;
        }
    }
    match which("code").await {
        Some(path) => {
            let mut tool = found_at("vscode", path.clone(), "PATH");
            tool.version = version_of("vscode", &path).await;
            tool
        }
        None => not_found("vscode", None),
    }
}

#[cfg(target_os = "windows")]
async fn discover_vscode() -> DiscoveredTool {
    let candidates = [
        format!("{}\\Programs\\Microsoft VS Code\\Code.exe", env_or("LOCALAPPDATA", "")),
        format!("{}\\Microsoft VS Code\\Code.exe", env_or("ProgramFiles", "C:\\Program Files")),
    ];
    for path in candidates {
        if Path::new(&path).exists() {
            return found_at("vscode", path, "wellKnownPath");
        }
    }
    match which("code").await {
        Some(path) => found_at("vscode", path, "PATH"),
        None => not_found("vscode", None),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn discover_vscode() -> DiscoveredTool {
    match which("code").await {
        Some(path) => found_at("vscode", path, "PATH"),
        None => not_found("vscode", None),
    }
}

// ---------- Visual Studio ----------

#[cfg(target_os = "windows")]
async fn discover_visual_studio() -> DiscoveredTool {
    let vswhere = format!(
        "{}\\Microsoft Visual Studio\\Installer\\vswhere.exe",
        env_or("ProgramFiles(x86)", "C:\\Program Files (x86)")
    );
    if !Path::new(&vswhere).exists() {
        return not_found("visualstudio", Some("vswhere non trovato: nessun VS ≥2017 installato".into()));
    }
    // -prerelease: senza questo flag vswhere esclude le edizioni Preview/Insiders
    // (es. VS 2026 finché resta in preview), facendole risultare "non trovate".
    let cmd = &mut exec::cmd(&vswhere);
    cmd.args(["-all", "-prerelease", "-products", "*", "-format", "json"]);
    let Some(output) = exec::text(cmd).await else {
        return not_found("visualstudio", Some("vswhere eseguito ma fallito".into()));
    };
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap_or_default();
    let editions: Vec<ToolEdition> = parsed
        .iter()
        .filter_map(|v| {
            Some(ToolEdition {
                label: format!(
                    "{} {}",
                    v.get("displayName")?.as_str()?,
                    v.get("catalog")?.get("productDisplayVersion")?.as_str().unwrap_or("")
                ),
                path: v.get("productPath")?.as_str()?.to_string(),
            })
        })
        .collect();
    match editions.first() {
        Some(first) => DiscoveredTool {
            id: "visualstudio",
            found: true,
            path: Some(first.path.clone()),
            version: Some(first.label.clone()),
            source: "registry",
            platform_note: None,
            editions,
        },
        None => not_found("visualstudio", Some("Nessuna installazione VS trovata da vswhere".into())),
    }
}

#[cfg(not(target_os = "windows"))]
async fn discover_visual_studio() -> DiscoveredTool {
    not_found(
        "visualstudio",
        Some("Visual Studio è solo Windows (VS for Mac è stato ritirato nel 2024): usa VS Code o Rider".into()),
    )
}

// ---------- Terminale ----------

#[cfg(target_os = "macos")]
async fn discover_terminal() -> DiscoveredTool {
    if Path::new("/Applications/iTerm.app").exists() {
        return found_at("terminal", "/Applications/iTerm.app".into(), "wellKnownPath");
    }
    found_at("terminal", "/System/Applications/Utilities/Terminal.app".into(), "wellKnownPath")
}

#[cfg(target_os = "windows")]
async fn discover_terminal() -> DiscoveredTool {
    match which("wt").await {
        Some(path) => found_at("terminal", path, "PATH"),
        None => found_at("terminal", "cmd".into(), "wellKnownPath"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn discover_terminal() -> DiscoveredTool {
    for candidate in ["x-terminal-emulator", "gnome-terminal", "konsole"] {
        if let Some(path) = which(candidate).await {
            return found_at("terminal", path, "PATH");
        }
    }
    not_found("terminal", None)
}

// ---------- lancio ----------

/// Avvia un tool, opzionalmente su una cartella/soluzione.
pub async fn launch(tool: &DiscoveredTool, target: Option<&str>) -> Result<(), String> {
    let path = tool.path.as_deref().ok_or("tool non trovato")?;
    if let Some(t) = target {
        if !Path::new(t).exists() {
            return Err(format!("il percorso {t} non esiste"));
        }
    }
    match tool.id {
        "vscode" => launch_vscode(path, target),
        "visualstudio" => launch_visual_studio(path, target),
        "terminal" => launch_terminal(path, target),
        _ => Err(format!("il tool {} non è avviabile direttamente", tool.id)),
    }
}

#[cfg(target_os = "macos")]
fn launch_vscode(path: &str, target: Option<&str>) -> Result<(), String> {
    let mut cmd = exec::sync_cmd("open");
    cmd.args(["-a", path]);
    if let Some(t) = target {
        cmd.arg(t);
    }
    spawn(cmd)
}

#[cfg(not(target_os = "macos"))]
fn launch_vscode(path: &str, target: Option<&str>) -> Result<(), String> {
    let mut cmd = exec::sync_cmd(path);
    if let Some(t) = target {
        cmd.arg(t);
    }
    spawn(cmd)
}

#[cfg(target_os = "windows")]
fn launch_visual_studio(path: &str, target: Option<&str>) -> Result<(), String> {
    let mut cmd = exec::sync_cmd(path);
    if let Some(t) = target {
        cmd.arg(t);
    }
    spawn(cmd)
}

#[cfg(not(target_os = "windows"))]
fn launch_visual_studio(_path: &str, _target: Option<&str>) -> Result<(), String> {
    Err("Visual Studio è disponibile solo su Windows".into())
}

#[cfg(target_os = "macos")]
fn launch_terminal(path: &str, target: Option<&str>) -> Result<(), String> {
    let mut cmd = exec::sync_cmd("open");
    cmd.args(["-a", path]);
    if let Some(t) = target {
        cmd.arg(t);
    }
    spawn(cmd)
}

#[cfg(target_os = "windows")]
fn launch_terminal(path: &str, target: Option<&str>) -> Result<(), String> {
    let dir = target.unwrap_or(".");
    if path.ends_with("wt") || path.ends_with("wt.exe") {
        let mut c = exec::sync_cmd(path);
        c.args(["-d", dir]);
        spawn(c)
    } else {
        // Qui la console è il prodotto, non un effetto collaterale: `start`
        // apre il terminale che l'utente ha chiesto.
        let mut c = exec::sync_cmd_with_console("cmd");
        c.args(["/c", "start", "cmd", "/K", "cd", "/d", dir]);
        spawn(c)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch_terminal(path: &str, target: Option<&str>) -> Result<(), String> {
    let mut cmd = exec::sync_cmd(path);
    if let Some(t) = target {
        cmd.current_dir(t);
    }
    spawn(cmd)
}

fn spawn(mut cmd: std::process::Command) -> Result<(), String> {
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[cfg(target_os = "macos")]
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/Users".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovery_trova_git_e_node() {
        let tools = discover_all(&HashMap::new()).await;
        let get = |id: &str| tools.iter().find(|t| t.id == id).unwrap();
        // git e node esistono sulla macchina di sviluppo e in CI.
        assert!(get("git").found, "git non trovato");
        assert!(get("node").found, "node non trovato");
        // Visual Studio su non-Windows deve essere assente con nota.
        #[cfg(not(target_os = "windows"))]
        {
            let vs = get("visualstudio");
            assert!(!vs.found);
            assert!(vs.platform_note.is_some());
        }
        // Il terminale si trova sempre su mac/win.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        assert!(get("terminal").found);
    }

    #[tokio::test]
    async fn override_utente_vince() {
        let mut overrides = HashMap::new();
        overrides.insert("git".to_string(), "/percorso/inesistente".to_string());
        let tools = discover_all(&overrides).await;
        let git = tools.iter().find(|t| t.id == "git").unwrap();
        assert_eq!(git.source, "userConfig");
        assert!(!git.found);
    }

    #[tokio::test]
    #[ignore = "contract test per-OS: risolve git nel PATH e ne legge la versione (--ignored)"]
    async fn contract_discovery_git_reale() {
        let git = discover_cli("git").await;
        assert!(git.found, "git non risolto nel PATH");
        assert_eq!(git.source, "PATH");
        assert!(git.path.is_some());
        let version = git.version.expect("versione di git");
        assert!(
            version.to_lowercase().contains("git version"),
            "output versione inatteso: {version:?}"
        );
    }
}
