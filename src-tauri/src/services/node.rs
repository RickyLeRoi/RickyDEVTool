use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageManager {
    Npm,
    Yarn,
    Pnpm,
}

impl PackageManager {
    pub fn command(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Yarn => "yarn",
            PackageManager::Pnpm => "pnpm",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "npm" => Some(Self::Npm),
            "yarn" => Some(Self::Yarn),
            "pnpm" => Some(Self::Pnpm),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeProject {
    pub path: String,
    pub package_name: Option<String>,
    pub package_manager: PackageManager,
    pub pm_source: &'static str,
    pub scripts: BTreeMap<String, String>,
    pub primary_start: Option<String>,
    pub node_modules_present: bool,
}

pub fn inspect(path: &str, user_override: Option<&str>) -> Result<NodeProject, String> {
    let dir = Path::new(path);
    let package_json = dir.join("package.json");
    let raw = std::fs::read_to_string(&package_json)
        .map_err(|_| "package.json non trovato nella cartella".to_string())?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("package.json non valido: {e}"))?;

    let scripts: BTreeMap<String, String> = parsed
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let (package_manager, pm_source) = detect_pm(dir, &parsed, user_override);

    let primary_start = ["start", "dev", "serve"]
        .iter()
        .find(|s| scripts.contains_key(**s))
        .map(|s| s.to_string());

    Ok(NodeProject {
        path: path.to_string(),
        package_name: parsed.get("name").and_then(|n| n.as_str()).map(String::from),
        package_manager,
        pm_source,
        scripts,
        primary_start,
        node_modules_present: dir.join("node_modules").is_dir(),
    })
}

fn detect_pm(
    dir: &Path,
    package_json: &serde_json::Value,
    user_override: Option<&str>,
) -> (PackageManager, &'static str) {
    if let Some(pm) = user_override.and_then(PackageManager::from_str) {
        return (pm, "userOverride");
    }
    if dir.join("pnpm-lock.yaml").is_file() {
        return (PackageManager::Pnpm, "lockfile");
    }
    if dir.join("yarn.lock").is_file() {
        return (PackageManager::Yarn, "lockfile");
    }
    if dir.join("package-lock.json").is_file() {
        return (PackageManager::Npm, "lockfile");
    }
    if let Some(field) = package_json.get("packageManager").and_then(|v| v.as_str()) {
        if let Some(pm) = PackageManager::from_str(field.split('@').next().unwrap_or("")) {
            return (pm, "packageManagerField");
        }
    }
    (PackageManager::Npm, "default")
}

pub fn command_for(pm: PackageManager, script: Option<&str>) -> (String, Vec<String>) {
    let cmd = pm.command().to_string();
    match script {
        None => (cmd, vec!["install".to_string()]),
        Some(s) => (cmd, vec!["run".to_string(), s.to_string()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(package_json: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), package_json).unwrap();
        dir
    }

    #[test]
    fn rileva_pm_da_lockfile_con_priorita() {
        let dir = setup(r#"{"name":"x","packageManager":"yarn@4.0.0"}"#);
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        std::fs::write(dir.path().join("package-lock.json"), "").unwrap();
        let p = inspect(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(p.package_manager, PackageManager::Pnpm);
        assert_eq!(p.pm_source, "lockfile");
    }

    #[test]
    fn rileva_pm_da_campo_package_manager() {
        let dir = setup(r#"{"name":"x","packageManager":"yarn@4.0.0"}"#);
        let p = inspect(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(p.package_manager, PackageManager::Yarn);
        assert_eq!(p.pm_source, "packageManagerField");
    }

    #[test]
    fn override_utente_vince_su_tutto() {
        let dir = setup(r#"{"name":"x"}"#);
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let p = inspect(dir.path().to_str().unwrap(), Some("pnpm")).unwrap();
        assert_eq!(p.package_manager, PackageManager::Pnpm);
        assert_eq!(p.pm_source, "userOverride");
    }

    #[test]
    fn primary_start_preferisce_start_poi_dev() {
        let dir = setup(r#"{"scripts":{"dev":"vite","build":"vite build"}}"#);
        let p = inspect(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(p.primary_start.as_deref(), Some("dev"));
        assert_eq!(p.scripts.len(), 2);
    }
}
