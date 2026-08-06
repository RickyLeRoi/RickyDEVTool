use std::collections::HashSet;
use std::net::SocketAddr;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::Response;
use serde::Deserialize;

use super::ServerState;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ClientMessage {
    Subscribe {
        topic: String,
        // segreto del device: obbligatorio per i canali drop:{deviceId}
        #[serde(default)]
        auth: Option<String>,
    },
    Unsubscribe {
        topic: String,
    },
}

// 20260806 ++ RG #Drop sottoscriversi è un permesso di lettura, non una preferenza: i drop
// (file, testo, appunti di rete) sono per un destinatario preciso.
//
// Il topic "drop" nudo va negato a parte: handle_socket consegna un evento anche a chi ha
// sottoscritto la sua *base*, quindi "drop" varrebbe come "drop:*".
//
// task:/tail: restano invece volutamente globali: gli stessi dati sono già leggibili da
// qualunque device abbinato via GET /api/tasks e /api/tasks/{id}/log, restringere qui
// darebbe l'illusione di un confine che il REST non ha.
fn subscribe_allowed(
    drop: &crate::services::drop::DropService,
    topic: &str,
    auth: Option<&str>,
    is_loopback: bool,
) -> bool {
    if topic == "drop" {
        return false;
    }
    let Some(owner) = topic.strip_prefix("drop:") else {
        return true;
    };
    if owner == drop.hub_id() {
        // il canale dell'hub raccoglie i drop dagli altri PC: è del desktop
        return is_loopback;
    }
    drop.owns_channel(owner, auth.unwrap_or_default())
}

pub async fn ws_handler(
    State(state): State<ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let is_loopback = peer.ip().is_loopback();
    upgrade.on_upgrade(move |socket| handle_socket(socket, state, is_loopback))
}

async fn handle_socket(mut socket: WebSocket, state: ServerState, is_loopback: bool) {
    let mut events = state.bus.subscribe();
    let mut topics: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        let base = event.topic.split(':').next().unwrap_or(&event.topic);
                        if topics.contains(&event.topic) || topics.contains(base) {
                            let Ok(body) = serde_json::to_string(&event) else { continue };
                            if socket.send(Message::Text(body.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "client WS in ritardo, eventi persi");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let Message::Text(text) = message else { continue };
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Subscribe { topic, auth }) => {
                        if !subscribe_allowed(&state.drop, &topic, auth.as_deref(), is_loopback) {
                            tracing::warn!(%topic, "sottoscrizione WS rifiutata");
                            continue;
                        }
                        if topics.insert(topic.clone()) && state.pollers.known_topic(&topic) {
                            state.pollers.add_subscriber(&topic);
                        }
                    }
                    Ok(ClientMessage::Unsubscribe { topic }) => {
                        if topics.remove(&topic) && state.pollers.known_topic(&topic) {
                            state.pollers.remove_subscriber(&topic);
                        }
                    }
                    Err(e) => tracing::debug!(%e, "messaggio WS non valido"),
                }
            }
        }
    }

    for topic in topics {
        if state.pollers.known_topic(&topic) {
            state.pollers.remove_subscriber(&topic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEGRETO: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ALTRO: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn servizio() -> crate::services::drop::DropService {
        crate::services::drop::DropService::new(
            crate::events::EventBus::new(),
            crate::config::ConfigHandle::in_memory(),
            std::sync::Arc::new(crate::services::hubdiscovery::HubRegistry::new()),
        )
    }

    #[test]
    fn il_canale_drop_di_un_altro_device_e_negato() {
        let drop = servizio();
        drop.hello("vittima", SEGRETO, "iPhone", false).expect("hello");

        assert!(
            subscribe_allowed(&drop, "drop:vittima", Some(SEGRETO), false),
            "il proprietario si sottoscrive al proprio canale"
        );
        // il telefono ostile ha letto il deviceId da /api/drop/peers, ma non il segreto
        assert!(!subscribe_allowed(&drop, "drop:vittima", Some(ALTRO), false));
        assert!(!subscribe_allowed(&drop, "drop:vittima", None, false));
        assert!(
            !subscribe_allowed(&drop, "drop:vittima", None, true),
            "nemmeno da loopback: il canale non è dell'hub"
        );
    }

    #[test]
    fn il_topic_drop_nudo_non_fa_da_jolly() {
        let drop = servizio();
        drop.hello("vittima", SEGRETO, "iPhone", false).expect("hello");

        // handle_socket consegna l'evento anche a chi ha sottoscritto la base del topic:
        // senza questo divieto "drop" varrebbe come "drop:*" e aggirerebbe tutto il resto.
        assert!(!subscribe_allowed(&drop, "drop", Some(SEGRETO), false));
        assert!(!subscribe_allowed(&drop, "drop", None, true));
    }

    #[test]
    fn il_canale_dellhub_e_solo_del_desktop() {
        let drop = servizio();
        let topic = format!("drop:{}", drop.hub_id());

        assert!(subscribe_allowed(&drop, &topic, None, true), "il desktop è loopback");
        assert!(
            !subscribe_allowed(&drop, &topic, Some(SEGRETO), false),
            "il telefono non deve vedere i drop arrivati dagli altri PC"
        );
    }

    #[test]
    fn i_topic_non_drop_restano_aperti() {
        let drop = servizio();
        for topic in ["stats", "drop-peers", "task:t1", "tail:l1", "docker"] {
            assert!(
                subscribe_allowed(&drop, topic, None, false),
                "{topic} non deve richiedere autorizzazione"
            );
        }
    }
}
