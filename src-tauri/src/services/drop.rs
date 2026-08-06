use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::config::ConfigHandle;
use crate::events::{now_ms, EventBus};

const PEER_TTL_MS: u64 = 45_000;
const TRANSFER_TTL_MS: u64 = 3_600_000;
// 20260806 ++ RG #Drop la rivendicazione di un deviceId sopravvive alla presenza: se scadesse
// coi 45s dei peer, basterebbe aspettare che il telefono chiuda l'app per prenderne il canale.
const CLAIM_TTL_MS: u64 = 24 * 3_600_000;
const MAX_TEXT_LEN: usize = 64 * 1024;
pub const MAX_PROXY_BYTES: usize = 200 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub device_id: String,
    pub name: String,
    pub is_desktop: bool,
    pub last_seen: u64,
    #[serde(default)]
    pub remote: bool,
}

struct Transfer {
    name: String,
    path: PathBuf,
    saved_path: Option<String>,
    created_at: u64,
    // a chi era destinato: serve a impedire che un altro device abbinato lo scarichi
    to_device: String,
}

struct Claim {
    secret: String,
    claimed_at: u64,
}

pub struct DropService {
    bus: EventBus,
    config: ConfigHandle,
    hub_registry: Arc<crate::services::hubdiscovery::HubRegistry>,
    peers: Mutex<HashMap<String, PeerInfo>>,
    claims: Mutex<HashMap<String, Claim>>,
    transfers: Mutex<HashMap<String, Transfer>>,
    transfer_dir: PathBuf,
    received_dir: PathBuf,
}

// 20260806 ++ RG #Drop il deviceId è pubblico (sta nell'elenco dei peer e serve come "to"):
// a decidere chi può *ricevere* su un canale è questo segreto, che non lascia mai il device.
fn secret_matches(expected: &str, given: &str) -> bool {
    if expected.is_empty() || expected.len() != given.len() {
        return false;
    }
    expected
        .bytes()
        .zip(given.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

pub fn valid_device_secret(secret: &str) -> bool {
    secret.len() >= 16 && secret.len() <= 128 && secret.chars().all(|c| c.is_ascii_alphanumeric())
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
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
    pub fn new(
        bus: EventBus,
        config: ConfigHandle,
        hub_registry: Arc<crate::services::hubdiscovery::HubRegistry>,
    ) -> Self {
        #[cfg(not(test))]
        let (transfer_dir, received_dir) = (
            crate::config::data_dir().join("drop-transfers"),
            dirs::download_dir()
                .unwrap_or_else(crate::config::data_dir)
                .join("RickyDEVTool"),
        );
        // 20260806 RG new() svuota transfer_dir: due istanze che condividono la cartella si
        // cancellano i file a vicenda, e i test non devono scrivere nei Download veri.
        #[cfg(test)]
        let (transfer_dir, received_dir) = {
            let sandbox = std::env::temp_dir().join(format!("rickydev-drop-{}", random_hex(8)));
            (sandbox.join("transfers"), sandbox.join("received"))
        };
        let _ = std::fs::remove_dir_all(&transfer_dir);
        let _ = std::fs::create_dir_all(&transfer_dir);
        Self {
            bus,
            config,
            hub_registry,
            peers: Mutex::new(HashMap::new()),
            claims: Mutex::new(HashMap::new()),
            transfers: Mutex::new(HashMap::new()),
            transfer_dir,
            received_dir,
        }
    }

    pub fn received_dir(&self) -> &PathBuf {
        &self.received_dir
    }

    pub fn hub_id(&self) -> String {
        self.config.get().drop_hub_id
    }

    pub fn remote_hubs(&self) -> Vec<crate::services::hubdiscovery::RemoteHub> {
        self.hub_registry.list()
    }

    pub fn remote_hub(&self, hub_id: &str) -> Option<crate::services::hubdiscovery::RemoteHub> {
        self.hub_registry.get(hub_id)
    }

    pub fn forget_hubs(&self) {
        self.hub_registry.clear();
    }

    // 20260806 ++ RG #Drop primo che arriva si prende il deviceId: chi lo rivendica dopo con
    // un segreto diverso viene respinto, altrimenti basterebbe leggere /api/drop/peers e
    // ripresentarsi con l'id della vittima per dirottarne il canale.
    pub fn hello(
        &self,
        device_id: &str,
        secret: &str,
        name: &str,
        is_desktop: bool,
    ) -> Result<Vec<PeerInfo>, String> {
        if !valid_device_secret(secret) {
            return Err("deviceSecret mancante o non valido".to_string());
        }
        let now = now_ms();
        {
            let mut claims = self.claims.lock().expect("claims lock");
            claims.retain(|_, c| now.saturating_sub(c.claimed_at) < CLAIM_TTL_MS);
            match claims.get_mut(device_id) {
                Some(existing) if !secret_matches(&existing.secret, secret) => {
                    tracing::warn!(device = device_id, "deviceId già rivendicato da un altro segreto");
                    return Err("questo deviceId è già in uso da un altro dispositivo".to_string());
                }
                Some(existing) => existing.claimed_at = now,
                None => {
                    claims.insert(
                        device_id.to_string(),
                        Claim { secret: secret.to_string(), claimed_at: now },
                    );
                }
            }
        }
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
                    remote: false,
                },
            );
        }
        self.cleanup_transfers();
        self.publish_peers();
        Ok(self.peers_except(device_id))
    }

    // chi presenta il segreto giusto è il proprietario del canale drop:{device_id}
    pub fn owns_channel(&self, device_id: &str, secret: &str) -> bool {
        let now = now_ms();
        let claims = self.claims.lock().expect("claims lock");
        claims
            .get(device_id)
            .filter(|c| now.saturating_sub(c.claimed_at) < CLAIM_TTL_MS)
            .is_some_and(|c| secret_matches(&c.secret, secret))
    }

    pub fn peers_except(&self, device_id: &str) -> Vec<PeerInfo> {
        let now = now_ms();
        let mut list: Vec<PeerInfo> = {
            let peers = self.peers.lock().expect("peers lock");
            peers
                .values()
                .filter(|p| p.device_id != device_id && now.saturating_sub(p.last_seen) < PEER_TTL_MS)
                .cloned()
                .collect()
        };
        for hub in self.hub_registry.list() {
            if hub.hub_id == device_id {
                continue;
            }
            list.push(PeerInfo {
                device_id: hub.hub_id,
                name: hub.name,
                is_desktop: true,
                last_seen: hub.last_seen,
                remote: true,
            });
        }
        list.sort_by(|a, b| b.is_desktop.cmp(&a.is_desktop).then(a.name.cmp(&b.name)));
        list
    }

    fn peer_is_desktop(&self, device_id: &str) -> Option<bool> {
        if device_id == self.hub_id() {
            return Some(true);
        }
        let now = now_ms();
        let peers = self.peers.lock().expect("peers lock");
        peers
            .get(device_id)
            .filter(|p| now.saturating_sub(p.last_seen) < PEER_TTL_MS)
            .map(|p| p.is_desktop)
    }

    pub fn prepare_incoming(&self, to_device: &str, file_name: &str) -> Result<(String, PathBuf, bool), String> {
        let is_desktop = self
            .peer_is_desktop(to_device)
            .ok_or("destinatario non più connesso")?;
        let name = sanitize_name(file_name);
        // 20260806 ++ RG #Drop id opaco, non un contatore: era enumerabile (d1, d2, d3…) e
        // /api/drop/download lo serviva a chiunque lo indovinasse.
        let id = format!("d-{}", random_hex(16));
        let path = if is_desktop {
            let _ = std::fs::create_dir_all(&self.received_dir);
            dedupe_path(&self.received_dir, &name)
        } else {
            self.transfer_dir.join(&id)
        };
        Ok((id, path, is_desktop))
    }

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
                to_device: to_device.to_string(),
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

    pub fn send_clipboard(&self, to_device: &str, from_name: &str, text: &str) -> Result<(), String> {
        if text.is_empty() || text.len() > MAX_TEXT_LEN {
            return Err("testo vuoto o troppo lungo".to_string());
        }
        self.peer_is_desktop(to_device)
            .ok_or("destinatario non più connesso")?;
        self.bus.publish(
            &format!("drop:{to_device}"),
            serde_json::json!({ "kind": "clipboard", "text": text, "fromName": from_name }),
        );
        Ok(())
    }

    pub async fn send_local_file(&self, to: &str, from_name: &str, source: &std::path::Path) -> Result<(), String> {
        let file_name = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or("percorso file non valido")?;

        if let Some(hub) = self.remote_hub(to) {
            // 20260806 RG la dimensione si legge dai metadati: leggere il file e *poi*
            // rifiutarlo vorrebbe dire allocarlo comunque tutto, come faceva drop_send.
            let declared = tokio::fs::metadata(source).await.map_err(|e| e.to_string())?.len();
            if declared > MAX_PROXY_BYTES as u64 {
                return Err(format!(
                    "file troppo grande per l'invio a un altro computer (max {}MB)",
                    MAX_PROXY_BYTES / 1024 / 1024
                ));
            }
            let bytes = tokio::fs::read(source).await.map_err(|e| e.to_string())?;
            if bytes.len() > MAX_PROXY_BYTES {
                return Err(format!(
                    "file troppo grande per l'invio a un altro computer (max {}MB)",
                    MAX_PROXY_BYTES / 1024 / 1024
                ));
            }
            let size = bytes.len() as u64;
            self.proxy_send_file(&hub, from_name, &file_name, bytes).await?;
            tracing::info!(to, %file_name, size, "drop: file inviato da tray a hub remoto");
            return Ok(());
        }

        let (id, path, saved_on_disk) = self.prepare_incoming(to, &file_name)?;
        let size = tokio::fs::copy(source, &path).await.map_err(|e| e.to_string())?;
        self.finish_incoming(&id, to, from_name, &file_name, path, size, saved_on_disk);
        Ok(())
    }

    pub async fn proxy_send_file(
        &self,
        hub: &crate::services::hubdiscovery::RemoteHub,
        from_name: &str,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<u64, String> {
        let size = bytes.len() as u64;
        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name.to_string());
        let form = reqwest::multipart::Form::new()
            .text("to", hub.hub_id.clone())
            .text("fromName", from_name.to_string())
            .part("file", part);
        let url = format!("http://{}:{}/api/drop/send", hub.ip, hub.http_port);
        let response = reqwest::Client::new()
            .post(&url)
            .header("X-RickyDev-Hub-Id", self.hub_id())
            .multipart(form)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| format!("invio a {} fallito: {e}", hub.name))?;
        if !response.status().is_success() {
            return Err(format!("{} ha rifiutato il file (HTTP {})", hub.name, response.status()));
        }
        Ok(size)
    }

    pub async fn proxy_send_text(
        &self,
        hub: &crate::services::hubdiscovery::RemoteHub,
        from_name: &str,
        text: &str,
    ) -> Result<(), String> {
        let url = format!("http://{}:{}/api/drop/text", hub.ip, hub.http_port);
        let body = serde_json::json!({ "to": hub.hub_id, "fromName": from_name, "text": text });
        let response = reqwest::Client::new()
            .post(&url)
            .header("X-RickyDev-Hub-Id", self.hub_id())
            .json(&body)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("invio a {} fallito: {e}", hub.name))?;
        if !response.status().is_success() {
            return Err(format!("{} ha rifiutato il messaggio (HTTP {})", hub.name, response.status()));
        }
        Ok(())
    }

    // 20260806 ++ RG #Drop l'id casuale è già una capability, ma il file resta legato al
    // destinatario: chi scarica deve provare di possedere il canale a cui era indirizzato.
    // Il desktop (loopback) può sempre riprendersi i propri.
    pub fn transfer_file(
        &self,
        id: &str,
        secret: Option<&str>,
        is_loopback: bool,
    ) -> Result<(PathBuf, String), TransferError> {
        let (path, name, to_device) = {
            let transfers = self.transfers.lock().expect("transfers lock");
            let t = transfers.get(id).ok_or(TransferError::NotFound)?;
            (t.path.clone(), t.name.clone(), t.to_device.clone())
        };
        let owner = is_loopback && to_device == self.hub_id()
            || self.owns_channel(&to_device, secret.unwrap_or_default());
        if !owner {
            tracing::warn!(transfer = id, "download rifiutato: non è il destinatario");
            return Err(TransferError::Forbidden);
        }
        Ok((path, name))
    }

    fn cleanup_transfers(&self) {
        let now = now_ms();
        let mut transfers = self.transfers.lock().expect("transfers lock");
        transfers.retain(|_, t| {
            let keep = now.saturating_sub(t.created_at) < TRANSFER_TTL_MS;
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

#[derive(Debug, PartialEq, Eq)]
pub enum TransferError {
    NotFound,
    Forbidden,
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

    fn test_service() -> DropService {
        DropService::new(
            EventBus::new(),
            ConfigHandle::in_memory(),
            Arc::new(crate::services::hubdiscovery::HubRegistry::new()),
        )
    }

    const SEGRETO_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SEGRETO_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn peers_ttl_e_esclusione() {
        let service = test_service();
        service.hello("a", SEGRETO_A, "iPhone", false).expect("hello a");
        service.hello("b", SEGRETO_B, "Desktop", true).expect("hello b");
        let peers = service.peers_except("a");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id, "b");
        assert!(peers[0].is_desktop);
    }

    #[test]
    fn un_deviceid_gia_rivendicato_non_si_dirotta() {
        let service = test_service();
        service.hello("vittima", SEGRETO_A, "iPhone", false).expect("primo hello");

        // l'attaccante legge il deviceId da /api/drop/peers e prova a ripresentarsi come lui
        let esito = service.hello("vittima", SEGRETO_B, "iPhone", false);
        assert!(esito.is_err(), "il deviceId altrui non si può rivendicare");

        assert!(service.owns_channel("vittima", SEGRETO_A));
        assert!(!service.owns_channel("vittima", SEGRETO_B));
        assert!(!service.owns_channel("vittima", ""));

        // il legittimo proprietario continua a rinnovare la presenza
        assert!(service.hello("vittima", SEGRETO_A, "iPhone", false).is_ok());
    }

    #[test]
    fn un_hello_senza_segreto_valido_e_respinto() {
        let service = test_service();
        assert!(service.hello("a", "", "iPhone", false).is_err(), "segreto assente");
        assert!(service.hello("a", "corto", "iPhone", false).is_err(), "troppo corto");
        assert!(
            service.hello("a", "non-alfanumerico!!!!!!!!", "iPhone", false).is_err(),
            "caratteri fuori alfabeto"
        );
        assert!(!service.owns_channel("a", ""), "nessuna rivendicazione registrata");
    }

    #[test]
    fn owns_channel_non_si_fa_ingannare_da_prefissi() {
        let service = test_service();
        service.hello("a", SEGRETO_A, "iPhone", false).expect("hello");
        assert!(!service.owns_channel("a", &SEGRETO_A[..16]), "prefisso del segreto");
        assert!(!service.owns_channel("a", &format!("{SEGRETO_A}x")), "segreto più lungo");
        assert!(!service.owns_channel("sconosciuto", SEGRETO_A), "device mai visto");
    }

    #[test]
    fn testo_verso_peer_sconosciuto() {
        let service = test_service();
        assert!(service.send_text("nessuno", "X", "ciao").is_err());
    }

    #[test]
    fn clipboard_pubblica_kind_dedicato() {
        let service = test_service();
        service.hello("b", SEGRETO_B, "Desktop", true).expect("hello");
        let mut rx = service.bus.subscribe();
        service.send_clipboard("b", "iPhone", "segreto").expect("send");
        let event = rx.try_recv().expect("evento pubblicato");
        assert_eq!(event.topic, "drop:b");
        assert_eq!(event.payload.get("kind").and_then(|v| v.as_str()), Some("clipboard"));
        assert_eq!(event.payload.get("text").and_then(|v| v.as_str()), Some("segreto"));
        assert!(service.send_clipboard("nessuno", "X", "ciao").is_err());
    }

    #[test]
    fn il_proprio_hub_id_e_sempre_desktop_anche_senza_hello() {
        let service = test_service();
        let hub_id = service.hub_id();
        assert!(service.send_text(&hub_id, "Altro PC", "ciao dal proxy").is_ok());
    }

    #[test]
    fn gli_id_di_trasferimento_non_sono_enumerabili() {
        let service = test_service();
        service.hello("telefono", SEGRETO_A, "iPhone", false).expect("hello");

        let (primo, _, _) = service.prepare_incoming("telefono", "a.pdf").expect("primo");
        let (secondo, _, _) = service.prepare_incoming("telefono", "b.pdf").expect("secondo");

        assert_ne!(primo, secondo);
        assert!(primo.starts_with("d-") && primo.len() == 34, "16 byte esadecimali: {primo}");
        // il vecchio schema era d1, d2, d3…: da un id non si deve poter dedurre il successivo
        assert!(!primo.chars().skip(2).all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn scarica_solo_il_destinatario() {
        let service = test_service();
        service.hello("telefono", SEGRETO_A, "iPhone", false).expect("hello");
        service.hello("altro", SEGRETO_B, "Tablet", false).expect("hello");

        let (id, path, saved) = service.prepare_incoming("telefono", "segreto.pdf").expect("prepare");
        service.finish_incoming(&id, "telefono", "Mac", "segreto.pdf", path, 9, saved);

        assert!(service.transfer_file(&id, Some(SEGRETO_A), false).is_ok(), "il destinatario");
        assert_eq!(
            service.transfer_file(&id, Some(SEGRETO_B), false),
            Err(TransferError::Forbidden),
            "un altro device abbinato non deve poterlo scaricare"
        );
        assert_eq!(
            service.transfer_file(&id, None, false),
            Err(TransferError::Forbidden),
            "senza segreto non si scarica"
        );
        assert_eq!(
            service.transfer_file(&id, None, true),
            Err(TransferError::Forbidden),
            "nemmeno da loopback: non era destinato all'hub"
        );
        assert_eq!(
            service.transfer_file("d-inesistente", Some(SEGRETO_A), false),
            Err(TransferError::NotFound)
        );
    }

    #[test]
    fn il_desktop_riprende_i_trasferimenti_del_proprio_hub() {
        let service = test_service();
        let hub_id = service.hub_id();

        let (id, path, saved) = service.prepare_incoming(&hub_id, "da-altro-pc.pdf").expect("prepare");
        service.finish_incoming(&id, &hub_id, "PC Ufficio", "da-altro-pc.pdf", path, 1, saved);

        assert!(service.transfer_file(&id, None, true).is_ok(), "il desktop è loopback");
        assert_eq!(
            service.transfer_file(&id, None, false),
            Err(TransferError::Forbidden),
            "da remoto no: il canale dell'hub è del desktop"
        );
    }
}
