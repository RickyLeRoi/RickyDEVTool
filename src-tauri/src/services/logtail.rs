use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::events::{now_ms, EventBus};

const POLL_MS: u64 = 500;
const INITIAL_TAIL_BYTES: u64 = 64 * 1024;
const MAX_TAILS: usize = 5;
const MAX_AGE_MS: u64 = 2 * 3600 * 1000;
const MAX_LINE_LEN: usize = 4000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailInfo {
    pub id: String,
    pub path: String,
    pub started_at: u64,
}

struct TailHandle {
    info: TailInfo,
    task: tokio::task::JoinHandle<()>,
}

pub struct TailRegistry {
    bus: EventBus,
    tails: Mutex<HashMap<String, TailHandle>>,
    counter: AtomicU64,
}

impl TailRegistry {
    pub fn new(bus: EventBus) -> Self {
        Self {
            bus,
            tails: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(1),
        }
    }

    pub fn list(&self) -> Vec<TailInfo> {
        let tails = self.tails.lock().expect("tail lock");
        let mut list: Vec<TailInfo> = tails.values().map(|h| h.info.clone()).collect();
        list.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        list
    }

    pub fn start(self: &Arc<Self>, path: &str) -> Result<TailInfo, String> {
        let file_path = PathBuf::from(path)
            .canonicalize()
            .map_err(|e| format!("percorso non valido: {e}"))?;
        if !file_path.is_file() {
            return Err("il percorso non è un file".to_string());
        }

        let mut tails = self.tails.lock().expect("tail lock");
        let now = now_ms();
        tails.retain(|_, h| {
            let keep = now.saturating_sub(h.info.started_at) < MAX_AGE_MS;
            if !keep {
                h.task.abort();
            }
            keep
        });
        let path_str = file_path.to_string_lossy().to_string();
        if let Some(existing) = tails.values().find(|h| h.info.path == path_str) {
            return Ok(existing.info.clone());
        }
        if tails.len() >= MAX_TAILS {
            return Err(format!("massimo {MAX_TAILS} tail contemporanei: ferma quelli che non servono"));
        }

        let id = format!("l{}", self.counter.fetch_add(1, Ordering::Relaxed));
        let info = TailInfo {
            id: id.clone(),
            path: path_str,
            started_at: now,
        };
        let task = tokio::spawn(tail_loop(self.bus.clone(), id.clone(), file_path));
        tails.insert(id, TailHandle { info: info.clone(), task });
        Ok(info)
    }

    pub fn stop(&self, id: &str) -> Result<(), String> {
        let mut tails = self.tails.lock().expect("tail lock");
        let handle = tails.remove(id).ok_or("tail non trovato")?;
        handle.task.abort();
        Ok(())
    }
}

async fn tail_loop(bus: EventBus, id: String, path: PathBuf) {
    let topic = format!("tail:{id}");
    let mut pos: u64 = 0;
    match read_initial(&path, &mut pos) {
        Ok(lines) => {
            for line in lines {
                publish_line(&bus, &topic, &line);
            }
        }
        Err(e) => {
            bus.publish(&topic, serde_json::json!({ "event": "error", "message": e }));
            return;
        }
    }

    let mut carry = String::new();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let len = meta.len();
        if len < pos {
            pos = 0;
            carry.clear();
            bus.publish(&topic, serde_json::json!({ "event": "rotated" }));
        }
        if len == pos {
            continue;
        }
        let chunk = match read_range(&path, pos, len) {
            Ok(c) => c,
            Err(_) => continue,
        };
        pos = len;
        carry.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(newline) = carry.find('\n') {
            let line: String = carry.drain(..=newline).collect();
            publish_line(&bus, &topic, line.trim_end_matches(['\n', '\r']));
        }
        if carry.len() > MAX_LINE_LEN {
            let flushed: String = carry.drain(..).collect();
            publish_line(&bus, &topic, &flushed);
        }
    }
}

fn publish_line(bus: &EventBus, topic: &str, line: &str) {
    let mut line = line;
    if line.len() > MAX_LINE_LEN {
        let mut end = MAX_LINE_LEN;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line = &line[..end];
    }
    bus.publish(
        topic,
        serde_json::json!({ "event": "line", "stream": "out", "line": line }),
    );
}

fn read_initial(path: &PathBuf, pos: &mut u64) -> Result<Vec<String>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let start = len.saturating_sub(INITIAL_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    *pos = len;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    Ok(lines)
}

fn read_range(path: &PathBuf, from: u64, to: u64) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(from)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; (to - from) as usize];
    let mut read = 0;
    while read < buf.len() {
        match std::io::Read::read(&mut file, &mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) => return Err(e.to_string()),
        }
    }
    buf.truncate(read);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn tail_streamma_righe_nuove() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let registry = Arc::new(TailRegistry::new(bus));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        std::fs::write(&path, "vecchia1\nvecchia2\n").unwrap();

        let info = registry.start(&path.to_string_lossy()).expect("start");

        let mut got = Vec::new();
        for _ in 0..2 {
            let ev = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
                .await
                .expect("timeout")
                .expect("bus");
            assert_eq!(ev.topic, format!("tail:{}", info.id));
            got.push(ev.payload.get("line").unwrap().as_str().unwrap().to_string());
        }
        assert_eq!(got, vec!["vecchia1", "vecchia2"]);

        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(f, "nuova").unwrap();
        }
        let ev = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("timeout")
            .expect("bus");
        assert_eq!(ev.payload.get("line").unwrap().as_str().unwrap(), "nuova");

        registry.stop(&info.id).expect("stop");
        assert!(registry.list().is_empty());
    }

    #[tokio::test]
    async fn doppio_start_stesso_file_riusa_il_tail() {
        let bus = EventBus::new();
        let registry = Arc::new(TailRegistry::new(bus));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.log");
        std::fs::write(&path, "").unwrap();
        let a = registry.start(&path.to_string_lossy()).unwrap();
        let b = registry.start(&path.to_string_lossy()).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(registry.list().len(), 1);
    }
}
