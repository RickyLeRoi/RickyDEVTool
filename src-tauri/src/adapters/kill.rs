use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::ports::kill_protection_for;
use super::procs::is_system_process;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillRequest {
    pub pid: u32,
    /// Il kill viene rifiutato se il PID ora appartiene a un altro processo.
    pub expected_name: String,
    pub expected_started_at: Option<u64>,
    #[serde(default)]
    pub force: bool,
    /// Per i processi protetti: deve essere uguale al nome del processo.
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
/// Tolleranza sul confronto di start time (secondi): gli orologi dei due
/// campionamenti possono differire di poco.
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

    // Verifica identità: PID riusato da un altro processo = mai killare.
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
    // Escalation: se dopo il periodo di grazia lo STESSO processo è ancora vivo, SIGKILL.
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
    // taskkill senza /F invia WM_CLOSE (chiusura gentile, solo app con finestra);
    // /T termina anche l'albero dei figli.
    let mut cmd = std::process::Command::new("taskkill");
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
        // Lascia che il processo compaia nella tabella.
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
        assert!(!status.success()); // terminato da segnale
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
}
