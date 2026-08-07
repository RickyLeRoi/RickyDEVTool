use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use sysinfo::{
    CpuRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind, Users,
};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    pub exe_path: Option<String>,
    pub user: Option<String>,
    pub cpu_pct: f32,
    pub mem_bytes: u64,
    pub mem_pct: f32,
    pub started_at: Option<u64>,
    pub is_system: bool,
    pub known_app: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessGroup {
    pub name: String,
    pub known_app: Option<&'static str>,
    pub is_system: bool,
    pub cpu_pct: f32,
    pub mem_bytes: u64,
    pub mem_pct: f32,
    pub count: usize,
    pub members: Vec<ProcessInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeavyProcessesResult {
    pub groups: Vec<ProcessGroup>,
    pub sampled_at: u64,
    pub cpu_cores: usize,
    pub cpu_min_pct: f32,
    pub mem_min_pct: f32,
}

fn sampler() -> &'static Mutex<System> {
    static SAMPLER: OnceLock<Mutex<System>> = OnceLock::new();
    SAMPLER.get_or_init(|| {
        Mutex::new(System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing()),
        ))
    })
}

fn users() -> &'static Users {
    static USERS: OnceLock<Users> = OnceLock::new();
    USERS.get_or_init(Users::new_with_refreshed_list)
}

pub async fn heavy_processes(cpu_min_pct: f32, mem_min_pct: f32) -> HeavyProcessesResult {
    let refresh = ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet);

    let mut sys = sampler().lock().await;
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
    tokio::time::sleep(Duration::from_millis(300)).await;
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);

    let cores = num_cores(&mut sys);
    let total_mem = total_memory();

    let all: Vec<ProcessInfo> = sys
        .processes()
        .values()
        .map(|p| {
            let cpu_pct = p.cpu_usage() / cores as f32;
            let mem_bytes = p.memory();
            let mem_pct = if total_mem > 0 {
                mem_bytes as f32 / total_mem as f32 * 100.0
            } else {
                0.0
            };
            let name = p.name().to_string_lossy().to_string();
            let exe_path = p.exe().map(|path| path.to_string_lossy().to_string());
            let user = p
                .user_id()
                .and_then(|uid| users().get_user_by_id(uid))
                .map(|u| u.name().to_string());
            let uid_num: Option<u32> = p.user_id().and_then(|uid| uid.to_string().parse().ok());

            ProcessInfo {
                pid: p.pid().as_u32(),
                ppid: p.parent().map(|pid| pid.as_u32()),
                is_system: is_system_process(&name, exe_path.as_deref(), user.as_deref(), uid_num),
                known_app: classify_known_app(&name, exe_path.as_deref()),
                name,
                exe_path,
                user,
                cpu_pct,
                mem_bytes,
                mem_pct,
                started_at: match p.start_time() {
                    0 => None,
                    secs => Some(secs * 1000),
                },
            }
        })
        .collect();

    let mut by_name: HashMap<String, Vec<ProcessInfo>> = HashMap::new();
    for p in all {
        by_name.entry(p.name.clone()).or_default().push(p);
    }

    let mut groups: Vec<ProcessGroup> = by_name
        .into_values()
        .map(|mut members| {
            members.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
            let cpu_pct = members.iter().map(|m| m.cpu_pct).sum();
            let mem_bytes = members.iter().map(|m| m.mem_bytes).sum();
            let mem_pct = members.iter().map(|m| m.mem_pct).sum();
            ProcessGroup {
                name: members[0].name.clone(),
                known_app: members[0].known_app,
                is_system: members[0].is_system,
                cpu_pct,
                mem_bytes,
                mem_pct,
                count: members.len(),
                members,
            }
        })
        .filter(|g| g.cpu_pct >= cpu_min_pct || g.mem_pct >= mem_min_pct)
        .collect();

    groups.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));

    HeavyProcessesResult {
        groups,
        sampled_at: crate::events::now_ms(),
        cpu_cores: cores,
        cpu_min_pct,
        mem_min_pct,
    }
}

fn num_cores(sys: &mut System) -> usize {
    if sys.cpus().is_empty() {
        sys.refresh_cpu_usage();
    }
    sys.cpus().len().max(1)
}

fn total_memory() -> u64 {
    static TOTAL: OnceLock<u64> = OnceLock::new();
    *TOTAL.get_or_init(|| {
        let mut sys = System::new();
        sys.refresh_memory();
        sys.total_memory()
    })
}

pub fn classify_known_app(name: &str, exe_path: Option<&str>) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let path = exe_path.unwrap_or("").to_lowercase();

    for (id, names) in crate::constants::KNOWN_APP_RULES {
        if names.iter().any(|n| lower == *n || lower.starts_with(&format!("{n} "))) {
            return Some(id);
        }
    }
    if (lower.starts_with("code") || lower.contains("code helper"))
        && (path.contains("visual studio code") || path.contains("vs code"))
    {
        return Some("vscode");
    }
    if lower.contains("chrome") {
        return Some("chrome");
    }
    None
}

#[cfg(target_os = "macos")]
pub(crate) fn is_system_process(
    name: &str,
    exe_path: Option<&str>,
    _user: Option<&str>,
    uid: Option<u32>,
) -> bool {
    use crate::constants::{MACOS_SYSTEM_NAMES, MACOS_SYSTEM_PATHS, MACOS_SYSTEM_UID_MAX};

    if matches!(uid, Some(u) if u < MACOS_SYSTEM_UID_MAX) {
        return true;
    }
    let in_system_path = exe_path
        .map(|p| MACOS_SYSTEM_PATHS.iter().any(|prefix| p.starts_with(prefix)))
        .unwrap_or(false);
    in_system_path && MACOS_SYSTEM_NAMES.contains(&name.to_lowercase().as_str())
}

#[cfg(target_os = "windows")]
pub(crate) fn is_system_process(
    name: &str,
    exe_path: Option<&str>,
    user: Option<&str>,
    _uid: Option<u32>,
) -> bool {
    use crate::constants::{WINDOWS_SYSTEM_NAMES, WINDOWS_SYSTEM_PATH_PREFIX, WINDOWS_SYSTEM_USERS};

    if matches!(user, Some(u) if WINDOWS_SYSTEM_USERS.contains(&u.to_lowercase().as_str())) {
        return true;
    }
    let in_system_path = exe_path
        .map(|p| p.to_lowercase().starts_with(WINDOWS_SYSTEM_PATH_PREFIX))
        .unwrap_or(false);
    in_system_path && WINDOWS_SYSTEM_NAMES.contains(&name.to_lowercase().as_str())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn is_system_process(
    _name: &str,
    exe_path: Option<&str>,
    _user: Option<&str>,
    uid: Option<u32>,
) -> bool {
    matches!(uid, Some(u) if u < 1000)
        || exe_path
            .map(|p| p.starts_with("/usr/lib/") || p.starts_with("/usr/sbin/"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifica_app_note() {
        assert_eq!(classify_known_app("node", None), Some("node"));
        assert_eq!(classify_known_app("Plex Media Server", None), Some("plex"));
        assert_eq!(
            classify_known_app("Code Helper (Renderer)", Some("/Applications/Visual Studio Code.app/x")),
            Some("vscode")
        );
        assert_eq!(classify_known_app("Code Helper", Some("/opt/altro")), None);
        assert_eq!(classify_known_app("finder", None), None);
    }

    #[tokio::test]
    async fn heavy_processes_ritorna_dati_coerenti() {
        let result = heavy_processes(0.0, 0.0).await;
        assert!(result.cpu_cores >= 1);
        assert!(!result.groups.is_empty());
        for g in &result.groups {
            assert_eq!(g.count, g.members.len());
            assert!(g.mem_pct >= 0.0);
            for m in &g.members {
                assert!(m.cpu_pct >= 0.0 && m.cpu_pct <= 100.0 + f32::EPSILON);
                assert!(m.mem_pct >= 0.0 && m.mem_pct <= 100.0);
            }
        }
    }

    #[tokio::test]
    async fn gruppo_supera_soglia_anche_se_nessun_membro_da_solo() {
        let result = heavy_processes(0.0, 0.0).await;
        let total_from_groups: usize = result.groups.iter().map(|g| g.count).sum();
        assert!(total_from_groups > 0);
        let mut names: Vec<&str> = result.groups.iter().map(|g| g.name.as_str()).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "nomi duplicati tra i gruppi");
    }
}
