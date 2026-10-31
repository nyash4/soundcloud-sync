//! SQLite persistence. One connection lives on a dedicated thread; async callers send it closures.

use std::path::Path;
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use tokio::sync::oneshot;

use crate::room::Snapshot;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

#[derive(Clone)]
pub struct Store {
    jobs: mpsc::Sender<Job>,
}

pub struct StoredRoom {
    pub token_hash: [u8; 32],
    pub snapshot: Snapshot,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS rooms (
    id           TEXT PRIMARY KEY,
    token_sha256 BLOB NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    snapshot     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS rooms_updated_at ON rooms (updated_at);
";

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating database directory {}", dir.display()))?;
        }
        let mut conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;

        let (jobs, inbox) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("sqlite".into())
            .spawn(move || {
                for job in inbox {
                    job(&mut conn);
                }
            })?;
        Ok(Self { jobs })
    }

    /// Enqueues `f` immediately (so jobs run in call order) and returns a future for its result.
    fn call<T, F>(&self, f: F) -> impl Future<Output = anyhow::Result<T>> + use<T, F>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> anyhow::Result<T> + Send + 'static,
    {
        let (reply, result) = oneshot::channel();
        let queued = self.jobs.send(Box::new(move |conn| {
            let _ = reply.send(f(conn));
        }));
        async move {
            queued.map_err(|_| anyhow!("database thread is gone"))?;
            result
                .await
                .map_err(|_| anyhow!("database thread dropped the job"))?
        }
    }

    /// Inserts a new room; returns `false` if the id is already taken.
    pub async fn create_room(
        &self,
        id: String,
        token_hash: [u8; 32],
        now: i64,
    ) -> anyhow::Result<bool> {
        let snapshot = serde_json::to_string(&Snapshot::empty())?;
        self.call(move |conn| {
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO rooms (id, token_sha256, created_at, updated_at, snapshot)
                 VALUES (?1, ?2, ?3, ?3, ?4)",
                params![id, token_hash.as_slice(), now, snapshot],
            )?;
            Ok(inserted == 1)
        })
        .await
    }

    /// Loads a room and marks it as used, which keeps it away from the 30-day cleanup.
    pub async fn load_room(&self, id: String, now: i64) -> anyhow::Result<Option<StoredRoom>> {
        self.call(move |conn| {
            let row = conn
                .query_row(
                    "UPDATE rooms SET updated_at = ?2 WHERE id = ?1
                     RETURNING token_sha256, snapshot",
                    params![id, now],
                    |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let Some((hash, snapshot)) = row else {
                return Ok(None);
            };
            let token_hash = hash
                .try_into()
                .map_err(|_| anyhow!("room {id}: malformed token hash"))?;
            let snapshot = serde_json::from_str(&snapshot)
                .with_context(|| format!("room {id}: malformed snapshot"))?;
            Ok(Some(StoredRoom {
                token_hash,
                snapshot,
            }))
        })
        .await
    }

    /// Queues the write right away; awaiting the returned future is optional.
    pub fn save_snapshot(
        &self,
        id: String,
        snapshot: &Snapshot,
        now: i64,
    ) -> impl Future<Output = anyhow::Result<()>> + use<> {
        let json = serde_json::to_string(snapshot).expect("snapshots always serialize");
        self.call(move |conn| {
            conn.execute(
                "UPDATE rooms SET snapshot = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, json, now],
            )?;
            Ok(())
        })
    }

    /// Deletes rooms not used since `cutoff`; returns how many were removed.
    pub async fn delete_unused_since(&self, cutoff: i64) -> anyhow::Result<usize> {
        self.call(move |conn| {
            Ok(conn.execute("DELETE FROM rooms WHERE updated_at < ?1", params![cutoff])?)
        })
        .await
    }
}
