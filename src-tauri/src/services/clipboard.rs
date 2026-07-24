//! Storico degli appunti (stile Windows+V): un thread campiona la clipboard di
//! sistema e ne conserva la cronologia. Il **testo vive solo in memoria** — mai
//! su disco: contiene spesso password e token. I **file e le immagini** copiati
//! finiscono invece in una cache su disco temporanea ([`clipboard_cache`])
//! cancellata a ogni riavvio, così si può ri-copiarli o salvarli anche dopo.
//! La cattura è mettibile in pausa e la cronologia svuotabile.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::adapters::clipboard::ClipRead;
use crate::services::clipboard_cache::{BlobCache, BlobId};

/// Ogni quanto leggere la clipboard.
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
/// Voci non fissate conservate al massimo (le fissate non contano).
const MAX_ENTRIES: usize = 100;
/// Testo più lungo di così non viene catturato (probabile copia di un file
/// intero): evita di gonfiare la memoria.
const MAX_TEXT_BYTES: usize = 256 * 1024;
/// File più grandi di così: si tiene solo il nome/dimensione, non una copia del
/// contenuto (scelta con l'utente per non riempire il disco).
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
    /// `true` se una copia del contenuto è disponibile in cache (scaricabile /
    /// ri-copiabile). `false` per cartelle o file oltre il limite.
    pub has_blob: bool,
    /// Id del blob in cache (non serializzato: uso interno).
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
    /// Etichetta/anteprima mostrata in lista (per i file: il nome o "N file";
    /// per l'immagine: le dimensioni).
    pub text: String,
    pub bytes: u64,
    pub copied_at: u64,
    pub pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<ClipFile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ClipImage>,
    /// Firma di deduplica (non serializzata): identifica il contenuto per
    /// evitare doppioni e riportare in cima una copia ripetuta.
    #[serde(skip)]
    pub sig: String,
}

struct State {
    entries: VecDeque<ClipEntry>,
    /// Ultima firma vista dal sampler: evita di ri-registrare lo stesso
    /// contenuto a ogni giro di polling.
    last_seen: Option<String>,
}

/// Cosa serve al server per servire un blob (immagine inline o file in download).
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
    /// Ultimo contatore di modifica clipboard visto: gating del polling.
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

    /// Avvia il sampler sempre attivo su un thread OS dedicato.
    pub fn start() -> Arc<Self> {
        // Pulisci i blob di sessioni precedenti prima di creare la nuova cache.
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

    // --------------------------- registrazione ---------------------------

    /// Registra un testo appena visto in clipboard. Idempotente sullo stesso
    /// contenuto; scarta vuoti e testi enormi.
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

    /// Registra una copia di uno o più file: nome + dimensione, e — se sotto al
    /// limite — una copia del contenuto in cache.
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

    /// Registra un'immagine copiata (PNG già scritto su `png_path`, che viene
    /// adottato dalla cache).
    pub fn record_image(&self, png_path: PathBuf, mime: String, width: u32, height: u32) {
        let size = std::fs::metadata(&png_path).map(|m| m.len()).unwrap_or(0);
        let sig = image_sig(width, height, size);
        if self.is_duplicate_or_promote(&sig) {
            let _ = std::fs::remove_file(&png_path); // già in storico: scarta il temp
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

    /// `true` se il contenuto (per firma) è un doppione consecutivo o esisteva
    /// già (in tal caso lo riporta in cima riusandone id/blob/pinned).
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

    /// Rimuove le voci non fissate più vecchie oltre il limite; restituisce le
    /// voci sfrattate perché chi chiama ne liberi i blob (fuori dal lock).
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

    // ------------------------------ lettura ------------------------------

    pub fn list(&self) -> Vec<ClipEntry> {
        // Fissate prima (in ordine di copia), poi le altre per recency.
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

    /// Svuota la cronologia. Con `keep_pinned` conserva le voci fissate.
    ///
    /// Non serve toccare la clipboard di sistema: svuotare lo storico non la
    /// modifica, quindi il gating sul contatore impedisce già che l'ultimo
    /// elemento riappaia al prossimo giro (ma `last_seen` va azzerato perché una
    /// *nuova* copia dello stesso contenuto torni a registrarsi).
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

    /// Etichetta testuale di una voce (per la re-copia testo lato server e per
    /// l'invio via rete). Per file/immagini è il nome/anteprima.
    pub fn text_of(&self, id: u64) -> Option<String> {
        let st = self.state.lock().unwrap();
        st.entries.iter().find(|e| e.id == id).map(|e| e.text.clone())
    }

    /// Riporta in clipboard il contenuto di una voce (testo, immagine o file).
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
        // Evita che il sampler tratti la nostra scrittura come una nuova copia.
        self.mark_written(&entry.sig);
        Ok(())
    }

    /// Percorso/metadati del blob da servire: `index` seleziona il file in una
    /// voce multi-file (ignorato per le immagini).
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

    /// Da chiamare dopo aver scritto in clipboard: la firma è marcata come
    /// "appena vista", così il prossimo giro del sampler non la ri-cattura.
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
        h.record("ciao".into()); // consecutivo identico → ignorato
        h.record("   ".into()); // vuoto (solo spazi) → ignorato
        assert_eq!(h.list().len(), 1);
        assert_eq!(h.list()[0].text, "ciao");
    }

    #[test]
    fn ricopiare_un_vecchio_lo_riporta_in_cima_senza_duplicare() {
        let h = ClipboardHistory::new();
        h.record("uno".into());
        h.record("due".into());
        h.record("uno".into()); // di nuovo "uno" (non consecutivo)
        let list = h.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].text, "uno"); // in cima
    }

    #[test]
    fn pin_protegge_dalla_eviction_e_ordina_prima() {
        let h = ClipboardHistory::new();
        h.record("importante".into());
        let id = h.list()[0].id;
        assert!(h.set_pinned(id, true));
        // riempi oltre il limite con voci non fissate
        for i in 0..(MAX_ENTRIES + 10) {
            h.record(format!("v{i}"));
        }
        let list = h.list();
        // la voce fissata sopravvive e sta in testa
        assert_eq!(list[0].text, "importante");
        assert!(list[0].pinned);
        assert!(list.iter().any(|e| e.id == id));
        // le non fissate sono limitate
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
        h.record("dal-server".into()); // il sampler lo rilegge → deve ignorarlo
        assert!(h.list().is_empty());
    }

    #[test]
    fn json_contratto_ui_camelcase() {
        // Blinda i nomi dei campi verso il frontend (types.ts): un rename
        // silenzioso romperebbe la UI senza che i test lo notino.
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
        assert!(files[0].has_blob); // sotto al limite → copia disponibile
        // il blob è servibile
        let serve = h.blob_for(list[0].id, 0).unwrap();
        assert_eq!(serve.name, "nota.txt");
        assert_eq!(std::fs::read(&serve.path).unwrap(), b"contenuto");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
