//! WebSocket-Endpunkt: Yjs-Sync-Protokoll (y-websocket-kompatibel), Lesemodus für r.-Kennungen.
use crate::{docs::ClientGuard, AppState};
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use yrs::sync::{DefaultProtocol, Message, MessageReader, Protocol, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{Transact, Update};

/// Ohne eingehende Nachricht (auch Pong) gilt die Verbindung nach dieser Zeit als tot.
const IDLE: Duration = Duration::from_secs(90);
const PING: Duration = Duration::from_secs(30);
/// Höchstens so viele Update-Bytes je Verbindung und Minute; darüber wird getrennt.
const BUDGET_PER_MIN: usize = 2 << 20;

pub async fn run(state: Arc<AppState>, guard: ClientGuard, socket: WebSocket, readonly: bool, author: Option<String>) {
    let room = guard.room.clone();
    let origin_tag = author.unwrap_or_default();
    let (mut sink, mut stream) = socket.split();
    let mut rx = room.tx.subscribe();
    // Awareness-Client-IDs dieser Verbindung (zum Aufräumen und gegen fremde IDs)
    let mut my_ids: HashSet<yrs::block::ClientID> = HashSet::new();
    {
        let aw = room.awareness.lock().await;
        let mut enc = EncoderV1::new();
        if DefaultProtocol.start(&aw, &mut enc).is_ok() { let _ = sink.send(WsMessage::Binary(enc.to_vec().into())).await; }
    }
    let mut ping = tokio::time::interval(PING);
    let mut last_seen = tokio::time::Instant::now();
    let (mut budget_start, mut budget_used) = (tokio::time::Instant::now(), 0usize);
    'conn: loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                last_seen = tokio::time::Instant::now();
                let data: Vec<u8> = match msg { WsMessage::Binary(b) => b.to_vec(), WsMessage::Close(_) => break, _ => continue };
                if budget_start.elapsed() > Duration::from_secs(60) { budget_start = tokio::time::Instant::now(); budget_used = 0; }
                budget_used += data.len();
                if budget_used > BUDGET_PER_MIN { tracing::warn!(pad = %room.pad_id, "Verbindung überschreitet das Datenbudget, getrennt"); break; }
                let replies = {
                    let mut aw = room.awareness.lock().await;
                    let mut dec = DecoderV1::from(data.as_slice());
                    let mut out: Vec<Message> = Vec::new();
                    for m in MessageReader::new(&mut dec) {
                        let Ok(m) = m else { break };
                        let m = match m {
                            Message::Sync(SyncMessage::Update(bytes)) | Message::Sync(SyncMessage::SyncStep2(bytes)) => {
                                if readonly { continue; }
                                match Update::decode_v1(&bytes) {
                                    Ok(u) => { let mut txn = aw.doc().transact_mut_with(origin_tag.clone()); if let Err(e) = txn.apply_update(u) { tracing::debug!("Update verworfen: {e}"); } }
                                    Err(e) => tracing::debug!("Update nicht dekodierbar: {e}"),
                                }
                                continue;
                            }
                            Message::Awareness(upd) => {
                                // nur eigene Client-IDs zulassen: die erste gesehene ID gehört dieser Verbindung
                                let ids: Vec<yrs::block::ClientID> = upd.clients.keys().copied().collect();
                                if my_ids.is_empty() { if let Some(first) = ids.first() { my_ids.insert(*first); } }
                                if ids.iter().any(|id| !my_ids.contains(id)) { continue; }
                                Message::Awareness(upd)
                            }
                            other => other,
                        };
                        match DefaultProtocol.handle_message(&mut aw, m) { Ok(Some(r)) => out.push(r), Ok(None) => {}, Err(e) => tracing::debug!("Sync-Fehler: {e}") }
                    }
                    out
                };
                for r in replies { if sink.send(WsMessage::Binary(r.encode_v1().into())).await.is_err() { break 'conn; } }
            }
            bc = rx.recv() => {
                match bc {
                    Ok(bytes) => {
                        if bytes.is_empty() { break; } // Pad gelöscht
                        if sink.send(WsMessage::Binary(bytes.as_ref().clone().into())).await.is_err() { break; }
                    }
                    // zu weit zurück: trennen, der Client verbindet neu und synchronisiert vollständig
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                    Err(_) => break,
                }
            }
            _ = ping.tick() => {
                if last_seen.elapsed() > IDLE { break; }
                if sink.send(WsMessage::Ping(Vec::new().into())).await.is_err() { break; }
            }
        }
    }
    // Awareness-Zustände dieser Verbindung entfernen (keine Geister in der Teilnehmendenliste)
    if !my_ids.is_empty() {
        let mut aw = room.awareness.lock().await;
        for id in &my_ids { aw.remove_state(*id); }
    }
    drop(guard);
    state.rooms.release(&room).await;
}
