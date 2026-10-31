//! Room logic as a pure, synchronous state machine. Time is injected (`now` in server epoch ms)
//! and every side effect is returned as an [`Output`], so the whole thing is unit-testable.

pub mod actor;
mod commands;
mod playback;
mod state;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;

use crate::protocol::{
    AckError, ClientMsg, Entry, FatalError, NoticeLevel, OutReason, ParticipantView, Phase,
    PlaybackView, RoomState, ServerMsg, Status,
};
pub use commands::{ImportBatch, QueueFull};
use state::{AckCache, ImportCache, Participant, Playback, Role};
pub use state::{ConnId, Snapshot, Timing};

const HISTORY_LIMIT: usize = 30;
const MAX_QUEUE_LEN: usize = 2_000;
const ACK_CACHE_SIZE: usize = 256;
const IMPORT_CACHE_SIZE: usize = 64;
/// Disconnected participants disappear from the list after this long.
const FORGET_OFFLINE_MS: i64 = 10 * 60_000;

#[derive(Debug)]
pub enum Input {
    /// A socket passed `hello` and token checks.
    Connect {
        conn: ConnId,
        client_id: String,
        name: String,
    },
    Disconnect {
        conn: ConnId,
    },
    Client {
        conn: ConnId,
        msg: ClientMsg,
    },
    /// A command refused before reaching the room (rate limit, malformed); it still gets an ack.
    Rejected {
        conn: ConnId,
        command_id: String,
        error: AckError,
    },
    Tick,
}

#[derive(Debug, Clone)]
pub enum Output {
    Send(ConnId, ServerMsg),
    Broadcast(ServerMsg),
    /// Close a socket that was taken over by a newer connection of the same client.
    Close(ConnId),
    /// Durable state changed; persist soon.
    Persist,
    /// Only the playback position changed; persist at a throttled rate.
    PersistPosition,
}

#[derive(Default)]
struct Effects {
    out: Vec<Output>,
    notices: Vec<ServerMsg>,
    changed: bool,
    position: bool,
}

pub struct Room {
    id: String,
    timing: Timing,
    revision: u64,
    /// Revision of the last queue mutation; `queue.move` conflicts only if the client is older.
    queue_revision: u64,
    queue: Vec<Entry>,
    current: Option<Entry>,
    history: VecDeque<Entry>,
    playback_counter: u64,
    pb: Playback,
    /// Last reported position of `current` for the current playback.
    position_ms: Option<i64>,
    participants: Vec<Participant>,
    acks: AckCache,
    imports: ImportCache,
    fx: Effects,
}

/// Trims and validates a display name (1–32 characters, no control characters).
pub fn clean_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    let len = name.chars().count();
    ((1..=32).contains(&len) && !name.chars().any(char::is_control)).then(|| name.to_owned())
}

impl Room {
    /// Restores a persisted room. A track that was current comes back paused in `preparing`,
    /// positioned where it was last heard, so music never resumes on its own after a restart.
    pub fn restore(id: String, timing: Timing, snap: Snapshot) -> Self {
        let mut room = Self {
            id,
            timing,
            revision: snap.revision,
            queue_revision: snap.queue_revision,
            queue: snap.queue,
            current: None,
            history: snap.history.into(),
            playback_counter: snap.playback_counter,
            pb: Playback::idle(),
            position_ms: None,
            participants: Vec::new(),
            acks: AckCache::new(ACK_CACHE_SIZE),
            imports: ImportCache::new(IMPORT_CACHE_SIZE),
            fx: Effects::default(),
        };
        if let Some(entry) = snap.current {
            // The paused flag must be set before preparing so the ready timer stays off.
            room.pb.paused = true;
            room.prepare(entry, 0);
            room.pb.start_ms = snap.position_ms.unwrap_or(0).max(0);
        }
        // Clients may have seen a revision that was not flushed before the restart.
        room.revision = snap.revision + 1;
        room.fx = Effects::default();
        room
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn snapshot(&self, now: i64) -> Snapshot {
        let position_ms = self.current.as_ref().map(|_| {
            self.position_ms.unwrap_or_else(|| match self.pb.phase {
                Phase::Playing | Phase::WaitingEnd => self.pb.position_at(now),
                _ => self.pb.start_ms,
            })
        });
        Snapshot {
            revision: self.revision,
            queue_revision: self.queue_revision,
            queue: self.queue.clone(),
            current: self.current.clone(),
            history: self.history.iter().cloned().collect(),
            playback_counter: self.playback_counter,
            position_ms,
            paused: self.pb.paused,
        }
    }

    pub fn handle(&mut self, input: Input, now: i64) -> Vec<Output> {
        match input {
            Input::Connect {
                conn,
                client_id,
                name,
            } => self.on_connect(conn, client_id, name, now),
            Input::Disconnect { conn } => self.on_disconnect(conn, now),
            Input::Client { conn, msg } => self.on_client(conn, msg, now),
            Input::Rejected {
                conn,
                command_id,
                error,
            } => self.on_rejected(conn, command_id, error, now),
            Input::Tick => self.on_tick(now),
        }
        self.reevaluate(now);
        self.finish(now)
    }

    /// Earliest moment at which [`Input::Tick`] has something to do.
    pub fn next_deadline(&self) -> Option<i64> {
        let playback = match self.pb.phase {
            _ if self.pb.paused => None,
            Phase::Preparing => self.pb.ready_deadline,
            Phase::WaitingEnd => self.pb.end_deadline,
            _ => None,
        };
        let heartbeat = self.timing.heartbeat_timeout_ms;
        let participants = self.participants.iter().filter_map(|p| {
            let since = p.offline_since?;
            let forget = since + FORGET_OFFLINE_MS;
            Some(match p.role {
                Role::Barrier { .. } => forget.min(since + heartbeat),
                _ => forget,
            })
        });
        playback.into_iter().chain(participants).min()
    }

    pub fn view(&self, now: i64) -> RoomState {
        RoomState {
            room_id: self.id.clone(),
            revision: self.revision,
            server_time: now,
            participants: self
                .participants
                .iter()
                .map(|p| self.participant_view(p))
                .collect(),
            current: self.current.clone(),
            queue: self.queue.clone(),
            history: self.history.iter().cloned().collect(),
            playback: PlaybackView {
                phase: self.pb.phase,
                paused: self.pb.paused,
                playback_id: self.pb.playback_id.clone(),
                start_ms: self.pb.start_ms,
                start_at: self.pb.start_at,
                anchor: self.pb.anchor,
                ready_deadline: self.pb.ready_deadline,
                end_deadline: self.pb.end_deadline,
            },
        }
    }

    fn participant_view(&self, p: &Participant) -> ParticipantView {
        let out_reason = match p.role {
            Role::Out(reason) => Some(reason),
            _ => None,
        };
        let status = if !p.online() {
            Status::Offline
        } else if self.pb.phase == Phase::Idle {
            Status::Idle
        } else if p.audio_blocked && !matches!(p.role, Role::Barrier { ended: true, .. }) {
            Status::Blocked
        } else {
            match p.role {
                Role::None => Status::Idle,
                Role::Out(_) => Status::Out,
                Role::Barrier { ready, ended } => match self.pb.phase {
                    Phase::Preparing if !ready => Status::Loading,
                    Phase::Preparing if self.pb.paused => Status::Paused,
                    Phase::Preparing => Status::Waiting,
                    _ if ended => Status::Waiting,
                    _ if self.pb.paused => Status::Paused,
                    _ => Status::Listening,
                },
            }
        };
        ParticipantView {
            id: p.id.clone(),
            name: p.name.clone(),
            online: p.online(),
            status,
            out_reason,
        }
    }

    fn on_connect(&mut self, conn: ConnId, client_id: String, name: String, now: i64) {
        let phase = self.pb.phase;
        let idx = match self
            .participants
            .iter()
            .position(|p| p.client_id == client_id)
        {
            Some(idx) => {
                let p = &mut self.participants[idx];
                if let Some(old) = p.conn.replace(conn) {
                    self.fx.out.push(Output::Send(old, ServerMsg::Fatal {
                        error: FatalError::SessionReplaced,
                        message: "Комната открыта в другой вкладке. Продолжайте слушать там или обновите эту страницу.".into(),
                    }));
                    self.fx.out.push(Output::Close(old));
                }
                p.name = name;
                p.offline_since = None;
                p.audio_blocked = false;
                // Someone who dropped out while the next track was loading simply joins its barrier.
                if phase == Phase::Preparing && p.role == Role::Out(OutReason::Offline) {
                    p.role = Role::Barrier {
                        ready: false,
                        ended: false,
                    };
                }
                idx
            }
            None => {
                let role = match phase {
                    Phase::Idle => Role::None,
                    Phase::Preparing => Role::Barrier {
                        ready: false,
                        ended: false,
                    },
                    Phase::Playing | Phase::WaitingEnd => Role::Out(OutReason::JoinedLate),
                };
                self.participants.push(Participant {
                    id: crate::ids::participant_id(),
                    client_id,
                    name,
                    conn: Some(conn),
                    offline_since: None,
                    audio_blocked: false,
                    role,
                });
                self.participants.len() - 1
            }
        };
        self.touch();
        let welcome = ServerMsg::Welcome {
            self_id: self.participants[idx].id.clone(),
            state: Box::new(self.view(now)),
            server_time: now,
        };
        self.fx.out.push(Output::Send(conn, welcome));
    }

    fn on_disconnect(&mut self, conn: ConnId, now: i64) {
        // A socket that was already taken over is not the participant's live connection anymore.
        if let Some(p) = self.participants.iter_mut().find(|p| p.conn == Some(conn)) {
            p.conn = None;
            p.offline_since = Some(now);
            self.touch();
        }
    }

    fn on_client(&mut self, conn: ConnId, msg: ClientMsg, now: i64) {
        let Some(idx) = self.participants.iter().position(|p| p.conn == Some(conn)) else {
            return;
        };
        match msg {
            ClientMsg::Cmd(cmd) => self.on_command(conn, idx, cmd, now),
            ClientMsg::Ready { playback_id } => self.on_ready(idx, &playback_id, now),
            ClientMsg::Ended { playback_id } => self.on_ended(idx, &playback_id, now),
            ClientMsg::Error { playback_id, code } => self.on_player_error(idx, &playback_id, code),
            ClientMsg::Join { playback_id } => self.on_join(idx, &playback_id),
            ClientMsg::Position {
                playback_id, ms, ..
            } => self.on_position(idx, &playback_id, ms),
            ClientMsg::Audio { blocked } => {
                let p = &mut self.participants[idx];
                if p.audio_blocked != blocked {
                    p.audio_blocked = blocked;
                    self.touch();
                }
            }
            ClientMsg::Hello { .. } | ClientMsg::Ping { .. } => {}
        }
    }

    fn on_tick(&mut self, now: i64) {
        let before = self.participants.len();
        self.participants.retain(|p| {
            p.offline_since
                .is_none_or(|since| now - since < FORGET_OFFLINE_MS)
        });
        if self.participants.len() != before {
            self.touch();
        }
        self.expire_offline(now);
        self.expire_deadlines(now);
    }

    /// Marks the state as changed. The revision is bumped once per handled input,
    /// so acks built later in the same step already carry the final revision.
    fn touch(&mut self) {
        if !self.fx.changed {
            self.fx.changed = true;
            self.revision += 1;
        }
    }

    fn touch_queue(&mut self) {
        self.touch();
        self.queue_revision = self.revision;
    }

    fn send(&mut self, conn: ConnId, msg: ServerMsg) {
        self.fx.out.push(Output::Send(conn, msg));
    }

    fn notice(&mut self, level: NoticeLevel, text: String, now: i64) {
        self.fx.notices.push(ServerMsg::Notice {
            level,
            text,
            at: now,
        });
    }

    fn name_of(&self, idx: usize) -> String {
        self.participants[idx].name.clone()
    }

    fn finish(&mut self, now: i64) -> Vec<Output> {
        let fx = std::mem::take(&mut self.fx);
        let mut out = fx.out;
        if fx.changed {
            out.push(Output::Broadcast(ServerMsg::State {
                state: Box::new(self.view(now)),
            }));
            out.push(Output::Persist);
        } else if fx.position {
            out.push(Output::PersistPosition);
        }
        out.extend(fx.notices.into_iter().map(Output::Broadcast));
        out
    }
}

/// Russian plural for «трек».
fn tracks(n: usize) -> String {
    let word = match (n % 10, n % 100) {
        (_, 11..=14) => "треков",
        (1, _) => "трек",
        (2..=4, _) => "трека",
        _ => "треков",
    };
    format!("{n} {word}")
}
