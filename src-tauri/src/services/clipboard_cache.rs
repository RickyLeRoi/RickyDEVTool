use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub type BlobId = u64;

static INSTANCE: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    std::env::temp_dir().join("rickydev-clipboard")
}

pub fn purge_root() {
    let _ = fs::remove_dir_all(root());
}

use crate::constants::CLIPBOARD_CACHE_BUDGET_BYTES;

pub struct BlobCache {
    dir: PathBuf,
    next_id: AtomicU64,
    state: Mutex<Inner>,
}

struct Inner {
    order: VecDeque<BlobId>,
    meta: HashMap<BlobId, (String, u64)>,
    total: u64,
}

impl BlobCache {
    pub fn new() -> Self {
        let n = INSTANCE.fetch_add(1, Ordering::Relaxed);
        let dir = root().join(format!("{}-{}", std::process::id(), n));
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

    pub fn store_copy(&self, src: &Path, name: &str) -> io::Result<BlobId> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let sub = self.sub(id);
        fs::create_dir_all(&sub)?;
        let dst = sub.join(sanitize(name));
        let size = fs::copy(src, &dst)?;
        self.track(id, name, size);
        Ok(id)
    }

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
        while st.total > CLIPBOARD_CACHE_BUDGET_BYTES {
            let Some(old) = st.order.pop_front() else { break };
            if let Some((_, sz)) = st.meta.remove(&old) {
                st.total = st.total.saturating_sub(sz);
            }
            let _ = fs::remove_dir_all(self.sub(old));
        }
    }

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
