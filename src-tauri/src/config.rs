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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            lan_enabled: true,
            pair_token: String::new(),
            stats_interval_ms: 1000,
            tool_paths: std::collections::HashMap::new(),
            pinned_folders: Vec::new(),
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
        let handle = Self {
            inner: Arc::new(RwLock::new(cfg)),
            path,
        };
        handle.save();
        handle
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

pub fn data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME);
    let _ = std::fs::create_dir_all(&dir);
    dir
}
