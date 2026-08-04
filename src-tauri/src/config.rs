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
    /// Override manuali dei path dei tool (id -> path eseguibile/bundle).
    pub tool_paths: std::collections::HashMap<String, String>,
    /// Cartelle progetti pinnate nella sezione Progetti.
    pub pinned_folders: Vec<String>,
    /// Override del package manager per progetto (path -> npm|yarn|pnpm).
    pub node_pm_overrides: std::collections::HashMap<String, String>,
    /// Progetto di avvio .NET per cartella (path cartella -> path csproj).
    pub dotnet_startup: std::collections::HashMap<String, String>,
    /// Profilo launchSettings selezionato per cartella.
    pub dotnet_profile: std::collections::HashMap<String, String>,
    /// Servizi online monitorati (preset + personalizzati).
    pub services: Vec<crate::services::online::ServiceDef>,
    /// Se true, i device LAN abbinati possono eseguire azioni (kill, run, git).
    pub remote_control_enabled: bool,
    /// Se true, dopo 5 min di inattività muove il mouse ogni 3 min (anti-idle).
    pub anti_idle_enabled: bool,
    /// Notifiche push degli alert via ntfy (app ntfy sul telefono, nessun HTTPS richiesto in LAN).
    pub push_enabled: bool,
    /// Server ntfy (default pubblico; sostituibile con un'istanza self-hosted).
    pub push_server: String,
    /// Topic ntfy: generato random al primo avvio, fa da segreto condiviso col telefono.
    pub push_topic: String,
    /// Severità minima da notificare: info | warning | critical.
    pub push_min_severity: String,
    /// Identità stabile di questo desktop per la discovery cross-host di Drop
    /// (generato una volta, sopravvive ai riavvii): funge anche da deviceId
    /// permanente quando altri hub ci inviano file/testo via proxy.
    pub drop_hub_id: String,
    /// Nome mostrato agli altri hub; vuoto = usa l'hostname di sistema.
    pub drop_hub_name: String,
    /// Profili di avvio composito (più task lanciati insieme).
    pub launch_bundles: Vec<crate::services::launch::LaunchBundle>,
    /// Host Docker remoto (es. "ssh://user@host" o "tcp://ip:2375"); vuoto = daemon locale.
    #[serde(default)]
    pub docker_host: Option<String>,
    /// Snippet / comandi salvati eseguibili al volo.
    #[serde(default)]
    pub snippets: Vec<crate::services::snippets::Snippet>,
    /// Host SSH salvati per l'esecuzione rapida di comandi.
    #[serde(default)]
    pub ssh_hosts: Vec<crate::services::ssh::SshHost>,
    /// Soglie configurabili degli alert (CPU/RAM/temperatura/batteria).
    #[serde(default)]
    pub alert_thresholds: AlertThresholds,
    // I campi di RickyAI non portano un `#[serde(default)]` per campo, ed è
    // voluto: quell'attributo scavalca il `default` della struct e ricade sul
    // default del *tipo*. Su `ai_enabled` significherebbe `false` per chiunque
    // abbia già un config.json — cioè RickyAI spenta su ogni installazione
    // esistente, senza che nessuno l'abbia disattivata.
    /// RickyAI: avvia `of-free serve` all'accensione del tool.
    pub ai_enabled: bool,
    /// Porta di `of-free` (con fallback sulle successive se occupata).
    pub ai_port: u16,
    /// Override del percorso del binario `of-free`; vuoto = risolto nel PATH.
    pub ai_command: Option<String>,
    /// File con le chiavi dei provider; vuoto = catena di default di of-free
    /// (`~/.onfeather/.env`, `~/.config/onfeather/.env`).
    pub ai_env_file: Option<String>,
    /// Strategia di routing: balanced | fast | local.
    pub ai_strategy: String,
    /// Prompt di sistema anteposto alle conversazioni di RickyAI.
    pub ai_system_prompt: String,
}

/// Soglie oltre le quali scattano gli alert. Modificabili dall'utente; i default
/// riproducono i valori storici (CPU 90%, RAM 92%) più temperatura e batteria.
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
            ai_port: crate::services::rickyai::DEFAULT_PORT,
            ai_command: None,
            ai_env_file: None,
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
            // Il topic è l'unico segreto di ntfy: random e impronunciabile.
            cfg.push_topic = format!("rickydev-{}", generate_token());
        }
        if cfg.drop_hub_id.is_empty() {
            cfg.drop_hub_id = format!("hub-{}", generate_token());
        }
        // Integra i preset mancanti (nuovi preset in versioni future compaiono da soli;
        // le personalizzazioni enabled/disabled dell'utente restano).
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

    /// Config isolata per i test: default in memoria, salvataggi su file temporaneo.
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

    /// Scrittura atomica: file temporaneo + rename.
    fn save(&self) {
        let cfg = self.get();
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(&cfg).expect("config serializzabile");
        if std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &self.path)).is_err() {
            tracing::error!("impossibile salvare la config in {:?}", self.path);
        }
    }
}

fn generate_token() -> String {
    let mut buf = [0u8; 16];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un config.json scritto da una versione precedente non conosce i campi
    /// nuovi: devono arrivare dal `Default` della struct, non da quello del
    /// tipo. Sembra la stessa cosa e non lo è — un `#[serde(default)]` per
    /// campo scavalca il primo e ricade sul secondo, e i default del tipo sono
    /// tutti valori plausibili (porta 0, strategia ""), quindi la differenza
    /// non si vede finché qualcosa non parte male senza spiegazioni.
    #[test]
    fn i_campi_nuovi_prendono_il_default_della_struct_non_del_tipo() {
        let vecchio = r#"{ "port": 6969, "lanEnabled": true, "statsIntervalMs": 5000 }"#;
        let cfg: AppConfig = serde_json::from_str(vecchio).expect("config leggibile");

        assert_eq!(cfg.ai_port, crate::services::rickyai::DEFAULT_PORT);
        assert_eq!(cfg.ai_strategy, "balanced");
        assert!(!cfg.ai_enabled);
        // I campi già presenti nel file restano quelli del file.
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

    /// Ogni campo della struct sopravvive a un giro di serializzazione: è il
    /// modo generico di accorgersi che un `#[serde(default)]` di troppo sta
    /// riscrivendo un valore invece di conservarlo.
    #[test]
    fn un_config_completo_sopravvive_al_salvataggio() {
        let mut cfg = AppConfig::default();
        cfg.ai_enabled = false;
        cfg.ai_port = 4300;
        cfg.ai_strategy = "fast".into();
        cfg.ai_system_prompt = "sei RickyAI".into();
        cfg.ai_env_file = Some("/tmp/keys.env".into());

        let json = serde_json::to_string(&cfg).expect("serializzabile");
        let riletto: AppConfig = serde_json::from_str(&json).expect("rileggibile");

        assert!(!riletto.ai_enabled);
        assert_eq!(riletto.ai_port, 4300);
        assert_eq!(riletto.ai_strategy, "fast");
        assert_eq!(riletto.ai_system_prompt, "sei RickyAI");
        assert_eq!(riletto.ai_env_file.as_deref(), Some("/tmp/keys.env"));
    }
}

pub fn data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME);
    let _ = std::fs::create_dir_all(&dir);
    dir
}
