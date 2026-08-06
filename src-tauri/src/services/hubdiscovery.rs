use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

use crate::config::ConfigHandle;
use crate::events::now_ms;

pub const DISCOVERY_PORT: u16 = 51969;
const BEACON_INTERVAL_SECS: u64 = 5;
const HUB_TTL_MS: u64 = 22_000;
const MAGIC: &str = "rickydevtool-hub";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Beacon {
    app: String,
    hub_id: String,
    name: String,
    http_port: u16,
    // 20260806 ++ RG #Drop HMAC-SHA256 del beacon col codice hub condiviso. Senza di lui
    // chiunque in LAN si registrava come hub e saltava il pairing su /api/drop/send.
    #[serde(default)]
    sig: String,
}

fn beacon_signature(hub_id: &str, name: &str, http_port: u16, code: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(code.as_bytes())
        .expect("HMAC accetta chiavi di qualunque lunghezza");
    mac.update(hub_id.as_bytes());
    mac.update(b"\0");
    mac.update(name.as_bytes());
    mac.update(b"\0");
    mac.update(http_port.to_string().as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// 20260806 ++ RG #Drop il codice è normalizzato prima dell'uso: l'utente lo ridigita a mano
// sull'altro PC, spazi e maiuscole non devono farlo fallire.
pub fn normalize_hub_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn signature_valid(beacon: &Beacon, code: &str) -> bool {
    if code.is_empty() || beacon.sig.is_empty() {
        return false;
    }
    let expected = beacon_signature(&beacon.hub_id, &beacon.name, beacon.http_port, code);
    // confronto a tempo costante: le due firme hanno sempre la stessa lunghezza
    if expected.len() != beacon.sig.len() {
        return false;
    }
    expected
        .bytes()
        .zip(beacon.sig.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteHub {
    pub hub_id: String,
    pub name: String,
    pub ip: String,
    pub http_port: u16,
    pub last_seen: u64,
}

pub struct HubRegistry {
    hubs: Mutex<HashMap<String, RemoteHub>>,
}

impl HubRegistry {
    pub fn new() -> Self {
        Self { hubs: Mutex::new(HashMap::new()) }
    }

    pub fn list(&self) -> Vec<RemoteHub> {
        let now = now_ms();
        let mut hubs = self.hubs.lock().expect("hubs lock");
        hubs.retain(|_, h| now.saturating_sub(h.last_seen) < HUB_TTL_MS);
        let mut list: Vec<RemoteHub> = hubs.values().cloned().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn get(&self, hub_id: &str) -> Option<RemoteHub> {
        self.list().into_iter().find(|h| h.hub_id == hub_id)
    }

    fn upsert(&self, hub: RemoteHub) {
        self.hubs.lock().expect("hubs lock").insert(hub.hub_id.clone(), hub);
    }

    // 20260806 ++ RG #Drop cambiare il codice invalida gli hub già scoperti: erano stati
    // verificati con la chiave vecchia, si devono riannunciare con quella nuova.
    pub fn clear(&self) {
        self.hubs.lock().expect("hubs lock").clear();
    }
}

pub fn start(config: &ConfigHandle, http_port: u16) -> Arc<HubRegistry> {
    let registry = Arc::new(HubRegistry::new());
    let hub_id = config.get().drop_hub_id.clone();

    let listen_registry = Arc::clone(&registry);
    let listen_hub_id = hub_id.clone();
    let listen_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = listen_loop(listen_hub_id, listen_config, listen_registry).await {
            tracing::warn!(%e, "hub discovery: listener non avviato (porta occupata o firewall)");
        }
    });

    let beacon_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = beacon_loop(hub_id, beacon_config, http_port).await {
            tracing::warn!(%e, "hub discovery: beacon non avviato");
        }
    });

    registry
}

pub(crate) fn hub_name(config: &ConfigHandle) -> String {
    let cfg = config.get();
    if !cfg.drop_hub_name.trim().is_empty() {
        return cfg.drop_hub_name.clone();
    }
    sysinfo::System::host_name().unwrap_or_else(|| "RickyDEVTool".to_string())
}

fn reusable_udp_socket(port: u16) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    // 20260704 RG alla prima bind il Firewall di Windows Defender chiede conferma all'utente.
    socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port).into())?;
    UdpSocket::from_std(socket.into())
}

// 20260806 ++ RG #Drop nome e codice si rileggono a ogni giro: cambiarli dalle Impostazioni
// deve avere effetto senza riavviare l'app. Codice vuoto = nessun beacon, la funzione è spenta.
async fn beacon_loop(hub_id: String, config: ConfigHandle, http_port: u16) -> std::io::Result<()> {
    let socket = reusable_udp_socket(0)?;
    socket.set_broadcast(true)?;
    loop {
        let code = normalize_hub_code(&config.get().drop_hub_code);
        if !code.is_empty() {
            let name = hub_name(&config);
            let beacon = Beacon {
                app: MAGIC.to_string(),
                sig: beacon_signature(&hub_id, &name, http_port, &code),
                hub_id: hub_id.clone(),
                name,
                http_port,
            };
            let payload = serde_json::to_vec(&beacon).unwrap_or_default();
            let _ = socket
                .send_to(&payload, (Ipv4Addr::BROADCAST, DISCOVERY_PORT))
                .await;
        }
        tokio::time::sleep(Duration::from_secs(BEACON_INTERVAL_SECS)).await;
    }
}

async fn listen_loop(
    self_hub_id: String,
    config: ConfigHandle,
    registry: Arc<HubRegistry>,
) -> std::io::Result<()> {
    let socket = reusable_udp_socket(DISCOVERY_PORT)?;
    let mut buf = [0u8; 1024];
    loop {
        let (len, from) = socket.recv_from(&mut buf).await?;
        let code = normalize_hub_code(&config.get().drop_hub_code);
        handle_packet(&buf[..len], from, &self_hub_id, &code, &registry);
    }
}

fn handle_packet(
    data: &[u8],
    from: SocketAddr,
    self_hub_id: &str,
    code: &str,
    registry: &HubRegistry,
) {
    let Ok(beacon) = serde_json::from_slice::<Beacon>(data) else { return };
    if beacon.app != MAGIC || beacon.hub_id == self_hub_id {
        return;
    }
    // un hub entra nel registro solo se prova di conoscere il codice condiviso: da qui in poi
    // auth_middleware si fida di lui per saltare il pairing sui drop hub-to-hub.
    if !signature_valid(&beacon, code) {
        tracing::debug!(hub = %beacon.hub_id, %from, "beacon hub scartato: firma assente o non valida");
        return;
    }
    registry.upsert(RemoteHub {
        hub_id: beacon.hub_id,
        name: beacon.name,
        ip: from.ip().to_string(),
        http_port: beacon.http_port,
        last_seen: now_ms(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODICE: &str = "k7f29m4xtq81";

    fn beacon_firmato(hub_id: &str, name: &str, port: u16, code: &str) -> Vec<u8> {
        serde_json::to_vec(&Beacon {
            app: MAGIC.into(),
            hub_id: hub_id.into(),
            name: name.into(),
            http_port: port,
            sig: beacon_signature(hub_id, name, port, code),
        })
        .unwrap()
    }

    #[test]
    fn ignora_pacchetti_non_nostri_e_il_proprio_beacon() {
        let registry = HubRegistry::new();
        let from: SocketAddr = "10.0.0.5:1234".parse().unwrap();

        let altro = serde_json::to_vec(&Beacon {
            app: "altra-app".into(),
            hub_id: "hub-x".into(),
            name: "X".into(),
            http_port: 6969,
            sig: beacon_signature("hub-x", "X", 6969, CODICE),
        })
        .unwrap();
        handle_packet(&altro, from, "hub-self", CODICE, &registry);
        assert!(registry.list().is_empty());

        let proprio = beacon_firmato("hub-self", "Io", 6969, CODICE);
        handle_packet(&proprio, from, "hub-self", CODICE, &registry);
        assert!(registry.list().is_empty());

        let altrui = beacon_firmato("hub-remoto", "PC Windows", 6970, CODICE);
        handle_packet(&altrui, from, "hub-self", CODICE, &registry);
        let hubs = registry.list();
        assert_eq!(hubs.len(), 1);
        assert_eq!(hubs[0].hub_id, "hub-remoto");
        assert_eq!(hubs[0].ip, "10.0.0.5");
        assert_eq!(hubs[0].http_port, 6970);
    }

    #[test]
    fn un_beacon_non_firmato_non_diventa_un_hub() {
        let registry = HubRegistry::new();
        let from: SocketAddr = "10.0.0.66:1234".parse().unwrap();

        // com'era prima del fix: nessuna firma. È il caso dell'estraneo in LAN che si
        // registra come hub e poi salta il pairing su /api/drop/send.
        let nudo = serde_json::to_vec(&Beacon {
            app: MAGIC.into(),
            hub_id: "hub-ostile".into(),
            name: "Intruso".into(),
            http_port: 6969,
            sig: String::new(),
        })
        .unwrap();
        handle_packet(&nudo, from, "hub-self", CODICE, &registry);
        assert!(registry.list().is_empty(), "senza firma non si entra nel registro");

        let firma_sbagliata = beacon_firmato("hub-ostile", "Intruso", 6969, "codice-indovinato");
        handle_packet(&firma_sbagliata, from, "hub-self", CODICE, &registry);
        assert!(registry.list().is_empty(), "firma con un altro codice non vale");

        // e nemmeno riusando una firma valida su un beacon manomesso
        let mut manomesso: Beacon =
            serde_json::from_slice(&beacon_firmato("hub-remoto", "PC Windows", 6970, CODICE))
                .unwrap();
        manomesso.http_port = 9999;
        handle_packet(
            &serde_json::to_vec(&manomesso).unwrap(),
            from,
            "hub-self",
            CODICE,
            &registry,
        );
        assert!(registry.list().is_empty(), "la porta è dentro la firma");
    }

    #[test]
    fn senza_codice_configurato_nessun_hub_e_accettato() {
        let registry = HubRegistry::new();
        let from: SocketAddr = "10.0.0.5:1234".parse().unwrap();

        // il codice vuoto non deve diventare "chiave vuota che valida tutto": la funzione
        // hub-to-hub è semplicemente spenta.
        handle_packet(&beacon_firmato("hub-remoto", "PC", 6970, ""), from, "hub-self", "", &registry);
        assert!(registry.list().is_empty());
        handle_packet(&beacon_firmato("hub-remoto", "PC", 6970, CODICE), from, "hub-self", "", &registry);
        assert!(registry.list().is_empty());
    }

    #[test]
    fn il_codice_si_normalizza_prima_del_confronto() {
        assert_eq!(normalize_hub_code("K7F2-9M4X-TQ81"), CODICE);
        assert_eq!(normalize_hub_code(" k7f2 9m4x tq81 "), CODICE);
        assert_eq!(normalize_hub_code(""), "");

        // due PC che scrivono lo stesso codice in modo diverso si devono comunque vedere
        let registry = HubRegistry::new();
        let from: SocketAddr = "10.0.0.5:1234".parse().unwrap();
        let beacon = beacon_firmato("hub-remoto", "PC", 6970, &normalize_hub_code("K7F2-9M4X-TQ81"));
        handle_packet(&beacon, from, "hub-self", &normalize_hub_code("k7f2 9m4x tq81"), &registry);
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn pacchetto_non_json_non_va_in_panico() {
        let registry = HubRegistry::new();
        let from: SocketAddr = "10.0.0.5:1234".parse().unwrap();
        handle_packet(b"non e' json", from, "hub-self", CODICE, &registry);
        assert!(registry.list().is_empty());
    }
}
