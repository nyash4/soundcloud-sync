//! WebSocket endpoint (PROTOCOL §3): handshake, liveness, rate limiting, and relaying to the room actor.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::http::Shared;
use crate::protocol::{AckError, ClientMsg, FatalError, ServerMsg};
use crate::ratelimit::TokenBucket;
use crate::room::actor::{RoomHandle, RoomMsg};
use crate::room::{ConnId, Input, clean_name};
use crate::{ids, now_ms};

const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const PING_INTERVAL: Duration = Duration::from_secs(5);
const SILENCE_TIMEOUT: Duration = Duration::from_secs(15);
const OUTBOX_SIZE: usize = 512;
const PONG_QUEUE: usize = 8;
const RATE_PER_SEC: f64 = 20.0;
const RATE_BURST: u32 = 40;
const MAX_CLIENT_ID_LEN: usize = 64;

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub async fn upgrade(ws: WebSocketUpgrade, State(state): State<Shared>) -> Response {
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve(socket, state))
}

struct Hello {
    room_id: String,
    token: String,
    client_id: String,
    name: String,
}

async fn serve(socket: WebSocket, state: Shared) {
    let (mut sink, mut stream) = socket.split();
    let hello = match tokio::time::timeout(HELLO_TIMEOUT, read_hello(&mut stream)).await {
        Ok(Some(hello)) => hello,
        _ => {
            let message = "Первым сообщением должно быть приветствие с именем";
            return fatal(&mut sink, FatalError::BadHello, message).await;
        }
    };
    if !ids::is_room_id(&hello.room_id) {
        return fatal(&mut sink, FatalError::RoomNotFound, "Комната не найдена").await;
    }
    let Some(mut room) = lookup(&state, &hello.room_id, &mut sink).await else {
        return;
    };
    if !ids::token_matches(&hello.token, &room.token_hash) {
        let message = "Ссылка-приглашение недействительна: токен комнаты не подходит";
        return fatal(&mut sink, FatalError::BadToken, message).await;
    }

    let conn = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (outbox, inbox) = mpsc::channel(OUTBOX_SIZE);
    let (pongs, pong_inbox) = mpsc::channel(PONG_QUEUE);
    // The actor may be retiring right now; a fresh handle starts a new one.
    let mut attached = false;
    for _ in 0..3 {
        let attach = RoomMsg::Attach {
            conn,
            client_id: hello.client_id.clone(),
            name: hello.name.clone(),
            outbox: outbox.clone(),
        };
        if room.send(attach).await.is_ok() {
            attached = true;
            break;
        }
        match lookup(&state, &hello.room_id, &mut sink).await {
            Some(fresh) => room = fresh,
            None => return,
        }
    }
    if !attached {
        let _ = sink.send(Message::Close(None)).await;
        return;
    }
    debug!(room = hello.room_id, conn, "socket attached");

    drop(outbox);
    let mut writer = tokio::spawn(write_loop(sink, inbox, pong_inbox));
    tokio::select! {
        () = read_loop(stream, &room, conn, &pongs) => {}
        _ = &mut writer => {}
    }
    let _ = room.send(RoomMsg::Input(Input::Disconnect { conn })).await;
    writer.abort();
    debug!(room = hello.room_id, conn, "socket detached");
}

async fn lookup(
    state: &Shared,
    room_id: &str,
    sink: &mut SplitSink<WebSocket, Message>,
) -> Option<RoomHandle> {
    match state.rooms.get(room_id).await {
        Ok(Some(room)) => Some(room),
        Ok(None) => {
            fatal(sink, FatalError::RoomNotFound, "Комната не найдена").await;
            None
        }
        Err(e) => {
            // Not fatal for the client: it reconnects with backoff.
            error!(room = room_id, "loading room failed: {e:#}");
            let _ = sink.send(Message::Close(None)).await;
            None
        }
    }
}

/// Waits for the first data frame and validates it as `hello`.
async fn read_hello(stream: &mut SplitStream<WebSocket>) -> Option<Hello> {
    let text = loop {
        match stream.next().await?.ok()? {
            Message::Text(text) => break text,
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => return None,
        }
    };
    let ClientMsg::Hello {
        room_id,
        token,
        client_id,
        name,
    } = serde_json::from_str(&text).ok()?
    else {
        return None;
    };
    let client_id_ok = !client_id.is_empty() && client_id.len() <= MAX_CLIENT_ID_LEN;
    Some(Hello {
        room_id,
        token,
        client_id: client_id_ok.then_some(client_id)?,
        name: clean_name(&name)?,
    })
}

async fn fatal(sink: &mut SplitSink<WebSocket, Message>, error: FatalError, message: &str) {
    let msg = ServerMsg::Fatal {
        error,
        message: message.to_owned(),
    };
    let _ = sink.send(Message::Text(msg.to_json().into())).await;
    let _ = sink.send(Message::Close(None)).await;
}

/// Forwards room messages and pongs to the socket and pings it. Ends when the room drops the outbox.
async fn write_loop(
    mut sink: SplitSink<WebSocket, Message>,
    mut inbox: mpsc::Receiver<Utf8Bytes>,
    mut pongs: mpsc::Receiver<Utf8Bytes>,
) {
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await;
    loop {
        let frame = tokio::select! {
            out = inbox.recv() => match out {
                Some(text) => Message::Text(text),
                None => break,
            },
            Some(pong) = pongs.recv() => Message::Text(pong),
            _ = ping.tick() => Message::Ping(Bytes::new()),
        };
        if sink.send(frame).await.is_err() {
            return;
        }
    }
    let _ = sink.send(Message::Close(None)).await;
}

/// Just enough of a `cmd` to ack it when the full message does not parse.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandStub {
    #[serde(rename = "type")]
    kind: String,
    command_id: String,
}

async fn read_loop(
    mut stream: SplitStream<WebSocket>,
    room: &RoomHandle,
    conn: ConnId,
    pongs: &mpsc::Sender<Utf8Bytes>,
) {
    let mut bucket = TokenBucket::new(RATE_BURST, RATE_PER_SEC);
    loop {
        // Any frame, Pong included, proves the peer is alive.
        let text = match tokio::time::timeout(SILENCE_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => text,
            Ok(Some(Ok(Message::Close(_)) | Err(_)) | None) | Err(_) => return,
            Ok(Some(Ok(_))) => continue,
        };
        let allowed = bucket.try_take(Instant::now());
        let input = match serde_json::from_str::<ClientMsg>(&text) {
            Ok(ClientMsg::Ping { t0 }) => {
                if allowed {
                    let pong = ServerMsg::Pong {
                        t0,
                        server_time: now_ms(),
                    };
                    let _ = pongs.try_send(Utf8Bytes::from(pong.to_json()));
                }
                continue;
            }
            Ok(ClientMsg::Hello { .. }) => continue,
            Ok(ClientMsg::Cmd(cmd)) if !allowed => Input::Rejected {
                conn,
                command_id: cmd.command_id,
                error: AckError::RateLimited,
            },
            Ok(_) if !allowed => continue,
            Ok(msg) => Input::Client { conn, msg },
            Err(e) => match serde_json::from_str::<CommandStub>(&text) {
                Ok(stub) if stub.kind == "cmd" && allowed => Input::Rejected {
                    conn,
                    command_id: stub.command_id,
                    error: AckError::BadArgs,
                },
                _ => {
                    debug!(conn, "ignoring malformed message: {e}");
                    continue;
                }
            },
        };
        if room.send(RoomMsg::Input(input)).await.is_err() {
            return;
        }
    }
}
