//! Storico degli appunti (stile Windows+V): un thread campiona la clipboard di
//! sistema e ne conserva la cronologia **solo in memoria** — mai su disco: la
//! clipboard contiene spesso password e token, non va persistita né tra i
//! riavvii. La cattura è mettibile in pausa e la cronologia svuotabile.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

/// Ogni quanto leggere la clipboard.
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
/// Voci non fissate conservate al massimo (le fissate non contano).
const MAX_ENTRIES: usize = 100;
/// Testo più lungo di così non viene catturato (probabile copia di un file
/// intero): evita di gonfiare la memoria.
const MAX_TEXT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipEntry {
    pub id: u64,
    pub text: String,
    pub bytes: usize,
    pub copied_at: u64,
    pub pinned: bool,
}

struct State {
    entries: VecDeque<ClipEntry>,
    counter: u64,
    /// Ultimo testo visto dal sampler: evita di ri-registrare lo stesso
    /// contenuto a ogni giro di polling.
    last_seen: Option<String>,
}

pub struct ClipboardHistory {
    state: Mutex<State>,
    enabled: AtomicBool,
    next_id: AtomicU64,
}

impl ClipboardHistory {
    fn new() -> Self {
        ClipboardHistory {
            state: Mutex::new(State {
                entries: VecDeque::new(),
                counter: 0,
                last_seen: None,
            }),
            enabled: AtomicBool::new(true),
            next_id: AtomicU64::new(1),
        }
    }

    /// Avvia il sampler sempre attivo su un thread OS dedicato.
    pub fn start() -> Arc<Self> {
        let history = Arc::new(Self::new());
        if crate::adapters::clipboard::supported() {
            let weak = Arc::downgrade(&history);
            std::thread::Builder::new()
                .name("clipboard-sampler".to_string())
                .spawn(move || loop {
                    let Some(history) = weak.upgrade() else { break };
                    if history.enabled.load(Ordering::Relaxed) {
                        if let Some(text) = crate::adapters::clipboard::read_text() {
                            history.record(text);
                        }
                    }
                    drop(history);
                    std::thread::sleep(POLL_INTERVAL);
                })
                .ok();
        }
        history
    }

    /// Registra un testo appena visto in clipboard. Idempotente sullo stesso
    /// contenuto consecutivo; scarta vuoti e testi enormi.
    pub fn record(&self, text: String) {
        let trimmed_empty = text.trim().is_empty();
        if trimmed_empty || text.len() > MAX_TEXT_BYTES {
            return;
        }
        let mut st = self.state.lock().unwrap();
        if st.last_seen.as_deref() == Some(text.as_str()) {
            return;
        }
        st.last_seen = Some(text.clone());

        // Se il testo esiste già (non consecutivo), spostalo in cima invece di
        // duplicarlo, preservandone lo stato pinned.
        let pinned = if let Some(pos) = st.entries.iter().position(|e| e.text == text) {
            let existing = st.entries.remove(pos).unwrap();
            existing.pinned
        } else {
            false
        };

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        st.counter += 1;
        st.entries.push_front(ClipEntry {
            id,
            bytes: text.len(),
            copied_at: now_ms(),
            pinned,
            text,
        });
        Self::evict(&mut st);
    }

    fn evict(st: &mut State) {
        // Rimuove le voci non fissate più vecchie finché si rientra nel limite.
        while st.entries.iter().filter(|e| !e.pinned).count() > MAX_ENTRIES {
            if let Some(pos) = st.entries.iter().rposition(|e| !e.pinned) {
                st.entries.remove(pos);
            } else {
                break;
            }
        }
    }

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
        let mut st = self.state.lock().unwrap();
        let found = st.entries.iter_mut().find(|e| e.id == id).map(|e| e.pinned = pinned).is_some();
        if found {
            Self::evict(&mut st);
        }
        found
    }

    pub fn delete(&self, id: u64) -> bool {
        let mut st = self.state.lock().unwrap();
        if let Some(pos) = st.entries.iter().position(|e| e.id == id) {
            st.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// Svuota la cronologia. Con `keep_pinned` conserva le voci fissate.
    pub fn clear(&self, keep_pinned: bool) {
        // Allinea `last_seen` alla clipboard di sistema attuale (letta fuori dal
        // lock): senza questo, il prossimo giro del sampler rivedrebbe lo stesso
        // contenuto come "nuovo" e farebbe ricomparire l'ultimo elemento appena
        // svuotato. Così la lista resta vuota finché non copi qualcosa di diverso.
        let current = crate::adapters::clipboard::read_text();
        let mut st = self.state.lock().unwrap();
        if keep_pinned {
            st.entries.retain(|e| e.pinned);
        } else {
            st.entries.clear();
        }
        st.last_seen = current;
    }

    /// Testo di una voce (per la re-copia lato server).
    pub fn text_of(&self, id: u64) -> Option<String> {
        let st = self.state.lock().unwrap();
        st.entries.iter().find(|e| e.id == id).map(|e| e.text.clone())
    }

    /// Da chiamare dopo aver scritto `text` in clipboard, così il prossimo giro
    /// del sampler non lo tratta come una nuova copia.
    pub fn mark_written(&self, text: &str) {
        let mut st = self.state.lock().unwrap();
        st.last_seen = Some(text.to_string());
    }
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
}
