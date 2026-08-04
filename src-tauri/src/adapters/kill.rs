use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::ports::kill_protection_for;
use super::procs::is_system_process;
#[cfg(windows)]
use crate::exec;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillRequest {
    pub pid: u32,
    pub expected_name: String,
    pub expected_started_at: Option<u64>,
    #[serde(default)]
    pub force: bool,
    pub confirm_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KillOutcome {
    pub killed: bool,
    pub forced: bool,
}

#[derive(Debug)]
pub enum KillError {
    ProcessGone,
    SystemProtected,
    TypedConfirmRequired { name: String },
    Failed { message: String, os_hint: Option<String> },
}

const GRACE_SECS: u64 = 5;
const START_TIME_TOLERANCE_S: i64 = 2;

pub async fn kill_process(req: KillRequest) -> Result<KillOutcome, KillError> {
    let pid = Pid::from_u32(req.pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet),
    );

    let Some(p) = sys.process(pid) else {
        return Err(KillError::ProcessGone);
    };
    let name = p.name().to_string_lossy().to_string();

    if !name.eq_ignore_ascii_case(&req.expected_name) {
        return Err(KillError::ProcessGone);
    }
    if let Some(expected_ms) = req.expected_started_at {
        let actual_s = p.start_time() as i64;
        let expected_s = (expected_ms / 1000) as i64;
        if (actual_s - expected_s).abs() > START_TIME_TOLERANCE_S {
            return Err(KillError::ProcessGone);
        }
    }

    let exe_path = p.exe().map(|path| path.to_string_lossy().to_string());
    let uid_num: Option<u32> = p.user_id().and_then(|uid| uid.to_string().parse().ok());
    if is_system_process(&name, exe_path.as_deref(), None, uid_num) {
        return Err(KillError::SystemProtected);
    }

    if kill_protection_for(&name) == "typed-confirm"
        && req.confirm_name.as_deref().map(|c| c.eq_ignore_ascii_case(&name)) != Some(true)
    {
        return Err(KillError::TypedConfirmRequired { name });
    }

    let start_time = p.start_time();
    tracing::info!(pid = req.pid, %name, force = req.force, "kill richiesto");

    if req.force {
        kill_now(req.pid, true)?;
        return Ok(KillOutcome { killed: true, forced: true });
    }

    kill_now(req.pid, false)?;
    tokio::spawn(escalate_if_alive(req.pid, name, start_time));
    Ok(KillOutcome { killed: true, forced: false })
}

async fn escalate_if_alive(pid_raw: u32, name: String, start_time: u64) {
    tokio::time::sleep(std::time::Duration::from_secs(GRACE_SECS)).await;
    let pid = Pid::from_u32(pid_raw);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    if let Some(p) = sys.process(pid) {
        if p.name().to_string_lossy().eq_ignore_ascii_case(&name) && p.start_time() == start_time {
            tracing::warn!(pid = pid_raw, %name, "ancora vivo dopo {GRACE_SECS}s, forzo il kill");
            let _ = kill_now(pid_raw, true);
        }
    }
}

#[cfg(unix)]
fn kill_now(pid: u32, force: bool) -> Result<(), KillError> {
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    let rc = unsafe { libc::kill(pid as i32, signal) };
    if rc == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => Err(KillError::Failed {
            message: "Permessi insufficienti per terminare il processo".into(),
            os_hint: Some(
                "Il processo appartiene a un altro utente o è elevato: serve terminarlo da un terminale con sudo".into(),
            ),
        }),
        Some(libc::ESRCH) => Err(KillError::ProcessGone),
        _ => Err(KillError::Failed {
            message: std::io::Error::last_os_error().to_string(),
            os_hint: None,
        }),
    }
}

#[cfg(windows)]
fn kill_now(pid: u32, force: bool) -> Result<(), KillError> {
    let mut cmd = exec::sync_cmd("taskkill");
    if force {
        cmd.args(["/F", "/T"]);
    }
    cmd.args(["/PID", &pid.to_string()]);
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
            if stderr.contains("access is denied") || stderr.contains("accesso negato") {
                Err(KillError::Failed {
                    message: "Permessi insufficienti per terminare il processo".into(),
                    os_hint: Some("Il processo è elevato: riavvia RickyDEVTool come amministratore".into()),
                })
            } else {
                Err(KillError::Failed { message: stderr.trim().to_string(), os_hint: None })
            }
        }
        Err(e) => Err(KillError::Failed { message: e.to_string(), os_hint: None }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    async fn spawn_sleeper() -> (u32, tokio::process::Child) {
        let child = tokio::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id().expect("pid");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        (pid, child)
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn kill_di_un_processo_figlio() {
        let (pid, mut child) = spawn_sleeper().await;
        let outcome = kill_process(KillRequest {
            pid,
            expected_name: "sleep".into(),
            expected_started_at: None,
            force: false,
            confirm_name: None,
        })
        .await
        .expect("kill ok");
        assert!(outcome.killed);
        let status = child.wait().await.expect("wait");
        assert!(!status.success());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn kill_rifiutato_se_nome_non_corrisponde() {
        let (pid, mut child) = spawn_sleeper().await;
        let result = kill_process(KillRequest {
            pid,
            expected_name: "nginx".into(),
            expected_started_at: None,
            force: false,
            confirm_name: None,
        })
        .await;
        assert!(matches!(result, Err(KillError::ProcessGone)));
        child.kill().await.expect("cleanup");
    }

    #[tokio::test]
    async fn kill_rifiutato_su_pid_inesistente() {
        let result = kill_process(KillRequest {
            pid: u32::MAX - 7,
            expected_name: "qualcosa".into(),
            expected_started_at: None,
            force: false,
            confirm_name: None,
        })
        .await;
        assert!(matches!(result, Err(KillError::ProcessGone)));
    }

    #[allow(dead_code)]
    async fn spawn_long_child() -> (u32, tokio::process::Child, String) {
        #[cfg(unix)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sleep");
            c.arg("30");
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("ping");
            c.args(["-n", "30", "127.0.0.1"]);
            c
        };
        let child = cmd.spawn().expect("spawn del processo figlio");
        let pid = child.id().expect("pid");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let name = real_process_name(pid).expect("nome del processo figlio");
        (pid, child, name)
    }

    #[allow(dead_code)]
    fn real_process_name(pid: u32) -> Option<String> {
        let p = Pid::from_u32(pid);
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[p]),
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.process(p).map(|pr| pr.name().to_string_lossy().to_string())
    }

    #[tokio::test]
    #[ignore = "contract test per-OS: spawna e termina un processo reale (--ignored)"]
    async fn contract_force_kill_child_reale() {
        let (pid, mut child, name) = spawn_long_child().await;
        let outcome = kill_process(KillRequest {
            pid,
            expected_name: name,
            expected_started_at: None,
            force: true,
            confirm_name: None,
        })
        .await
        .expect("kill ok");
        assert!(outcome.killed && outcome.forced);
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .expect("il figlio dovrebbe essere terminato entro 5s")
            .expect("wait");
        assert!(!status.success(), "processo terminato da segnale/kill");
    }

    #[tokio::test]
    #[ignore = "contract test per-OS: rifiuto su identità PID non coerente (--ignored)"]
    async fn contract_kill_rifiuta_nome_diverso_reale() {
        let (pid, mut child, _name) = spawn_long_child().await;
        let result = kill_process(KillRequest {
            pid,
            expected_name: "processo-che-non-esiste".into(),
            expected_started_at: None,
            force: true,
            confirm_name: None,
        })
        .await;
        assert!(matches!(result, Err(KillError::ProcessGone)));
        child.kill().await.expect("cleanup del figlio");
    }
}
