//! `cmd` handling with idempotency, queue edits and imports.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{ConnId, MAX_QUEUE_LEN, Output, Room, clean_name, tracks};
use crate::ids;
use crate::protocol::{
    Ack, AckError, Collection, CollectionKind, Command, Entry, ImportResponse, NoticeLevel,
    ServerMsg, TrackInfo, Unavailable,
};

/// A resolved import, ready to be appended to the queue as one operation.
#[derive(Debug, Clone)]
pub struct ImportBatch {
    pub command_id: String,
    pub client_id: String,
    pub collection: Option<Collection>,
    pub tracks: Vec<TrackInfo>,
    pub unavailable: Vec<Unavailable>,
    pub missing: usize,
}

/// The import would push the queue past its size limit.
#[derive(Debug)]
pub struct QueueFull;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveArgs {
    entry_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveArgs {
    entry_id: String,
    after_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NextArgs {
    #[serde(default)]
    from_entry_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeekArgs {
    playback_id: String,
    ms: i64,
}

#[derive(Deserialize)]
struct RenameArgs {
    name: String,
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, AckError> {
    serde_json::from_value(args).map_err(|_| AckError::BadArgs)
}

fn error_message(error: AckError) -> &'static str {
    match error {
        AckError::Conflict => "Очередь уже изменилась — показываем актуальную",
        AckError::NotFound => "Этой позиции уже нет в очереди",
        AckError::BadArgs => "Некорректная команда",
        AckError::RateLimited => "Слишком много действий подряд — подождите немного",
    }
}

impl Room {
    pub(super) fn on_command(&mut self, conn: ConnId, idx: usize, cmd: Command, now: i64) {
        let Command {
            command_id,
            name,
            args,
            revision,
        } = cmd;
        if let Some(ack) = self.acks.get(&command_id) {
            self.send(conn, ServerMsg::Ack(ack));
            return;
        }
        let result = match name.as_str() {
            "queue.remove" => parse(args).and_then(|a: RemoveArgs| self.queue_remove(&a.entry_id)),
            "queue.move" => parse(args).and_then(|a: MoveArgs| {
                let revision = revision.ok_or(AckError::BadArgs)?;
                self.queue_move(&a.entry_id, a.after_id.as_deref(), revision)
            }),
            "play" => {
                self.play(now);
                Ok(())
            }
            "pause" => {
                self.pause(now);
                Ok(())
            }
            "seek" => parse(args).and_then(|a: SeekArgs| self.seek(&a.playback_id, a.ms, now)),
            "next" => {
                // Absent args mean «skip whatever is current».
                let args = if args.is_null() {
                    Value::Object(Default::default())
                } else {
                    args
                };
                parse(args).map(|a: NextArgs| self.next(idx, a.from_entry_id.as_deref(), now))
            }
            "rename" => parse(args).and_then(|a: RenameArgs| {
                let name = clean_name(&a.name).ok_or(AckError::BadArgs)?;
                if self.participants[idx].name != name {
                    self.participants[idx].name = name;
                    self.touch();
                }
                Ok(())
            }),
            _ => Err(AckError::BadArgs),
        };
        // Commands can trigger transitions (e.g. `play` with everyone ready); settle them before acking.
        self.reevaluate(now);
        let ack = match result {
            Ok(()) => Ack::ok(command_id.clone(), self.revision),
            Err(e) => Ack::err(command_id.clone(), e, error_message(e), self.view(now)),
        };
        self.acks.insert(command_id, ack.clone());
        self.send(conn, ServerMsg::Ack(ack));
    }

    pub(super) fn on_rejected(
        &mut self,
        conn: ConnId,
        command_id: String,
        error: AckError,
        now: i64,
    ) {
        let ack = Ack::err(command_id, error, error_message(error), self.view(now));
        self.send(conn, ServerMsg::Ack(ack));
    }

    fn queue_remove(&mut self, entry_id: &str) -> Result<(), AckError> {
        let idx = self.queue_index(entry_id)?;
        self.queue.remove(idx);
        self.touch_queue();
        Ok(())
    }

    fn queue_move(
        &mut self,
        entry_id: &str,
        after_id: Option<&str>,
        revision: u64,
    ) -> Result<(), AckError> {
        if revision < self.queue_revision {
            return Err(AckError::Conflict);
        }
        let from = self.queue_index(entry_id)?;
        if after_id == Some(entry_id) {
            return Err(AckError::BadArgs);
        }
        if let Some(after) = after_id {
            self.queue_index(after)?;
        }
        let entry = self.queue.remove(from);
        let to = match after_id {
            None => 0,
            Some(after) => self.queue_index(after)? + 1,
        };
        self.queue.insert(to, entry);
        if to != from {
            self.touch_queue();
        }
        Ok(())
    }

    fn queue_index(&self, entry_id: &str) -> Result<usize, AckError> {
        self.queue
            .iter()
            .position(|e| e.id == entry_id)
            .ok_or(AckError::NotFound)
    }

    /// Result of an import already applied under this `commandId`, if any.
    pub fn import_result(&self, command_id: &str) -> Option<ImportResponse> {
        self.imports.get(command_id)
    }

    /// Appends a resolved import to the queue as a single revision.
    pub fn import(
        &mut self,
        batch: ImportBatch,
        now: i64,
    ) -> (Result<ImportResponse, QueueFull>, Vec<Output>) {
        if let Some(previous) = self.imports.get(&batch.command_id) {
            return (Ok(previous), Vec::new());
        }
        if self.queue.len() + batch.tracks.len() > MAX_QUEUE_LEN {
            return (Err(QueueFull), Vec::new());
        }
        let added_by = self
            .participants
            .iter()
            .find(|p| p.client_id == batch.client_id)
            .map_or_else(|| "Гость".to_owned(), |p| p.name.clone());
        let added = batch.tracks.len();
        let first_title = batch.tracks.first().map(|t| t.title.clone());
        self.queue.extend(batch.tracks.into_iter().map(|t| Entry {
            id: ids::entry_id(),
            track_id: t.track_id,
            widget_url: t.widget_url,
            permalink: t.permalink,
            title: t.title,
            artist: t.artist,
            duration_ms: t.duration_ms,
            artwork: t.artwork,
            collection: batch.collection.clone(),
            added_by: added_by.clone(),
        }));
        self.touch_queue();

        let mut text = match (&batch.collection, first_title) {
            (Some(c), _) => {
                let kind = match c.kind {
                    CollectionKind::Album => "альбом",
                    CollectionKind::Playlist => "плейлист",
                };
                format!(
                    "{added_by} добавляет {kind} «{}» — {}",
                    c.title,
                    tracks(added)
                )
            }
            (None, Some(title)) => format!("{added_by} добавляет «{title}»"),
            (None, None) => format!("{added_by} добавляет {}", tracks(added)),
        };
        let skipped = batch.unavailable.len() + batch.missing;
        if skipped > 0 {
            text.push_str(&format!(", недоступно: {skipped}"));
        }
        self.notice(NoticeLevel::Info, text, now);

        let response = ImportResponse {
            added,
            collection: batch.collection,
            unavailable: batch.unavailable,
            missing: batch.missing,
            revision: self.revision,
            needs_client_expansion: false,
            playlist_url: None,
        };
        self.imports.insert(batch.command_id, response.clone());
        (Ok(response), self.finish(now))
    }
}
