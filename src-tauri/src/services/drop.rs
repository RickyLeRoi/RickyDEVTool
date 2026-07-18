use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::events::{now_ms, EventBus};

/// File drop stile Snapdrop: ogni client con la UI aperta si annuncia come
/// peer (hello periodico), vede gli altri e gli invia file o testo. Il
/// trasferimento passa dal server (che gira sul desktop): mittente → upload,
/// destinatario → notifica WS su `drop:{deviceId}` e download. Se il
/// destinatario è il desktop, il file viene salvato direttamente in
/// Downloads/RickyDEVTool (la webview non ha un download manager).

/// Un peer sparisce se non si annuncia da 45s (hello ogni 15s).
const PEER_TTL_MS: u64 = 45_000;
/// I file in transito non ritirati vengono eliminati dopo 1h.
const TRANSFER_TTL_MS: u64 = 3_600_000;
const MAX_TEXT_LEN: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub device_id: String,
    pub name: String,
    pub is_desktop: bool,
    pub last_seen: u64,
}

struct Transfer {
    name: String,
    path: PathBuf,
    /// Percorso finale se salvato direttamente su disco (destinatario desktop).
    saved_path: Option<String>,
    created_at: u64,
}

pub struct DropService {
    bus: EventBus,
    peers: Mutex<HashMap<String, PeerInfo>>,
    transfers: Mutex<HashMap<String, Transfer>>,
    counter: AtomicU64,
    transfer_dir: PathBuf,
    received_dir: PathBuf,
}

fn sanitize_name(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .trim_start_matches('.');
    let cleaned: String = base
        .chars()
        .map(|c| if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    if cleaned.is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

/// "report.pdf" → "report (1).pdf" finché non trova un nome libero.
fn dedupe_path(dir: &PathBuf, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 1..1000 {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{ext}", now_ms()))
}

impl DropService {
    pub fn new(bus: EventBus) -> Self {
        let transfer_dir = crate::config::data_dir().join("drop-transfers");
        // Residui di sessioni precedenti: si riparte puliti.
        let _ = std::fs::remove_dir_all(&transfer_dir);
        let _ = std::fs::create_dir_all(&transfer_dir);
        let received_dir = dirs::download_dir()
            .unwrap_or_else(crate::config::data_dir)
            .join("RickyDEVTool");
        Self {
            bus,
            peers: Mutex::new(HashMap::new()),
            transfers: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(1),
            transfer_dir,
            received_dir,
        }
    }

    pub fn received_dir(&self) -> &PathBuf {
        &self.received_dir
    }

    /// Registra/aggiorna un peer e ritorna la lista aggiornata (senza il chiamante).
    pub fn hello(&self, device_id: &str, name: &str, is_desktop: bool) -> Vec<PeerInfo> {
        let now = now_ms();
        {
            let mut peers = self.peers.lock().expect("peers lock");
            peers.retain(|_, p| now.saturating_sub(p.last_seen) < PEER_TTL_MS);
            peers.insert(
                device_id.to_string(),
                PeerInfo {
                    device_id: device_id.to_string(),
                    name: name.trim().chars().take(40).collect(),
                    is_desktop,
                    last_seen: now,
                },
            );
        }
        self.cleanup_transfers();
        self.publish_peers();
        self.peers_except(device_id)
    }

    pub fn peers_except(&self, device_id: &str) -> Vec<PeerInfo> {
        let now = now_ms();
        let peers = self.peers.lock().expect("peers lock");
        let mut list: Vec<PeerInfo> = peers
            .values()
            .filter(|p| p.device_id != device_id && now.saturating_sub(p.last_seen) < PEER_TTL_MS)
            .cloned()
            .collect();
        list.sort_by(|a, b| b.is_desktop.cmp(&a.is_desktop).then(a.name.cmp(&b.name)));
        list
    }

    fn peer_is_desktop(&self, device_id: &str) -> Option<bool> {
        let now = now_ms();
        let peers = self.peers.lock().expect("peers lock");
        peers
            .get(device_id)
            .filter(|p| now.saturating_sub(p.last_seen) < PEER_TTL_MS)
            .map(|p| p.is_desktop)
    }

    /// Dove scrivere il file in arrivo per `to_device`.
    /// Desktop → direttamente nella cartella dei ricevuti; altri → dir di transito.
    pub fn prepare_incoming(&self, to_device: &str, file_name: &str) -> Result<(String, PathBuf, bool), String> {
        let is_desktop = self
            .peer_is_desktop(to_device)
            .ok_or("destinatario non più connesso")?;
        let name = sanitize_name(file_name);
        let id = format!("d{}", self.counter.fetch_add(1, Ordering::Relaxed));
        let path = if is_desktop {
            let _ = std::fs::create_dir_all(&self.received_dir);
            dedupe_path(&self.received_dir, &name)
        } else {
            self.transfer_dir.join(&id)
        };
        Ok((id, path, is_desktop))
    }

    /// Registra il trasferimento completato e notifica il destinatario via WS.
    pub fn finish_incoming(
        &self,
        id: &str,
        to_device: &str,
        from_name: &str,
        file_name: &str,
        path: PathBuf,
        size_bytes: u64,
        saved_on_disk: bool,
    ) {
        let name = if saved_on_disk {
            // Il nome può essere stato dedupato: usa quello reale su disco.
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| sanitize_name(file_name))
        } else {
            sanitize_name(file_name)
        };
        let saved_path = saved_on_disk.then(|| path.to_string_lossy().to_string());
        self.transfers.lock().expect("transfers lock").insert(
            id.to_string(),
            Transfer {
                name: name.clone(),
                path,
                saved_path: saved_path.clone(),
                created_at: now_ms(),
            },
        );
        self.bus.publish(
            &format!("drop:{to_device}"),
            serde_json::json!({
                "kind": "file",
                "transferId": id,
                "name": name,
                "sizeBytes": size_bytes,
                "fromName": from_name,
                "savedPath": saved_path,
            }),
        );
    }

    pub fn send_text(&self, to_device: &str, from_name: &str, text: &str) -> Result<(), String> {
        if text.is_empty() || text.len() > MAX_TEXT_LEN {
            return Err("testo vuoto o troppo lungo".to_string());
        }
        self.peer_is_desktop(to_device)
            .ok_or("destinatario non più connesso")?;
        self.bus.publish(
            &format!("drop:{to_device}"),
            serde_json::json!({ "kind": "text", "text": text, "fromName": from_name }),
        );
        Ok(())
    }

    /// (path, nome) per servire il download di un trasferimento.
    pub fn transfer_file(&self, id: &str) -> Option<(PathBuf, String)> {
        let transfers = self.transfers.lock().expect("transfers lock");
        let t = transfers.get(id)?;
        Some((t.path.clone(), t.name.clone()))
    }

    fn cleanup_transfers(&self) {
        let now = now_ms();
        let mut transfers = self.transfers.lock().expect("transfers lock");
        transfers.retain(|_, t| {
            let keep = now.saturating_sub(t.created_at) < TRANSFER_TTL_MS;
            // I file già salvati nei ricevuti restano: si elimina solo il transito.
            if !keep && t.saved_path.is_none() {
                let _ = std::fs::remove_file(&t.path);
            }
            keep
        });
    }

    fn publish_peers(&self) {
        let now = now_ms();
        let peers = self.peers.lock().expect("peers lock");
        let list: Vec<PeerInfo> = peers
            .values()
            .filter(|p| now.saturating_sub(p.last_seen) < PEER_TTL_MS)
            .cloned()
            .collect();
        self.bus
            .publish("drop-peers", serde_json::json!({ "peers": list }));
    }

    /// File ricevuti sul desktop (cartella Downloads/RickyDEVTool).
    pub fn received_files(&self) -> Vec<ReceivedFile> {
        let mut files = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.received_dir) else {
            return files;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let meta = entry.metadata().ok();
            files.push(ReceivedFile {
                name,
                size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified_at: meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64),
            });
        }
        files.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        files
    }

    pub fn delete_received(&self, name: &str) -> Result<(), String> {
        let clean = sanitize_name(name);
        if clean != name {
            return Err("nome non valido".to_string());
        }
        std::fs::remove_file(self.received_dir.join(&clean)).map_err(|e| e.to_string())
    }

    /// Percorso validato di un file ricevuto (per aprirlo/mostrarlo).
    /// Il nome deve restare dentro la cartella dei ricevuti: niente traversal.
    pub fn received_path(&self, name: &str) -> Result<PathBuf, String> {
        let clean = sanitize_name(name);
        if clean != name {
            return Err("nome non valido".to_string());
        }
        let path = self.received_dir.join(&clean);
        if !path.is_file() {
            return Err("file non trovato".to_string());
        }
        Ok(path)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedFile {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_nomi() {
        assert_eq!(sanitize_name("report.pdf"), "report.pdf");
        assert_eq!(sanitize_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_name("C:\\x\\nota:finale.txt"), "nota_finale.txt");
        assert_eq!(sanitize_name(".env"), "env");
        assert_eq!(sanitize_name(""), "file");
    }

    #[test]
    fn peers_ttl_e_esclusione() {
        let service = DropService::new(EventBus::new());
        service.hello("a", "iPhone", false);
        service.hello("b", "Desktop", true);
        let peers = service.peers_except("a");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id, "b");
        assert!(peers[0].is_desktop);
    }

    #[test]
    fn testo_verso_peer_sconosciuto() {
        let service = DropService::new(EventBus::new());
        assert!(service.send_text("nessuno", "X", "ciao").is_err());
    }
}
