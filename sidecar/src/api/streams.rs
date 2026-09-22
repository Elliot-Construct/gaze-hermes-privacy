use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::Response;

use super::AppState;
use crate::protocol::RequestNamespace;
use crate::streaming::StreamRestorer;

pub async fn stream_ws(
    State(state): State<AppState>,
    Path(stream_id): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, axum::http::StatusCode> {
    Ok(upgrade.on_upgrade(move |socket| handle_socket(state, stream_id, socket)))
}

async fn handle_socket(state: AppState, stream_id: String, mut socket: WebSocket) {
    let mut opened = false;
    let mut finished = false;

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                let parsed: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => {
                        send_err(&mut socket, "invalid_message").await;
                        continue;
                    }
                };
                match parsed["type"].as_str() {
                    Some("open") => {
                        if opened {
                            send_err(&mut socket, "stream_already_open").await;
                            continue;
                        }
                        let namespace: RequestNamespace = match serde_json::from_value(
                            parsed["namespace"].clone(),
                        ) {
                            Ok(ns) => ns,
                            Err(_) => {
                                send_err(&mut socket, "invalid_namespace").await;
                                continue;
                            }
                        };
                        match open_restorer(&state, &namespace).await {
                            Ok(restorer) => match state.streams.open(&stream_id, restorer) {
                                Ok(()) => {
                                    opened = true;
                                    let reply = serde_json::json!({
                                        "type": "opened",
                                        "stream_id": stream_id
                                    })
                                    .to_string();
                                    let _ = socket.send(Message::Text(reply.into())).await;
                                }
                                Err(err) => send_err(&mut socket, err.code()).await,
                            },
                            Err(code) => send_err(&mut socket, &code).await,
                        }
                    }
                    Some("chunk") if opened && !finished => {
                        let seq = parsed["seq"].as_u64();
                        let kind = parsed["kind"].as_str().unwrap_or("text").to_string();
                        let chunk = parsed["text"].as_str().unwrap_or("").to_string();
                        let Some(seq) = seq else {
                            send_err(&mut socket, "invalid_chunk").await;
                            continue;
                        };
                        let result = state
                            .streams
                            .get(&stream_id)
                            .and_then(|guard| guard.feed(seq, &kind, &chunk));
                        match result {
                            Ok(text) => {
                                let reply = serde_json::json!({
                                    "type": "chunk",
                                    "seq": seq,
                                    "text": text,
                                })
                                .to_string();
                                let _ = socket.send(Message::Text(reply.into())).await;
                            }
                            Err(err) => {
                                send_err(&mut socket, err.code()).await;
                                let _ = socket.send(Message::Close(None)).await;
                                state.streams.abort(&stream_id).ok();
                                return;
                            }
                        }
                    }
                    Some("finish") if opened && !finished => {
                        let result = state
                            .streams
                            .get(&stream_id)
                            .and_then(|guard| guard.finish());
                        match result {
                            Ok(()) => {
                                finished = true;
                                let reply = serde_json::json!({"type": "finished"}).to_string();
                                let _ = socket.send(Message::Text(reply.into())).await;
                            }
                            Err(err) => {
                                send_err(&mut socket, err.code()).await;
                                let _ = socket.send(Message::Close(None)).await;
                                state.streams.abort(&stream_id).ok();
                                return;
                            }
                        }
                    }
                    Some("abort") => {
                        state.streams.abort(&stream_id).ok();
                        let reply = serde_json::json!({"type": "aborted"}).to_string();
                        let _ = socket.send(Message::Text(reply.into())).await;
                        let _ = socket.send(Message::Close(None)).await;
                        return;
                    }
                    _ => send_err(&mut socket, "invalid_message").await,
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    state.streams.abort(&stream_id).ok();
}

async fn open_restorer(
    state: &AppState,
    namespace: &RequestNamespace,
) -> Result<StreamRestorer, &'static str> {
    let key = namespace.session_key();
    let handle = state
        .sessions
        .get_or_restore(&key)
        .await
        .map_err(|_| "session_unavailable")?;
    Ok(StreamRestorer::new(handle))
}

async fn send_err(socket: &mut WebSocket, code: &str) {
    let reply = serde_json::json!({"type": "error", "code": code}).to_string();
    let _ = socket.send(Message::Text(reply.into())).await;
}
