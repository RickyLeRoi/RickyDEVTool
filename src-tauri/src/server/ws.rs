use std::collections::HashSet;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;

use super::ServerState;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ClientMessage {
    Subscribe { topic: String },
    Unsubscribe { topic: String },
}

pub async fn ws_handler(State(state): State<ServerState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: ServerState) {
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
                    Ok(ClientMessage::Subscribe { topic }) => {
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
