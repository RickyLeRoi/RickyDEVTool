use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::events::{now_ms, EventBus};
use crate::exec;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub id: String,
    pub label: String,
    pub cwd: String,
    pub state: TaskState,
    pub exit_code: Option<i32>,
    pub started_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Running,
    Exited,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub stream: &'static str,
    pub line: String,
}

use crate::constants::{MAX_TASK_LOG_LINES, TASK_REAP_DELAY};

struct TaskHandle {
    info: TaskInfo,
    log: Arc<Mutex<Vec<LogLine>>>,
    #[cfg(unix)]
    pgid: i32,
    #[cfg(not(unix))]
    pid: u32,
}

pub struct TaskRegistry {
    bus: EventBus,
    tasks: Mutex<HashMap<String, TaskHandle>>,
    counter: AtomicU64,
}

impl TaskRegistry {
    pub fn new(bus: EventBus) -> Self {
        Self {
            bus,
            tasks: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(1),
        }
    }

    pub fn list(&self) -> Vec<TaskInfo> {
        let tasks = self.tasks.lock().expect("task lock");
        let mut list: Vec<TaskInfo> = tasks.values().map(|h| h.info.clone()).collect();
        list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        list
    }

    pub fn is_running(&self, id: &str) -> bool {
        let tasks = self.tasks.lock().expect("task lock");
        tasks.get(id).is_some_and(|h| h.info.state == TaskState::Running)
    }

    pub fn log(&self, id: &str) -> Option<Vec<LogLine>> {
        let buffer = {
            let tasks = self.tasks.lock().expect("task lock");
            tasks.get(id).map(|h| h.log.clone())
        }?;
        let lines = buffer.lock().expect("log lock").clone();
        Some(lines)
    }

    pub fn spawn(
        self: &Arc<Self>,
        label: &str,
        program: &str,
        args: &[&str],
        cwd: &str,
    ) -> Result<TaskInfo, String> {
        #[cfg(windows)]
        let cmd = {
            let mut c = exec::cmd("cmd");
            c.arg("/C").arg(program).args(args);
            c
        };
        #[cfg(not(windows))]
        let cmd = {
            let mut c = exec::cmd(program);
            c.args(args);
            c
        };
        self.launch(label, cwd, cmd)
    }

    pub fn spawn_shell(
        self: &Arc<Self>,
        label: &str,
        command: &str,
        cwd: &str,
    ) -> Result<TaskInfo, String> {
        if command.trim().is_empty() {
            return Err("comando vuoto".to_string());
        }
        #[cfg(windows)]
        let cmd = {
            let mut c = exec::cmd("cmd");
            c.arg("/C").arg(command);
            c
        };
        #[cfg(not(windows))]
        let cmd = {
            let mut c = exec::cmd("sh");
            c.arg("-c").arg(command);
            c
        };
        self.launch(label, cwd, cmd)
    }

    fn launch(
        self: &Arc<Self>,
        label: &str,
        cwd: &str,
        mut cmd: tokio::process::Command,
    ) -> Result<TaskInfo, String> {
        let id = format!("t{}", self.counter.fetch_add(1, Ordering::Relaxed));

        cmd.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn().map_err(|e| format!("avvio fallito: {e}"))?;
        let pid = child.id().ok_or("pid non disponibile")?;

        let info = TaskInfo {
            id: id.clone(),
            label: label.to_string(),
            cwd: cwd.to_string(),
            state: TaskState::Running,
            exit_code: None,
            started_at: now_ms(),
        };
        let log = Arc::new(Mutex::new(Vec::<LogLine>::new()));
        {
            let mut tasks = self.tasks.lock().expect("task lock");
            tasks.insert(
                id.clone(),
                TaskHandle {
                    info: info.clone(),
                    log: log.clone(),
                    #[cfg(unix)]
                    pgid: pid as i32,
                    #[cfg(not(unix))]
                    pid,
                },
            );
        }
        self.publish_state(&info);

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let Some(out) = stdout {
            tokio::spawn(stream_lines(self.bus.clone(), log.clone(), id.clone(), out, "out"));
        }
        if let Some(err) = stderr {
            tokio::spawn(stream_lines(self.bus.clone(), log.clone(), id.clone(), err, "err"));
        }

        let registry = Arc::clone(self);
        let task_id = id.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            let code = status.as_ref().ok().and_then(|s| s.code());
            let ok = status.map(|s| s.success()).unwrap_or(false);
            let info = {
                let mut tasks = registry.tasks.lock().expect("task lock");
                if let Some(handle) = tasks.get_mut(&task_id) {
                    handle.info.state = if ok { TaskState::Exited } else { TaskState::Failed };
                    handle.info.exit_code = code;
                    Some(handle.info.clone())
                } else {
                    None
                }
            };
            if let Some(info) = info {
                registry.bus.publish(
                    &format!("task:{task_id}"),
                    serde_json::json!({ "event": "exit", "exitCode": code, "ok": ok }),
                );
                registry.publish_state(&info);
                tracing::info!(task = %task_id, label = %info.label, ?code, "task terminato");
            }
        });

        Ok(info)
    }

    pub fn stop(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let tasks = self.tasks.lock().expect("task lock");
        let handle = tasks.get(id).ok_or("task non trovato")?;
        if handle.info.state != TaskState::Running {
            return Err("il task non è in esecuzione".to_string());
        }
        #[cfg(unix)]
        {
            let pgid = handle.pgid;
            unsafe { libc::killpg(pgid, libc::SIGTERM) };
            // 20260806 ++ RG #Security il pgid è il pid del figlio: se il SIGTERM basta e il SO riassegna quel
            // numero entro i 3s, un SIGKILL alla cieca ammazzerebbe processi estranei. Si forza solo se vivo.
            let registry = Arc::clone(self);
            let task_id = id.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(TASK_REAP_DELAY).await;
                if registry.is_running(&task_id) {
                    tracing::warn!(task = %task_id, "SIGTERM ignorato, forzo la chiusura");
                    unsafe { libc::killpg(pgid, libc::SIGKILL) };
                }
            });
        }
        #[cfg(not(unix))]
        {
            let _ = exec::sync_cmd("taskkill")
                .args(["/T", "/F", "/PID", &handle.pid.to_string()])
                .output();
        }
        Ok(())
    }

    pub fn clear_finished(&self) {
        {
            let mut tasks = self.tasks.lock().expect("task lock");
            tasks.retain(|_, h| h.info.state == TaskState::Running);
        }
        self.bus.publish("tasks", serde_json::json!({ "tasks": self.list() }));
    }

    fn publish_state(&self, _info: &TaskInfo) {
        self.bus.publish(
            "tasks",
            serde_json::json!({ "tasks": self.list() }),
        );
    }
}

async fn stream_lines<R: tokio::io::AsyncRead + Unpin>(
    bus: EventBus,
    log: Arc<Mutex<Vec<LogLine>>>,
    task_id: String,
    reader: R,
    stream: &'static str,
) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        {
            let mut buf = log.lock().expect("log lock");
            buf.push(LogLine { stream, line: line.clone() });
            if buf.len() > MAX_TASK_LOG_LINES {
                let excess = buf.len() - MAX_TASK_LOG_LINES;
                buf.drain(..excess);
            }
        }
        bus.publish(
            &format!("task:{task_id}"),
            serde_json::json!({ "event": "line", "stream": stream, "line": line }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(unix)]
    async fn task_esegue_e_streamma_output() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let registry = Arc::new(TaskRegistry::new(bus));
        let dir = tempfile::tempdir().unwrap();

        let info = registry
            .spawn("echo test", "sh", &["-c", "echo riga1; echo riga2"], dir.path().to_str().unwrap())
            .expect("spawn");
        assert_eq!(info.state, TaskState::Running);

        let mut lines = Vec::new();
        let mut exited = false;
        while !exited {
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .expect("timeout eventi")
                .expect("bus chiuso");
            if event.topic == format!("task:{}", info.id) {
                match event.payload.get("event").and_then(|v| v.as_str()) {
                    Some("line") => lines.push(
                        event.payload.get("line").unwrap().as_str().unwrap().to_string(),
                    ),
                    Some("exit") => exited = true,
                    _ => {}
                }
            }
        }
        assert_eq!(lines, vec!["riga1", "riga2"]);
        let final_state = registry.list().into_iter().find(|t| t.id == info.id).unwrap();
        assert_eq!(final_state.state, TaskState::Exited);
        assert_eq!(final_state.exit_code, Some(0));

        let log = registry.log(&info.id).expect("log presente");
        let buffered: Vec<&str> = log.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(buffered, vec!["riga1", "riga2"]);
        assert!(log.iter().all(|l| l.stream == "out"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn spawn_shell_usa_la_shell() {
        let bus = EventBus::new();
        let registry = Arc::new(TaskRegistry::new(bus));
        let dir = tempfile::tempdir().unwrap();
        let info = registry
            .spawn_shell("compound", "echo uno && echo $((1+1))", dir.path().to_str().unwrap())
            .expect("spawn_shell");
        for _ in 0..50 {
            if registry.list().iter().find(|t| t.id == info.id).unwrap().state
                != TaskState::Running
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let log = registry.log(&info.id).expect("log");
        let lines: Vec<&str> = log.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(lines, vec!["uno", "2"]);
        assert!(registry.spawn_shell("vuoto", "   ", dir.path().to_str().unwrap()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn stop_termina_il_process_group() {
        let bus = EventBus::new();
        let registry = Arc::new(TaskRegistry::new(bus));
        let dir = tempfile::tempdir().unwrap();

        let info = registry
            .spawn("sleep lungo", "sh", &["-c", "sleep 60"], dir.path().to_str().unwrap())
            .expect("spawn");
        registry.stop(&info.id).expect("stop");
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let state = registry.list().into_iter().find(|t| t.id == info.id).unwrap();
        assert_eq!(state.state, TaskState::Failed);
    }

    #[test]
    #[cfg(unix)]
    fn il_sigkill_resta_condizionato_allo_stato_del_task() {
        let sorgente = std::fs::read_to_string("src/tasks.rs").expect("sorgente dei task");
        let inizio = sorgente.find("pub fn stop(").expect("fn stop");
        let fine = sorgente[inizio..].find("\n    pub fn clear_finished").expect("fine di stop") + inizio;
        let corpo = &sorgente[inizio..fine];

        let kill = corpo.find("libc::SIGKILL").expect("l'escalation a SIGKILL");
        let guardia = corpo.find("is_running(&task_id)").expect(
            "il SIGKILL differito deve essere protetto da is_running: senza, dopo 3s colpisce \
             un pgid che il SO può aver riassegnato a processi estranei",
        );
        assert!(guardia < kill, "il controllo deve venire prima del SIGKILL, non dopo");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn stop_non_forza_un_task_gia_uscito() {
        let registry = Arc::new(TaskRegistry::new(EventBus::new()));
        let dir = tempfile::tempdir().unwrap();
        let info = registry
            .spawn("sleep lungo", "sh", &["-c", "sleep 60"], dir.path().to_str().unwrap())
            .expect("spawn");

        assert!(registry.is_running(&info.id));
        registry.stop(&info.id).expect("stop");
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;

        assert!(
            !registry.is_running(&info.id),
            "il SIGTERM è bastato, quindi a 3s l'escalation non deve trovare nulla da forzare"
        );
        assert!(registry.stop(&info.id).is_err(), "un task finito non si ferma due volte");
    }

    // 20260806 ++ RG #Security lento per forza: aspetta i 3s dell'escalation, perciò sta fra gli --ignored.
    #[tokio::test]
    #[cfg(unix)]
    #[ignore]
    async fn stop_forza_chi_ignora_il_sigterm() {
        let registry = Arc::new(TaskRegistry::new(EventBus::new()));
        let dir = tempfile::tempdir().unwrap();
        let info = registry
            .spawn(
                "sordo al TERM",
                "sh",
                // 20260806 ++ RG #Security il trap vale solo per la shell: killpg colpisce il gruppo, quindi un
                // `sleep 60` figlio la farebbe uscire. Il ciclo la tiene viva.
                &["-c", "trap '' TERM; while :; do sleep 0.2; done"],
                dir.path().to_str().unwrap(),
            )
            .expect("spawn");
        // 20260806 ++ RG #Security senza questa attesa il SIGTERM arriva prima che sh installi il trap.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;

        registry.stop(&info.id).expect("stop");
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        assert!(registry.is_running(&info.id), "ha ignorato il SIGTERM, è ancora vivo");

        tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
        assert!(
            !registry.is_running(&info.id),
            "passati i 3s l'escalation deve comunque forzarlo: il controllo non la disattiva"
        );
    }
}
