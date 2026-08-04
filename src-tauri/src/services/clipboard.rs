use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::adapters::clipboard::ClipRead;
use crate::services::clipboard_cache::{BlobCache, BlobId};

const POLL_INTERVAL: Duration = Duration::from_millis(1500);
const MAX_ENTRIES: usize = 100;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    Text,
    Image,
    Files,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipFile {
    pub name: String,
    pub size: u64,
    pub has_blob: bool,
    #[serde(skip)]
    pub blob: Option<BlobId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipImage {
    pub mime: String,
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub blob: BlobId,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipEntry {
    pub id: u64,
    pub kind: ClipKind,
    pub text: String,
    pub bytes: u64,
    pub copied_at: u64,
    pub pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<ClipFile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ClipImage>,
    #[serde(skip)]
    pub sig: String,
}

struct State {
    entries: VecDeque<ClipEntry>,
    last_seen: Option<String>,
}

pub struct BlobServe {
    pub path: PathBuf,
    pub name: String,
    pub mime: String,
    pub inline: bool,
}

pub struct ClipboardHistory {
    state: Mutex<State>,
    enabled: AtomicBool,
    next_id: AtomicU64,
    last_change: AtomicI64,
    cache: BlobCache,
}

impl ClipboardHistory {
    fn new() -> Self {
        ClipboardHistory {
            state: Mutex::new(State {
                entries: VecDeque::new(),
                last_seen: None,
            }),
            enabled: AtomicBool::new(true),
            next_id: AtomicU64::new(1),
            last_change: AtomicI64::new(0),
            cache: BlobCache::new(),
        }
    }

    pub fn start() -> Arc<Self> {
        crate::services::clipboard_cache::purge_root();
        let history = Arc::new(Self::new());
        if crate::adapters::clipboard::supported() {
            let weak = Arc::downgrade(&history);
            std::thread::Builder::new()
                .name("clipboard-sampler".to_string())
                .spawn(move || loop {
                    let Some(history) = weak.upgrade() else { break };
                    if history.enabled.load(Ordering::Relaxed) {
                        let last = history.last_change.load(Ordering::Relaxed);
                        let (change, read) = crate::adapters::clipboard::read(last);
                        history.last_change.store(change, Ordering::Relaxed);
                        match read {
                            Some(ClipRead::Text(t)) => history.record(t),
                            Some(ClipRead::Files(paths)) => history.record_files(paths),
                            Some(ClipRead::Image { png_path, mime, width, height }) => {
                                history.record_image(png_path, mime, width, height)
                            }
                            None => {}
                        }
                    }
                    drop(history);
                    std::thread::sleep(POLL_INTERVAL);
                })
                .ok();
        }
        history
    }

    pub fn record(&self, text: String) {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
            return;
        }
        if self.is_duplicate_or_promote(&text) {
            return;
        }
        let entry = self.build_entry(ClipKind::Text, text.clone(), text.len() as u64, None, None, text);
        self.push(entry);
    }

    pub fn record_files(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let sig = files_sig(&paths);
        if self.is_duplicate_or_promote(&sig) {
            return;
        }
        let mut files = Vec::with_capacity(paths.len());
        let mut total: u64 = 0;
        for p in &paths {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
                .to_string();
            let meta = std::fs::metadata(p).ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let is_file = meta.as_ref().map(|m| m.is_file()).unwrap_or(false);
            total += size;
            let blob = if is_file && size <= MAX_FILE_BYTES {
                self.cache.store_copy(p, &name).ok()
            } else {
                None
            };
            files.push(ClipFile { name, size, has_blob: blob.is_some(), blob });
        }
        let label = if files.len() == 1 {
            files[0].name.clone()
        } else {
            format!("{} file", files.len())
        };
        let entry = self.build_entry(ClipKind::Files, label, total, Some(files), None, sig);
        self.push(entry);
    }

    pub fn record_image(&self, png_path: PathBuf, mime: String, width: u32, height: u32) {
        let size = std::fs::metadata(&png_path).map(|m| m.len()).unwrap_or(0);
        let sig = image_sig(width, height, size);
        if self.is_duplicate_or_promote(&sig) {
            let _ = std::fs::remove_file(&png_path);
            return;
        }
        let blob = match self.cache.store_move(&png_path, "immagine.png") {
            Ok(id) => id,
            Err(_) => {
                let _ = std::fs::remove_file(&png_path);
                return;
            }
        };
        let label = format!("Immagine {width}×{height}");
        let image = ClipImage { mime, width, height, blob };
        let entry = self.build_entry(ClipKind::Image, label, size, None, Some(image), sig);
        self.push(entry);
    }

    fn build_entry(
        &self,
        kind: ClipKind,
        text: String,
        bytes: u64,
        files: Option<Vec<ClipFile>>,
        image: Option<ClipImage>,
        sig: String,
    ) -> ClipEntry {
        ClipEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            kind,
            text,
            bytes,
            copied_at: now_ms(),
            pinned: false,
            files,
            image,
            sig,
        }
    }

    fn is_duplicate_or_promote(&self, sig: &str) -> bool {
        let mut st = self.state.lock().unwrap();
        if st.last_seen.as_deref() == Some(sig) {
            return true;
        }
        if let Some(pos) = st.entries.iter().position(|e| e.sig == sig) {
            let mut e = st.entries.remove(pos).unwrap();
            e.copied_at = now_ms();
            st.entries.push_front(e);
            st.last_seen = Some(sig.to_string());
            return true;
        }
        false
    }

    fn push(&self, entry: ClipEntry) {
        let evicted = {
            let mut st = self.state.lock().unwrap();
            st.last_seen = Some(entry.sig.clone());
            st.entries.push_front(entry);
            Self::evict(&mut st)
        };
        for e in &evicted {
            self.free_blobs(e);
        }
    }

    fn evict(st: &mut State) -> Vec<ClipEntry> {
        let mut evicted = Vec::new();
        while st.entries.iter().filter(|e| !e.pinned).count() > MAX_ENTRIES {
            if let Some(pos) = st.entries.iter().rposition(|e| !e.pinned) {
                evicted.push(st.entries.remove(pos).unwrap());
            } else {
                break;
            }
        }
        evicted
    }

    fn free_blobs(&self, e: &ClipEntry) {
        if let Some(img) = &e.image {
            self.cache.remove(img.blob);
        }
        if let Some(files) = &e.files {
            for f in files {
                if let Some(b) = f.blob {
                    self.cache.remove(b);
                }
            }
        }
    }

    pub fn list(&self) -> Vec<ClipEntry> {
        let st = self.state.lock().unwrap();
        let mut pinned: Vec<ClipEntry> = st.entries.iter().filter(|e| e.pinned).cloned().collect();
        let mut rest: Vec<ClipEntry> = st.entries.iter().filter(|e| !e.pinned).cloned().collect();
        pinned.append(&mut rest);
        pinned
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn set_pinned(&self, id: u64, pinned: bool) -> bool {
        let (found, evicted) = {
            let mut st = self.state.lock().unwrap();
            let found = st
                .entries
                .iter_mut()
                .find(|e| e.id == id)
                .map(|e| e.pinned = pinned)
                .is_some();
            let evicted = if found { Self::evict(&mut st) } else { Vec::new() };
            (found, evicted)
        };
        for e in &evicted {
            self.free_blobs(e);
        }
        found
    }

    pub fn delete(&self, id: u64) -> bool {
        let removed = {
            let mut st = self.state.lock().unwrap();
            st.entries
                .iter()
                .position(|e| e.id == id)
                .map(|pos| st.entries.remove(pos).unwrap())
        };
        match removed {
            Some(e) => {
                self.free_blobs(&e);
                true
            }
            None => false,
        }
    }

    pub fn clear(&self, keep_pinned: bool) {
        let removed: Vec<ClipEntry> = {
            let mut st = self.state.lock().unwrap();
            let removed = if keep_pinned {
                let mut keep = VecDeque::new();
                let mut removed = Vec::new();
                for e in st.entries.drain(..) {
                    if e.pinned {
                        keep.push_back(e);
                    } else {
                        removed.push(e);
                    }
                }
                st.entries = keep;
                removed
            } else {
                st.entries.drain(..).collect()
            };
            st.last_seen = None;
            removed
        };
        for e in &removed {
            self.free_blobs(e);
        }
    }

    pub fn text_of(&self, id: u64) -> Option<String> {
        let st = self.state.lock().unwrap();
        st.entries.iter().find(|e| e.id == id).map(|e| e.text.clone())
    }

    pub fn copy_to_clipboard(&self, id: u64) -> Result<(), String> {
        let entry = {
            let st = self.state.lock().unwrap();
            st.entries.iter().find(|e| e.id == id).cloned()
        };
        let entry = entry.ok_or("voce non trovata")?;
        match entry.kind {
            ClipKind::Text => {
                crate::adapters::clipboard::write_text(&entry.text)?;
            }
            ClipKind::Image => {
                let img = entry.image.as_ref().ok_or("immagine non disponibile")?;
                let path = self.cache.path(img.blob).ok_or("immagine non più in cache")?;
                crate::adapters::clipboard::write_image(&path)?;
            }
            ClipKind::Files => {
                let files = entry.files.as_ref().ok_or("file non disponibili")?;
                let paths: Vec<PathBuf> = files
                    .iter()
                    .filter_map(|f| f.blob.and_then(|b| self.cache.path(b)))
                    .collect();
                if paths.is_empty() {
                    return Err("nessun file copiabile (troppo grande o non più in cache)".into());
                }
                crate::adapters::clipboard::write_files(&paths)?;
            }
        }
        self.mark_written(&entry.sig);
        Ok(())
    }

    pub fn blob_for(&self, id: u64, index: usize) -> Option<BlobServe> {
        let (blob, name, mime, inline) = {
            let st = self.state.lock().unwrap();
            let e = st.entries.iter().find(|e| e.id == id)?;
            if let Some(img) = &e.image {
                (img.blob, "immagine.png".to_string(), img.mime.clone(), true)
            } else {
                let f = e.files.as_ref()?.get(index)?;
                (f.blob?, f.name.clone(), "application/octet-stream".to_string(), false)
            }
        };
        let path = self.cache.path(blob)?;
        Some(BlobServe { path, name, mime, inline })
    }

    pub fn mark_written(&self, sig: &str) {
        let mut st = self.state.lock().unwrap();
        st.last_seen = Some(sig.to_string());
    }
}

fn files_sig(paths: &[PathBuf]) -> String {
    let mut s = String::from("\u{0}files");
    for p in paths {
        s.push('\u{0}');
        s.push_str(&p.to_string_lossy());
    }
    s
}

fn image_sig(width: u32, height: u32, size: u64) -> String {
    format!("\u{0}image\u{0}{width}x{height}\u{0}{size}")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_consecutivo_e_skip_vuoti() {
        let h = ClipboardHistory::new();
        h.record("ciao".into());
        h.record("ciao".into());
        h.record("   ".into());
        assert_eq!(h.list().len(), 1);
        assert_eq!(h.list()[0].text, "ciao");
    }

    #[test]
    fn ricopiare_un_vecchio_lo_riporta_in_cima_senza_duplicare() {
        let h = ClipboardHistory::new();
        h.record("uno".into());
        h.record("due".into());
        h.record("uno".into());
        let list = h.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].text, "uno");
    }

    #[test]
    fn pin_protegge_dalla_eviction_e_ordina_prima() {
        let h = ClipboardHistory::new();
        h.record("importante".into());
        let id = h.list()[0].id;
        assert!(h.set_pinned(id, true));
        for i in 0..(MAX_ENTRIES + 10) {
            h.record(format!("v{i}"));
        }
        let list = h.list();
        assert_eq!(list[0].text, "importante");
        assert!(list[0].pinned);
        assert!(list.iter().any(|e| e.id == id));
        assert_eq!(list.iter().filter(|e| !e.pinned).count(), MAX_ENTRIES);
    }

    #[test]
    fn clear_con_e_senza_pinned() {
        let h = ClipboardHistory::new();
        h.record("a".into());
        h.record("b".into());
        let id = h.list().iter().find(|e| e.text == "a").unwrap().id;
        h.set_pinned(id, true);
        h.clear(true);
        assert_eq!(h.list().len(), 1);
        assert_eq!(h.list()[0].text, "a");
        h.clear(false);
        assert!(h.list().is_empty());
    }

    #[test]
    fn delete_e_text_of() {
        let h = ClipboardHistory::new();
        h.record("segreto".into());
        let id = h.list()[0].id;
        assert_eq!(h.text_of(id).as_deref(), Some("segreto"));
        assert!(h.delete(id));
        assert!(!h.delete(id));
        assert!(h.text_of(id).is_none());
    }

    #[test]
    fn mark_written_evita_ricattura() {
        let h = ClipboardHistory::new();
        h.mark_written("dal-server");
        h.record("dal-server".into());
        assert!(h.list().is_empty());
    }

    #[test]
    fn json_contratto_ui_camelcase() {
        let files_entry = ClipEntry {
            id: 7,
            kind: ClipKind::Files,
            text: "report.pdf".into(),
            bytes: 1234,
            copied_at: 111,
            pinned: false,
            files: Some(vec![ClipFile {
                name: "report.pdf".into(),
                size: 1234,
                has_blob: true,
                blob: Some(3),
            }]),
            image: None,
            sig: "x".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&files_entry).unwrap();
        assert_eq!(v["kind"], "files");
        assert_eq!(v["copiedAt"], 111);
        assert_eq!(v["files"][0]["hasBlob"], true);
        assert_eq!(v["files"][0]["name"], "report.pdf");
        assert!(v.get("sig").is_none(), "sig non deve essere serializzato");
        assert!(v["files"][0].get("blob").is_none(), "blob id interno, non serializzato");

        let image_entry = ClipEntry {
            id: 8,
            kind: ClipKind::Image,
            text: "Immagine 4×4".into(),
            bytes: 73,
            copied_at: 222,
            pinned: true,
            files: None,
            image: Some(ClipImage { mime: "image/png".into(), width: 4, height: 4, blob: 9 }),
            sig: "y".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&image_entry).unwrap();
        assert_eq!(v["kind"], "image");
        assert_eq!(v["image"]["mime"], "image/png");
        assert_eq!(v["image"]["width"], 4);
        assert!(v["image"].get("blob").is_none());
        assert!(v.get("files").is_none(), "files assente sulle immagini");
    }

    #[test]
    fn record_files_metadata_e_blob_sotto_limite() {
        let h = ClipboardHistory::new();
        let dir = std::env::temp_dir().join(format!("rdt-clip-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("nota.txt");
        std::fs::write(&f, b"contenuto").unwrap();
        h.record_files(vec![f.clone()]);
        let list = h.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, ClipKind::Files);
        assert_eq!(list[0].text, "nota.txt");
        let files = list[0].files.as_ref().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].has_blob);
        let serve = h.blob_for(list[0].id, 0).unwrap();
        assert_eq!(serve.name, "nota.txt");
        assert_eq!(std::fs::read(&serve.path).unwrap(), b"contenuto");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
