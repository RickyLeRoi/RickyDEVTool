use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use rand::RngCore;
use serde::{Deserialize, Serialize};

pub const APP_DIR_NAME: &str = "RickyDEVTool";
pub const DEFAULT_PORT: u16 = 6969;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub port: u16,
    pub lan_enabled: bool,
    pub pair_token: String,
    pub stats_interval_ms: u64,
    pub tool_paths: std::collections::HashMap<String, String>,
    pub pinned_folders: Vec<String>,
    pub node_pm_overrides: std::collections::HashMap<String, String>,
    pub dotnet_startup: std::collections::HashMap<String, String>,
    pub dotnet_profile: std::collections::HashMap<String, String>,
    pub services: Vec<crate::services::online::ServiceDef>,
    pub remote_control_enabled: bool,
    pub anti_idle_enabled: bool,
    pub push_enabled: bool,
    pub push_server: String,
    pub push_topic: String,
    pub push_min_severity: String,
    pub drop_hub_id: String,
    pub drop_hub_name: String,
    pub launch_bundles: Vec<crate::services::launch::LaunchBundle>,
    #[serde(default)]
    pub docker_host: Option<String>,
    #[serde(default)]
    pub snippets: Vec<crate::services::snippets::Snippet>,
    #[serde(default)]
    pub ssh_hosts: Vec<crate::services::ssh::SshHost>,
    #[serde(default)]
    pub alert_thresholds: AlertThresholds,
    // 20260804 ++ RG #RickyAI niente `#[serde(default)]` su questi campi: scavalca il Default della
    // struct e ricade su quello del tipo, cioè RickyAI spenta su ogni config già esistente.
    pub ai_enabled: bool,
    pub ai_mode: String,
    pub ai_remote_url: Option<String>,
    pub ai_remote_key: Option<String>,
    pub ai_port: u16,
    pub ai_command: Option<String>,
    pub ai_keys: std::collections::BTreeMap<String, String>,
    pub ai_strategy: String,
    pub ai_system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AlertThresholds {
    pub cpu_pct: f64,
    pub mem_pct: f64,
    pub temp_c: f64,
    pub battery_pct: f64,
    pub temp_enabled: bool,
    pub battery_enabled: bool,
}

impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            cpu_pct: 90.0,
            mem_pct: 92.0,
            temp_c: 85.0,
            battery_pct: 15.0,
            temp_enabled: true,
            battery_enabled: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            lan_enabled: true,
            pair_token: String::new(),
            stats_interval_ms: 10_000,
            tool_paths: std::collections::HashMap::new(),
            pinned_folders: Vec::new(),
            node_pm_overrides: std::collections::HashMap::new(),
            dotnet_startup: std::collections::HashMap::new(),
            dotnet_profile: std::collections::HashMap::new(),
            services: Vec::new(),
            remote_control_enabled: false,
            anti_idle_enabled: false,
            push_enabled: false,
            push_server: "https://ntfy.sh".to_string(),
            push_topic: String::new(),
            push_min_severity: "warning".to_string(),
            drop_hub_id: String::new(),
            drop_hub_name: String::new(),
            launch_bundles: Vec::new(),
            docker_host: None,
            snippets: Vec::new(),
            ssh_hosts: Vec::new(),
            alert_thresholds: AlertThresholds::default(),
            ai_enabled: false,
            // 20260804 ++ RG #RickyAI si parte dal servizio in rete: è il caso comune, il locale
            // richiede of-free installato sulla macchina.
            ai_mode: "remote".to_string(),
            ai_remote_url: None,
            ai_remote_key: None,
            ai_port: crate::services::rickyai::DEFAULT_PORT,
            ai_command: None,
            ai_keys: std::collections::BTreeMap::new(),
            ai_strategy: "balanced".to_string(),
            ai_system_prompt: String::new(),
        }
    }
}

#[derive(Clone)]
pub struct ConfigHandle {
    inner: Arc<RwLock<AppConfig>>,
    path: PathBuf,
}

impl ConfigHandle {
    pub fn load() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_DIR_NAME);
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");

        let mut cfg: AppConfig = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        if cfg.pair_token.is_empty() {
            cfg.pair_token = generate_token();
        }
        if cfg.push_topic.is_empty() {
            cfg.push_topic = format!("rickydev-{}", generate_token());
        }
        if cfg.drop_hub_id.is_empty() {
            cfg.drop_hub_id = format!("hub-{}", generate_token());
        }
        for preset in crate::services::online::builtin_presets() {
            if !cfg.services.iter().any(|s| s.id == preset.id) {
                cfg.services.push(preset);
            }
        }
        let handle = Self {
            inner: Arc::new(RwLock::new(cfg)),
            path,
        };
        handle.save();
        handle
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        let mut token = [0u8; 8];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut token);
        let name: String = token.iter().map(|b| format!("{b:02x}")).collect();
        Self {
            inner: Arc::new(RwLock::new(AppConfig::default())),
            path: std::env::temp_dir().join(format!("rickydev-test-{name}.json")),
        }
    }

    pub fn get(&self) -> AppConfig {
        self.inner.read().expect("config lock poisoned").clone()
    }

    pub fn update<F: FnOnce(&mut AppConfig)>(&self, f: F) {
        {
            let mut guard = self.inner.write().expect("config lock poisoned");
            f(&mut guard);
        }
        self.save();
    }

    fn save(&self) {
        let cfg = self.get();
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(&cfg).expect("config serializzabile");
        let written = std::fs::write(&tmp, body)
            .and_then(|_| restrict_to_owner(&tmp))
            .and_then(|_| std::fs::rename(&tmp, &self.path));
        if written.is_err() {
            tracing::error!("impossibile salvare la config in {:?}", self.path);
        }
    }
}

// 20260704 RG la config contiene token di pairing e chiavi API: i permessi vanno messi
// sul file temporaneo prima del rename, o il definitivo esiste un istante a 0644.
fn restrict_to_owner(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn generate_token() -> String {
    let mut buf = [0u8; 16];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_campi_nuovi_prendono_il_default_della_struct_non_del_tipo() {
        let vecchio = r#"{ "port": 6969, "lanEnabled": true, "statsIntervalMs": 5000 }"#;
        let cfg: AppConfig = serde_json::from_str(vecchio).expect("config leggibile");

        assert_eq!(cfg.ai_port, crate::services::rickyai::DEFAULT_PORT);
        assert_eq!(cfg.ai_strategy, "balanced");
        assert_eq!(cfg.ai_mode, "remote", "si parte dal servizio in rete");
        assert!(!cfg.ai_enabled);
        assert_eq!(cfg.port, 6969);
        assert_eq!(cfg.stats_interval_ms, 5000);
    }

    #[test]
    fn un_config_che_ha_acceso_rickyai_resta_acceso() {
        let acceso = r#"{ "aiEnabled": true, "aiPort": 4200 }"#;
        let cfg: AppConfig = serde_json::from_str(acceso).expect("config leggibile");
        assert!(cfg.ai_enabled);
        assert_eq!(cfg.ai_port, 4200);
    }

    #[test]
    fn un_config_completo_sopravvive_al_salvataggio() {
        let mut cfg = AppConfig::default();
        cfg.ai_enabled = true;
        cfg.ai_mode = "remote".into();
        cfg.ai_remote_url = Some("http://192.168.1.50:4141".into());
        cfg.ai_port = 4300;
        cfg.ai_strategy = "fast".into();
        cfg.ai_system_prompt = "sei RickyAI".into();
        cfg.ai_keys.insert("GROQ_API_KEY".into(), "gsk_test".into());

        let json = serde_json::to_string(&cfg).expect("serializzabile");
        let riletto: AppConfig = serde_json::from_str(&json).expect("rileggibile");

        assert!(riletto.ai_enabled);
        assert_eq!(riletto.ai_mode, "remote");
        assert_eq!(riletto.ai_remote_url.as_deref(), Some("http://192.168.1.50:4141"));
        assert_eq!(riletto.ai_port, 4300);
        assert_eq!(riletto.ai_strategy, "fast");
        assert_eq!(riletto.ai_system_prompt, "sei RickyAI");
        assert_eq!(riletto.ai_keys.get("GROQ_API_KEY").map(String::as_str), Some("gsk_test"));
    }

    #[test]
    #[cfg(unix)]
    fn il_file_di_config_e_leggibile_solo_dal_proprietario() {
        use std::os::unix::fs::PermissionsExt;

        let handle = ConfigHandle::in_memory();
        handle.update(|c| {
            c.ai_keys.insert("GROQ_API_KEY".into(), "gsk_segreta".into());
        });

        let mode = std::fs::metadata(&handle.path).expect("config salvata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "permessi troppo larghi: {:o}", mode & 0o777);
        let _ = std::fs::remove_file(&handle.path);
    }
}

pub fn data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME);
    let _ = std::fs::create_dir_all(&dir);
    dir
}
