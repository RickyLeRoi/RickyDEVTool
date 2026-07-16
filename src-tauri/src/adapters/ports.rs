use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::sync::Mutex;

use super::procs::{classify_known_app, is_system_process};

/// Nomi che richiedono conferma rafforzata prima del kill (bozza Q7 in QUESTIONS.md).
pub const PROTECTED_NAMES: &[&str] = &[
    "sshd",
    "dockerd",
    "com.docker.backend",
    "smbd",
    "plex media server",
    "postgres",
    "mysqld",
    "redis-server",
    "nginx",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortEntry {
    pub port: u16,
    pub protocol: &'static str, // solo "tcp" (LISTEN) in M2
    pub addresses: Vec<String>,
    pub processes: Vec<PortProcess>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortProcess {
    pub pid: u32,
    pub name: String,
    pub exe_path: Option<String>,
    pub user: Option<String>,
    pub started_at: Option<u64>,
    pub is_system: bool,
    pub known_app: Option<&'static str>,
    pub kill_protection: &'static str, // "confirm" | "typed-confirm"
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortScan {
    pub ports: Vec<PortEntry>,
    pub hidden_system: usize,
    pub sampled_at: u64,
}

/// (pid, indirizzo, porta) grezzi dal sistema, prima dell'arricchimento.
#[derive(Debug, PartialEq)]
pub struct RawListener {
    pub pid: u32,
    pub address: String,
    pub port: u16,
}

pub fn kill_protection_for(name: &str) -> &'static str {
    if PROTECTED_NAMES.contains(&name.to_lowercase().as_str()) {
        "typed-confirm"
    } else {
        "confirm"
    }
}

fn proc_table() -> &'static Mutex<System> {
    static TABLE: OnceLock<Mutex<System>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(System::new()))
}

pub async fn scan_tcp_listen(include_system: bool) -> Result<PortScan, String> {
    let raw = list_listeners().await?;

    // Tabella processi per arricchire i PID (nome, exe, utente, classificazione).
    let mut sys = proc_table().lock().await;
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet),
    );
    let users = users();

    let mut hidden_system = 0usize;
    // BTreeMap: porte già ordinate in output.
    let mut by_port: BTreeMap<u16, PortEntry> = BTreeMap::new();
    // Evita duplicati (stesso pid su più bind della stessa porta).
    let mut seen: HashMap<(u16, u32), ()> = HashMap::new();

    for listener in raw {
        let pid = sysinfo::Pid::from_u32(listener.pid);
        let Some(p) = sys.process(pid) else { continue };

        let name = p.name().to_string_lossy().to_string();
        let exe_path = p.exe().map(|path| path.to_string_lossy().to_string());
        let user = p
            .user_id()
            .and_then(|uid| users.get_user_by_id(uid))
            .map(|u| u.name().to_string());
        let uid_num: Option<u32> = p.user_id().and_then(|uid| uid.to_string().parse().ok());
        let is_system = is_system_process(&name, exe_path.as_deref(), user.as_deref(), uid_num);

        if is_system && !include_system {
            hidden_system += 1;
            continue;
        }

        let entry = by_port.entry(listener.port).or_insert_with(|| PortEntry {
            port: listener.port,
            protocol: "tcp",
            addresses: Vec::new(),
            processes: Vec::new(),
        });
        if !entry.addresses.contains(&listener.address) {
            entry.addresses.push(listener.address.clone());
        }
        if seen.insert((listener.port, listener.pid), ()).is_none() {
            entry.processes.push(PortProcess {
                pid: listener.pid,
                known_app: classify_known_app(&name, exe_path.as_deref()),
                kill_protection: kill_protection_for(&name),
                started_at: match p.start_time() {
                    0 => None,
                    secs => Some(secs * 1000),
                },
                is_system,
                name,
                exe_path,
                user,
            });
        }
    }

    Ok(PortScan {
        ports: by_port.into_values().collect(),
        hidden_system,
        sampled_at: crate::events::now_ms(),
    })
}

fn users() -> &'static sysinfo::Users {
    static USERS: OnceLock<sysinfo::Users> = OnceLock::new();
    USERS.get_or_init(sysinfo::Users::new_with_refreshed_list)
}

// ---------- macOS: lsof in formato macchina (-F) ----------

#[cfg(target_os = "macos")]
async fn list_listeners() -> Result<Vec<RawListener>, String> {
    let output = tokio::process::Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-FpnP"])
        .output()
        .await
        .map_err(|e| format!("lsof non eseguibile: {e}"))?;
    // lsof esce 1 anche quando semplicemente non trova nulla: errore solo se stdout è vuoto E stderr parla.
    if output.stdout.is_empty() && !output.status.success() {
        return Err(format!(
            "lsof fallito: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(parse_lsof_f(&String::from_utf8_lossy(&output.stdout)))
}

/// Parser dell'output `lsof -FpnP`: righe prefissate da un carattere campo
/// (p=pid, P=protocollo, n=indirizzo). Testato su fixture.
pub fn parse_lsof_f(output: &str) -> Vec<RawListener> {
    let mut result = Vec::new();
    let mut current_pid: Option<u32> = None;
    let mut current_proto_tcp = false;

    for line in output.lines() {
        let Some(first) = line.chars().next() else { continue };
        let value = &line[1..];
        match first {
            'p' => current_pid = value.parse().ok(),
            'P' => current_proto_tcp = value.eq_ignore_ascii_case("TCP"),
            'n' => {
                if !current_proto_tcp {
                    continue;
                }
                let (Some(pid), Some((address, port))) = (current_pid, split_addr_port(value))
                else {
                    continue;
                };
                result.push(RawListener { pid, address, port });
            }
            _ => {}
        }
    }
    result
}

// ---------- Windows: netstat -ano ----------

#[cfg(target_os = "windows")]
async fn list_listeners() -> Result<Vec<RawListener>, String> {
    let output = tokio::process::Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .await
        .map_err(|e| format!("netstat non eseguibile: {e}"))?;
    if !output.status.success() {
        return Err("netstat fallito".to_string());
    }
    Ok(parse_netstat(&String::from_utf8_lossy(&output.stdout)))
}

/// Parser dell'output `netstat -ano -p TCP` (righe LISTENING). Testato su fixture.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn parse_netstat(output: &str) -> Vec<RawListener> {
    let mut result = Vec::new();
    for line in output.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // TCP <local> <remote> LISTENING <pid>
        if cols.len() != 5 || cols[0] != "TCP" || !cols[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        let (Some((address, port)), Ok(pid)) = (split_addr_port(cols[1]), cols[4].parse())
        else {
            continue;
        };
        result.push(RawListener { pid, address, port });
    }
    result
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn list_listeners() -> Result<Vec<RawListener>, String> {
    // Linux best-effort: stesso formato -F di lsof.
    let output = tokio::process::Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-FpnP"])
        .output()
        .await
        .map_err(|e| format!("lsof non eseguibile: {e}"))?;
    if output.stdout.is_empty() && !output.status.success() {
        return Err("lsof fallito".to_string());
    }
    Ok(parse_lsof_f(&String::from_utf8_lossy(&output.stdout)))
}

/// "\*:3000" | "127.0.0.1:3000" | "[::1]:3000" → (indirizzo, porta)
fn split_addr_port(value: &str) -> Option<(String, u16)> {
    let (addr, port) = value.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    let addr = addr.trim_start_matches('[').trim_end_matches(']');
    let addr = if addr == "*" { "*".to_string() } else { addr.to_string() };
    Some((addr, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lsof_formato_f() {
        let fixture = "p512\ncnode\nPTCP\nn*:3000\nPTCP\nn127.0.0.1:5173\np800\ncControlCe\nPTCP\nn[::1]:7000\n";
        let raw = parse_lsof_f(fixture);
        assert_eq!(
            raw,
            vec![
                RawListener { pid: 512, address: "*".into(), port: 3000 },
                RawListener { pid: 512, address: "127.0.0.1".into(), port: 5173 },
                RawListener { pid: 800, address: "::1".into(), port: 7000 },
            ]
        );
    }

    #[test]
    fn parse_netstat_listening() {
        let fixture = "\nActive Connections\n\n  Proto  Local Address          Foreign Address        State           PID\n  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1084\n  TCP    192.168.1.5:139        0.0.0.0:0              LISTENING       4\n  TCP    127.0.0.1:6969         127.0.0.1:54321        ESTABLISHED     512\n  TCP    [::]:445               [::]:0                 LISTENING       4\n";
        let raw = parse_netstat(fixture);
        assert_eq!(
            raw,
            vec![
                RawListener { pid: 1084, address: "0.0.0.0".into(), port: 135 },
                RawListener { pid: 4, address: "192.168.1.5".into(), port: 139 },
                RawListener { pid: 4, address: "::".into(), port: 445 },
            ]
        );
    }

    #[test]
    fn protezione_kill() {
        assert_eq!(kill_protection_for("sshd"), "typed-confirm");
        assert_eq!(kill_protection_for("Plex Media Server"), "typed-confirm");
        assert_eq!(kill_protection_for("node"), "confirm");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn scan_trova_un_listener_reale() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let scan = scan_tcp_listen(true).await.expect("scan ok");
        let found = scan
            .ports
            .iter()
            .find(|p| p.port == port)
            .unwrap_or_else(|| panic!("porta {port} non trovata nello scan"));
        assert!(found.processes.iter().any(|p| p.pid == std::process::id()));
    }
}
