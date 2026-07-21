//! Discovery cross-host per Drop via UDP broadcast.
//!
//! Il modello di Drop originale prevede un solo "hub" (il server di questo
//! desktop): telefono e altri browser fanno hello contro DI QUESTO server e si
//! vedono tra loro. Ma se RickyDEVTool gira come app separata su due computer
//! diversi (es. Mac + Windows), sono due hub indipendenti con registri peer
//! isolati in memoria: nessuno dei due sa che l'altro esiste, anche stando
//! sulla stessa rete. Non è un problema di permessi di rete: è che manca un
//! meccanismo di scoperta tra hub.
//!
//! Questo modulo lo aggiunge: ogni istanza manda un beacon UDP broadcast ogni
//! 5s con la propria identità (hub_id stabile, nome, porta HTTP) e ascolta i
//! beacon altrui, popolando un registro di "hub remoti" con TTL. Il server
//! espone questi hub remoti nella lista peer di Drop; l'invio di file/testo
//! verso un hub remoto passa da un proxy HTTP verso il suo `/api/drop/send`
//! o `/api/drop/text`, usando come target il suo stesso hub_id (che ogni
//! DropService registra sempre come proprio peer "desktop" permanente).
//!
//! Nota Windows: la prima volta che il processo apre queste porte (TCP 6969 e
//! UDP di questo modulo), il Firewall di Windows Defender può chiedere
//! conferma; su reti "pubbliche" o senza permessi amministrativi il blocco può
//! restare silenzioso (nessun errore visibile, semplicemente non arrivano
//! beacon). È una restrizione del sistema operativo, non risolvibile lato
//! applicazione senza privilegi elevati.

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
/// ~4 beacon mancati prima di considerare l'hub sparito.
const HUB_TTL_MS: u64 = 22_000;
const MAGIC: &str = "rickydevtool-hub";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Beacon {
    app: String,
    hub_id: String,
    name: String,
    http_port: u16,
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
    /// Vuoto: usato da `start()` e nei test che non hanno bisogno di beacon reali.
    pub fn new() -> Self {
        Self { hubs: Mutex::new(HashMap::new()) }
    }

    /// Hub visti di recente (TTL applicato qui, non nell'inserimento).
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
}

/// Avvia beacon + listener in background e ritorna il registro condiviso.
/// Non fallisce mai in modo visibile: se la UDP non si apre (firewall, rete
/// che vieta il broadcast), semplicemente non si scoprirà nessun hub remoto —
/// il resto dell'app (incluso Drop in-hub) continua a funzionare.
pub fn start(config: &ConfigHandle, http_port: u16) -> Arc<HubRegistry> {
    let registry = Arc::new(HubRegistry::new());
    let hub_id = config.get().drop_hub_id.clone();
    let name = hub_name(config);

    let listen_registry = Arc::clone(&registry);
    let listen_hub_id = hub_id.clone();
    tokio::spawn(async move {
        if let Err(e) = listen_loop(listen_hub_id, listen_registry).await {
            tracing::warn!(%e, "hub discovery: listener non avviato (porta occupata o firewall)");
        }
    });

    tokio::spawn(async move {
        if let Err(e) = beacon_loop(hub_id, name, http_port).await {
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

/// Socket UDP con SO_REUSEADDR/SO_REUSEPORT: permette a più processi sulla
/// stessa macchina di condividere la porta di discovery (serve sia nei test
/// con due istanze locali, sia quando l'utente tiene aperte più copie).
fn reusable_udp_socket(port: u16) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port).into())?;
    UdpSocket::from_std(socket.into())
}

async fn beacon_loop(hub_id: String, name: String, http_port: u16) -> std::io::Result<()> {
    let socket = reusable_udp_socket(0)?;
    socket.set_broadcast(true)?;
    let beacon = Beacon { app: MAGIC.to_string(), hub_id, name, http_port };
    let payload = serde_json::to_vec(&beacon).unwrap_or_default();
    loop {
        let _ = socket
            .send_to(&payload, (Ipv4Addr::BROADCAST, DISCOVERY_PORT))
            .await;
        tokio::time::sleep(Duration::from_secs(BEACON_INTERVAL_SECS)).await;
    }
}

async fn listen_loop(self_hub_id: String, registry: Arc<HubRegistry>) -> std::io::Result<()> {
    let socket = reusable_udp_socket(DISCOVERY_PORT)?;
    let mut buf = [0u8; 1024];
    loop {
        let (len, from) = socket.recv_from(&mut buf).await?;
        handle_packet(&buf[..len], from, &self_hub_id, &registry);
    }
}

fn handle_packet(data: &[u8], from: SocketAddr, self_hub_id: &str, registry: &HubRegistry) {
    let Ok(beacon) = serde_json::from_slice::<Beacon>(data) else { return };
    if beacon.app != MAGIC || beacon.hub_id == self_hub_id {
        return; // pacchetto non nostro, o è il nostro stesso beacon rimbalzato
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

    #[test]
    fn ignora_pacchetti_non_nostri_e_il_proprio_beacon() {
        let registry = HubRegistry::new();
        let from: SocketAddr = "10.0.0.5:1234".parse().unwrap();

        // JSON valido ma app diversa: ignorato.
        let altro = serde_json::to_vec(&Beacon {
            app: "altra-app".into(),
            hub_id: "hub-x".into(),
            name: "X".into(),
            http_port: 6969,
        })
        .unwrap();
        handle_packet(&altro, from, "hub-self", &registry);
        assert!(registry.list().is_empty());

        // Il nostro stesso hub_id: ignorato (eco del proprio broadcast).
        let proprio = serde_json::to_vec(&Beacon {
            app: MAGIC.into(),
            hub_id: "hub-self".into(),
            name: "Io".into(),
            http_port: 6969,
        })
        .unwrap();
        handle_packet(&proprio, from, "hub-self", &registry);
        assert!(registry.list().is_empty());

        // Beacon legittimo di un altro hub: registrato.
        let altrui = serde_json::to_vec(&Beacon {
            app: MAGIC.into(),
            hub_id: "hub-remoto".into(),
            name: "PC Windows".into(),
            http_port: 6970,
        })
        .unwrap();
        handle_packet(&altrui, from, "hub-self", &registry);
        let hubs = registry.list();
        assert_eq!(hubs.len(), 1);
        assert_eq!(hubs[0].hub_id, "hub-remoto");
        assert_eq!(hubs[0].ip, "10.0.0.5");
        assert_eq!(hubs[0].http_port, 6970);
    }

    #[test]
    fn pacchetto_non_json_non_va_in_panico() {
        let registry = HubRegistry::new();
        let from: SocketAddr = "10.0.0.5:1234".parse().unwrap();
        handle_packet(b"non e' json", from, "hub-self", &registry);
        assert!(registry.list().is_empty());
    }
}
