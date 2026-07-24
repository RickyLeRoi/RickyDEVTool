//! Cache su disco dei blob (immagini e file copiati) dello storico appunti.
//!
//! Vive in una cartella temporanea **svuotata all'avvio** e best-effort alla
//! chiusura: il contenuto continua così a "sparire a ogni riavvio", coerente con
//! la promessa dello storico. Il **testo non passa mai di qui** (resta solo in
//! RAM): su disco finiscono solo i file/immagini che l'utente ha copiato di
//! proposito, non ciò che digita.
//!
//! Ogni blob è una **sotto-cartella** `<dir>/<id>/` che contiene un solo file
//! col suo **nome reale** (così una ri-copia incolla il file col nome giusto, e
//! il download ha il filename corretto).

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub type BlobId = u64;

/// Contatore di processo: dà a ogni cache una sotto-cartella distinta, così più
/// istanze (es. i test in parallelo) non si pestano i piedi sulla stessa dir.
static INSTANCE: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    std::env::temp_dir().join("rickydev-clipboard")
}

/// Rimuove tutta la radice della cache: da chiamare **una volta all'avvio**
/// (prima di creare la history) così i blob di sessioni precedenti spariscono,
/// coerente con "sparisce a ogni riavvio".
pub fn purge_root() {
    let _ = fs::remove_dir_all(root());
}

/// Oltre questo totale la cache sfratta i blob più vecchi (FIFO). I metadata
/// della voce restano: il blob semplicemente non è più scaricabile e la UI lo
/// segnala. Fa da tetto al consumo di disco anche con molti file grandi.
const BUDGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct BlobCache {
    dir: PathBuf,
    next_id: AtomicU64,
    state: Mutex<Inner>,
}

struct Inner {
    /// Ordine d'inserimento per l'eviction FIFO sul budget.
    order: VecDeque<BlobId>,
    /// id → (nome file, dimensione in byte).
    meta: HashMap<BlobId, (String, u64)>,
    total: u64,
}

impl BlobCache {
    pub fn new() -> Self {
        let n = INSTANCE.fetch_add(1, Ordering::Relaxed);
        let dir = root().join(format!("{}-{}", std::process::id(), n));
        // Parti da una sotto-cartella pulita e solo nostra.
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        BlobCache {
            dir,
            next_id: AtomicU64::new(1),
            state: Mutex::new(Inner {
                order: VecDeque::new(),
                meta: HashMap::new(),
                total: 0,
            }),
        }
    }

    fn sub(&self, id: BlobId) -> PathBuf {
        self.dir.join(id.to_string())
    }

    /// Copia i byte di `src` nella cache col nome `name` (usato per i file
    /// copiati). Restituisce l'id del blob.
    pub fn store_copy(&self, src: &Path, name: &str) -> io::Result<BlobId> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let sub = self.sub(id);
        fs::create_dir_all(&sub)?;
        let dst = sub.join(sanitize(name));
        let size = fs::copy(src, &dst)?;
        self.track(id, name, size);
        Ok(id)
    }

    /// Adotta un file già scritto da noi (il PNG temporaneo di un'immagine)
    /// spostandolo nella cache; fallback a copia se il rename cross-device fallisce.
    pub fn store_move(&self, src: &Path, name: &str) -> io::Result<BlobId> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let sub = self.sub(id);
        fs::create_dir_all(&sub)?;
        let dst = sub.join(sanitize(name));
        if fs::rename(src, &dst).is_err() {
            fs::copy(src, &dst)?;
            let _ = fs::remove_file(src);
        }
        let size = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
        self.track(id, name, size);
        Ok(id)
    }

    fn track(&self, id: BlobId, name: &str, size: u64) {
        let mut st = self.state.lock().unwrap();
        st.order.push_back(id);
        st.meta.insert(id, (sanitize(name), size));
        st.total += size;
        while st.total > BUDGET_BYTES {
            let Some(old) = st.order.pop_front() else { break };
            if let Some((_, sz)) = st.meta.remove(&old) {
                st.total = st.total.saturating_sub(sz);
            }
            let _ = fs::remove_dir_all(self.sub(old));
        }
    }

    /// Percorso del file del blob se ancora presente su disco (altrimenti
    /// sfrattato/mancante → `None`).
    pub fn path(&self, id: BlobId) -> Option<PathBuf> {
        let st = self.state.lock().unwrap();
        let (name, _) = st.meta.get(&id)?;
        let p = self.sub(id).join(name);
        drop(st);
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    }

    /// Rimuove un singolo blob (file su disco + contabilità).
    pub fn remove(&self, id: BlobId) {
        let mut st = self.state.lock().unwrap();
        if let Some((_, sz)) = st.meta.remove(&id) {
            st.total = st.total.saturating_sub(sz);
            if let Some(pos) = st.order.iter().position(|x| *x == id) {
                st.order.remove(pos);
            }
        }
        let _ = fs::remove_dir_all(self.sub(id));
    }
}

/// Riduce `name` al solo nome file, senza separatori o `..`: il blob resta
/// confinato nella sua sotto-cartella qualunque sia il nome originale.
fn sanitize(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if base.is_empty() || base == ".." {
        "file".to_string()
    } else {
        base.to_string()
    }
}
