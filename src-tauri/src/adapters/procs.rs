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
    /// Normalizzata sul totale dei core (0..100) su entrambi gli OS.
    pub cpu_pct: f32,
    pub mem_bytes: u64,
    pub mem_pct: f32,
    pub started_at: Option<u64>,
    pub is_system: bool,
    pub known_app: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeavyProcessesResult {
    pub processes: Vec<ProcessInfo>,
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

/// Processi sopra soglia CPU **oppure** RAM, ordinati per CPU decrescente.
/// Doppio campionamento: la CPU per processo è un delta, il primo giro da solo darebbe 0.
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

    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .values()
        .filter_map(|p| {
            let cpu_pct = p.cpu_usage() / cores as f32;
            let mem_bytes = p.memory();
            let mem_pct = if total_mem > 0 {
                mem_bytes as f32 / total_mem as f32 * 100.0
            } else {
                0.0
            };
            if cpu_pct < cpu_min_pct && mem_pct < mem_min_pct {
                return None;
            }

            let name = p.name().to_string_lossy().to_string();
            let exe_path = p.exe().map(|path| path.to_string_lossy().to_string());
            let user = p
                .user_id()
                .and_then(|uid| users().get_user_by_id(uid))
                .map(|u| u.name().to_string());
            let uid_num: Option<u32> = p.user_id().and_then(|uid| uid.to_string().parse().ok());

            Some(ProcessInfo {
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
            })
        })
        .collect();

    processes.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));

    HeavyProcessesResult {
        processes,
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

/// Regole per associare icone/etichette alle app note (riusata dalla sezione porte in M2).
pub fn classify_known_app(name: &str, exe_path: Option<&str>) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let path = exe_path.unwrap_or("").to_lowercase();

    const RULES: &[(&str, &[&str])] = &[
        ("node", &["node", "node.exe"]),
        ("dotnet", &["dotnet", "dotnet.exe"]),
        ("docker", &["dockerd", "com.docker.backend", "docker.exe", "docker desktop"]),
        ("ssh", &["sshd", "ssh", "sshd.exe"]),
        ("plex", &["plex media server"]),
        ("samba", &["smbd", "nmbd"]),
        ("iisexpress", &["iisexpress.exe"]),
        ("visualstudio", &["devenv.exe"]),
        ("postgres", &["postgres", "postgres.exe"]),
        ("mysql", &["mysqld", "mysqld.exe"]),
        ("redis", &["redis-server", "redis-server.exe"]),
        ("nginx", &["nginx", "nginx.exe"]),
        ("python", &["python", "python3", "python.exe"]),
        ("java", &["java", "java.exe"]),
    ];
    for (id, names) in RULES {
        if names.iter().any(|n| lower == *n || lower.starts_with(&format!("{n} "))) {
            return Some(id);
        }
    }
    // Casi che richiedono anche il path per non fare falsi positivi.
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

/// Euristica sistema/non-sistema (vedi PROJECT.md §6): imperfetta per design.
#[cfg(target_os = "macos")]
pub(crate) fn is_system_process(
    name: &str,
    exe_path: Option<&str>,
    _user: Option<&str>,
    uid: Option<u32>,
) -> bool {
    const SYSTEM_PATHS: &[&str] = &["/System/", "/usr/libexec/", "/usr/sbin/", "/sbin/"];
    const SYSTEM_NAMES: &[&str] = &[
        "kernel_task", "launchd", "windowserver", "mds", "mds_stores", "mdworker",
        "distnoted", "cfprefsd", "coreaudiod", "logd", "notifyd", "securityd",
    ];
    if matches!(uid, Some(u) if u < 500) {
        return true;
    }
    let in_system_path = exe_path
        .map(|p| SYSTEM_PATHS.iter().any(|prefix| p.starts_with(prefix)))
        .unwrap_or(false);
    in_system_path && SYSTEM_NAMES.contains(&name.to_lowercase().as_str())
}

#[cfg(target_os = "windows")]
pub(crate) fn is_system_process(
    name: &str,
    exe_path: Option<&str>,
    user: Option<&str>,
    _uid: Option<u32>,
) -> bool {
    const SYSTEM_USERS: &[&str] = &["system", "local service", "network service"];
    const SYSTEM_NAMES: &[&str] = &[
        "svchost.exe", "csrss.exe", "wininit.exe", "services.exe", "lsass.exe",
        "smss.exe", "winlogon.exe", "dwm.exe", "registry", "memory compression",
    ];
    if matches!(user, Some(u) if SYSTEM_USERS.contains(&u.to_lowercase().as_str())) {
        return true;
    }
    let in_system_path = exe_path
        .map(|p| p.to_lowercase().starts_with("c:\\windows\\"))
        .unwrap_or(false);
    in_system_path && SYSTEM_NAMES.contains(&name.to_lowercase().as_str())
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
        assert!(!result.processes.is_empty());
        for p in &result.processes {
            assert!(p.cpu_pct >= 0.0 && p.cpu_pct <= 100.0 + f32::EPSILON);
            assert!(p.mem_pct >= 0.0 && p.mem_pct <= 100.0);
        }
    }
}
