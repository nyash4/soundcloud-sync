//! One tokio task per room owns the [`Room`] and processes its inputs strictly in order.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::bail;
use axum::extract::ws::Utf8Bytes;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use super::{ConnId, ImportBatch, Input, Output, QueueFull, Room, Timing};
use crate::now_ms;
use crate::protocol::ImportResponse;
use crate::store::Store;

const PERSIST_DEBOUNCE_MS: i64 = 300;
const POSITION_PERSIST_INTERVAL_MS: i64 = 5_000;
/// An actor with no sockets attached persists and exits after this long.
const IDLE_SHUTDOWN_MS: i64 = 5 * 60_000;
const INBOX_SIZE: usize = 256;
const SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ImportLookup {
    /// The response of an import already applied under the same `commandId`.
    pub previous: Option<ImportResponse>,
    pub revision: u64,
}

pub enum RoomMsg {
    Attach {
        conn: ConnId,
        client_id: String,
        name: String,
        /// The actor holds the only sender: dropping it closes the socket.
        outbox: mpsc::Sender<Utf8Bytes>,
    },
    Input(Input),
    ImportResult {
        command_id: String,
        reply: oneshot::Sender<ImportLookup>,
    },
    Import {
        batch: ImportBatch,
        reply: oneshot::Sender<Result<ImportResponse, QueueFull>>,
    },
    Flush {
        reply: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
pub struct RoomHandle {
    tx: mpsc::Sender<RoomMsg>,
    pub token_hash: [u8; 32],
}

/// The room actor has shut down; callers fetch a fresh handle from [`Rooms::get`].
#[derive(Debug)]
pub struct RoomGone;

impl RoomHandle {
    pub async fn send(&self, msg: RoomMsg) -> Result<(), RoomGone> {
        self.tx.send(msg).await.map_err(|_| RoomGone)
    }
}

/// Registry of live room actors; rooms are loaded lazily from the store.
#[derive(Clone)]
pub struct Rooms {
    inner: Arc<Inner>,
}

struct Inner {
    live: Mutex<HashMap<String, RoomHandle>>,
    store: Store,
    timing: Timing,
}

impl Rooms {
    pub fn new(store: Store, timing: Timing) -> Self {
        Self {
            inner: Arc::new(Inner {
                live: Mutex::new(HashMap::new()),
                store,
                timing,
            }),
        }
    }

    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, RoomHandle>> {
        self.inner.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Returns the running actor for `room_id`, starting it from the store if needed.
    pub async fn get(&self, room_id: &str) -> anyhow::Result<Option<RoomHandle>> {
        if let Some(handle) = self.live().get(room_id).filter(|h| !h.tx.is_closed()) {
            return Ok(Some(handle.clone()));
        }
        let Some(stored) = self
            .inner
            .store
            .load_room(room_id.to_owned(), now_ms())
            .await?
        else {
            return Ok(None);
        };

        let mut live = self.live();
        // Another caller may have started the actor while we were reading the store.
        if let Some(handle) = live.get(room_id).filter(|h| !h.tx.is_closed()) {
            return Ok(Some(handle.clone()));
        }
        let (tx, rx) = mpsc::channel(INBOX_SIZE);
        let handle = RoomHandle {
            tx,
            token_hash: stored.token_hash,
        };
        live.insert(room_id.to_owned(), handle.clone());
        let room = Room::restore(room_id.to_owned(), self.inner.timing, stored.snapshot);
        let actor = Actor {
            room,
            rx,
            conns: HashMap::new(),
            rooms: self.clone(),
            persist_at: Some(now_ms()),
            last_saved: 0,
            empty_since: Some(now_ms()),
        };
        debug!(room = room_id, "room loaded");
        tokio::spawn(actor.run());
        Ok(Some(handle))
    }

    /// Sends a request to the room and waits for its reply, retrying if the actor was just retiring.
    pub async fn request<T>(
        &self,
        room_id: &str,
        make: impl Fn(oneshot::Sender<T>) -> RoomMsg,
    ) -> anyhow::Result<Option<T>> {
        for _ in 0..3 {
            let Some(handle) = self.get(room_id).await? else {
                return Ok(None);
            };
            let (reply, result) = oneshot::channel();
            if handle.send(make(reply)).await.is_err() {
                continue;
            }
            if let Ok(value) = result.await {
                return Ok(Some(value));
            }
        }
        bail!("room {room_id} actor is not responding")
    }

    /// Persists every live room; used on shutdown.
    pub async fn flush_all(&self) {
        let handles: Vec<_> = self.live().values().cloned().collect();
        let flushes = handles.into_iter().map(|h| async move {
            let (reply, done) = oneshot::channel();
            if h.send(RoomMsg::Flush { reply }).await.is_ok() {
                let _ = done.await;
            }
        });
        let all = futures_util::future::join_all(flushes);
        if tokio::time::timeout(SHUTDOWN_FLUSH_TIMEOUT, all)
            .await
            .is_err()
        {
            warn!("timed out flushing rooms on shutdown");
        }
    }
}

struct Actor {
    room: Room,
    rx: mpsc::Receiver<RoomMsg>,
    conns: HashMap<ConnId, mpsc::Sender<Utf8Bytes>>,
    rooms: Rooms,
    persist_at: Option<i64>,
    last_saved: i64,
    empty_since: Option<i64>,
}

impl Actor {
    async fn run(mut self) {
        loop {
            let wake = [
                self.room.next_deadline(),
                self.persist_at,
                self.empty_since.map(|t| t + IDLE_SHUTDOWN_MS),
            ]
            .into_iter()
            .flatten()
            .min();
            let sleep = async {
                match wake {
                    Some(at) => {
                        let ms = (at - now_ms()).max(0) as u64;
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                    }
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                msg = self.rx.recv() => match msg {
                    Some(msg) => self.on_msg(msg).await,
                    None => break,
                },
                () = sleep => {
                    let now = now_ms();
                    let outputs = self.room.handle(Input::Tick, now);
                    self.apply(outputs, now);
                    if self.persist_at.is_some_and(|t| t <= now) {
                        self.persist(now);
                    }
                    if self.empty_since.is_some_and(|t| now - t >= IDLE_SHUTDOWN_MS)
                        && self.retire().await
                    {
                        break;
                    }
                }
            }
        }
        debug!(room = self.room.id(), "room actor stopped");
    }

    async fn on_msg(&mut self, msg: RoomMsg) {
        let now = now_ms();
        match msg {
            RoomMsg::Attach {
                conn,
                client_id,
                name,
                outbox,
            } => {
                self.conns.insert(conn, outbox);
                self.empty_since = None;
                let outputs = self.room.handle(
                    Input::Connect {
                        conn,
                        client_id,
                        name,
                    },
                    now,
                );
                self.apply(outputs, now);
            }
            RoomMsg::Input(input) => {
                if let Input::Disconnect { conn } = input {
                    self.drop_conn(conn, now);
                }
                let outputs = self.room.handle(input, now);
                self.apply(outputs, now);
            }
            RoomMsg::ImportResult { command_id, reply } => {
                let _ = reply.send(ImportLookup {
                    previous: self.room.import_result(&command_id),
                    revision: self.room.revision(),
                });
            }
            RoomMsg::Import { batch, reply } => {
                let (result, outputs) = self.room.import(batch, now);
                self.apply(outputs, now);
                let _ = reply.send(result);
            }
            RoomMsg::Flush { reply } => {
                let _ = self.save(now).await;
                let _ = reply.send(());
            }
        }
    }

    fn apply(&mut self, outputs: Vec<Output>, now: i64) {
        for output in outputs {
            match output {
                Output::Send(conn, msg) => self.deliver(conn, msg.to_json().into()),
                Output::Broadcast(msg) => {
                    let text: Utf8Bytes = msg.to_json().into();
                    let conns: Vec<ConnId> = self.conns.keys().copied().collect();
                    for conn in conns {
                        self.deliver(conn, text.clone());
                    }
                }
                Output::Close(conn) => {
                    self.conns.remove(&conn);
                }
                Output::Persist => self.schedule_persist(now + PERSIST_DEBOUNCE_MS),
                Output::PersistPosition => {
                    self.schedule_persist(now.max(self.last_saved + POSITION_PERSIST_INTERVAL_MS))
                }
            }
        }
    }

    fn deliver(&mut self, conn: ConnId, text: Utf8Bytes) {
        let Some(outbox) = self.conns.get(&conn) else {
            return;
        };
        match outbox.try_send(text) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // A socket that cannot keep up is closed; the client reconnects and gets a fresh snapshot.
                warn!(
                    room = self.room.id(),
                    conn, "outbox full, closing connection"
                );
                self.conns.remove(&conn);
            }
            // The socket is already gone and its Disconnect is on the way.
            Err(TrySendError::Closed(_)) => {
                self.conns.remove(&conn);
            }
        }
    }

    fn drop_conn(&mut self, conn: ConnId, now: i64) {
        self.conns.remove(&conn);
        if self.conns.is_empty() {
            self.empty_since = Some(now);
        }
    }

    fn schedule_persist(&mut self, at: i64) {
        self.persist_at = Some(self.persist_at.map_or(at, |t| t.min(at)));
    }

    /// Fire-and-forget save; the store runs writes in order, so a later one always wins.
    fn persist(&mut self, now: i64) {
        let write = self.start_save(now);
        let room = self.room.id().to_owned();
        tokio::spawn(async move {
            if let Err(e) = write.await {
                warn!(room, "saving snapshot failed: {e:#}");
            }
        });
    }

    async fn save(&mut self, now: i64) -> anyhow::Result<()> {
        self.start_save(now).await
    }

    fn start_save(&mut self, now: i64) -> impl Future<Output = anyhow::Result<()>> + use<> {
        self.persist_at = None;
        self.last_saved = now;
        let snapshot = self.room.snapshot(now);
        self.rooms
            .inner
            .store
            .save_snapshot(self.room.id().to_owned(), &snapshot, now)
    }

    /// Persists and unregisters the room if nothing arrived meanwhile. Unregistering and closing
    /// the inbox happen under the registry lock, so no caller can grab a handle that goes stale.
    async fn retire(&mut self) -> bool {
        if let Err(e) = self.save(now_ms()).await {
            warn!(
                room = self.room.id(),
                "final save failed, keeping room alive: {e:#}"
            );
            self.empty_since = Some(now_ms());
            return false;
        }
        let mut live = self.rooms.live();
        if !self.rx.is_empty() {
            return false;
        }
        live.remove(self.room.id());
        self.rx.close();
        info!(room = self.room.id(), "room unloaded");
        true
    }
}
