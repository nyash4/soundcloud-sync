//! Wire types shared by the WebSocket and HTTP layers (docs/PROTOCOL.md §2–3).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    Album,
    Playlist,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub title: String,
    pub kind: CollectionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub track_id: u64,
    pub widget_url: String,
    pub permalink: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u64>,
    pub artwork: Option<String>,
    pub collection: Option<Collection>,
    pub added_by: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Preparing,
    Playing,
    WaitingEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Listening,
    Paused,
    Loading,
    Waiting,
    Offline,
    Out,
    Blocked,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutReason {
    JoinedLate,
    ReadyTimeout,
    EndTimeout,
    Unavailable,
    LoadFailed,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Anchor {
    pub at: i64,
    pub ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantView {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub status: Status,
    pub out_reason: Option<OutReason>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackView {
    pub phase: Phase,
    pub paused: bool,
    pub playback_id: Option<String>,
    pub start_ms: i64,
    pub start_at: Option<i64>,
    pub anchor: Option<Anchor>,
    pub ready_deadline: Option<i64>,
    pub end_deadline: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomState {
    pub room_id: String,
    pub revision: u64,
    pub server_time: i64,
    pub participants: Vec<ParticipantView>,
    pub current: Option<Entry>,
    pub queue: Vec<Entry>,
    pub history: Vec<Entry>,
    pub playback: PlaybackView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerErrorCode {
    Unavailable,
    LoadFailed,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ClientMsg {
    Hello {
        room_id: String,
        token: String,
        client_id: String,
        name: String,
    },
    Ping {
        t0: serde_json::Number,
    },
    Cmd(Command),
    Ready {
        playback_id: String,
    },
    Ended {
        playback_id: String,
    },
    Error {
        playback_id: String,
        code: PlayerErrorCode,
    },
    Join {
        playback_id: String,
    },
    Position {
        playback_id: String,
        ms: f64,
        #[serde(rename = "playing")]
        _playing: bool,
    },
    Audio {
        blocked: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    pub command_id: String,
    pub name: String,
    #[serde(default)]
    pub args: Value,
    pub revision: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AckError {
    Conflict,
    NotFound,
    BadArgs,
    RateLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FatalError {
    BadToken,
    RoomNotFound,
    BadHello,
    SessionReplaced,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ServerMsg {
    Welcome {
        self_id: String,
        state: Box<RoomState>,
        server_time: i64,
    },
    Pong {
        t0: serde_json::Number,
        server_time: i64,
    },
    Ack(Ack),
    State {
        state: Box<RoomState>,
    },
    Notice {
        level: NoticeLevel,
        text: String,
        at: i64,
    },
    Fatal {
        error: FatalError,
        message: String,
    },
}

/// Flattened into `ServerMsg::Ack` so that `ok: true` acks carry `revision`
/// and failed ones carry `error`/`message`/`state`, exactly as the protocol shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    pub command_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AckError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Box<RoomState>>,
}

impl Ack {
    pub fn ok(command_id: String, revision: u64) -> Self {
        Self {
            command_id,
            ok: true,
            revision: Some(revision),
            error: None,
            message: None,
            state: None,
        }
    }

    pub fn err(command_id: String, error: AckError, message: &str, state: RoomState) -> Self {
        Self {
            command_id,
            ok: false,
            revision: None,
            error: Some(error),
            message: Some(message.to_owned()),
            state: Some(Box::new(state)),
        }
    }
}

impl ServerMsg {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("server messages always serialize")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    NotEmbeddable,
    Blocked,
    PreviewOnly,
    NotStreamable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unavailable {
    pub title: Option<String>,
    pub reason: UnavailableReason,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResponse {
    pub added: usize,
    pub collection: Option<Collection>,
    pub unavailable: Vec<Unavailable>,
    pub missing: usize,
    pub revision: u64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub needs_client_expansion: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_url: Option<String>,
}

/// Track metadata resolved from SoundCloud, before it becomes a queue entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub track_id: u64,
    pub widget_url: String,
    pub permalink: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u64>,
    pub artwork: Option<String>,
}
