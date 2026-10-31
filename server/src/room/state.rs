use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::protocol::{Ack, Anchor, Entry, ImportResponse, OutReason, Phase};

pub type ConnId = u64;

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub ready_timeout_ms: i64,
    pub end_timeout_ms: i64,
    pub heartbeat_timeout_ms: i64,
    pub start_lead_ms: i64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            ready_timeout_ms: 20_000,
            end_timeout_ms: 30_000,
            heartbeat_timeout_ms: 15_000,
            start_lead_ms: 1_500,
        }
    }
}

/// A participant's relation to the current `playbackId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Nothing is playing.
    None,
    /// Member of the barrier: the room waits for this participant.
    Barrier { ready: bool, ended: bool },
    /// Temporarily excluded from the shared playback.
    Out(OutReason),
}

#[derive(Debug, Clone)]
pub struct Participant {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub conn: Option<ConnId>,
    /// Set while the socket is gone; drives the barrier grace period and eventual forgetting.
    pub offline_since: Option<i64>,
    pub audio_blocked: bool,
    pub role: Role,
}

impl Participant {
    pub fn online(&self) -> bool {
        self.conn.is_some()
    }

    /// Still counted as present: either connected or within the heartbeat grace period.
    pub fn present(&self, now: i64, timing: &Timing) -> bool {
        match self.offline_since {
            None => true,
            Some(since) => now - since < timing.heartbeat_timeout_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Playback {
    pub phase: Phase,
    pub paused: bool,
    pub playback_id: Option<String>,
    pub start_ms: i64,
    pub start_at: Option<i64>,
    pub anchor: Option<Anchor>,
    pub ready_deadline: Option<i64>,
    pub end_deadline: Option<i64>,
    /// Remaining end-wait while paused in `waiting_end`.
    pub end_remaining: Option<i64>,
    /// Barrier members that reported a player error for this playback.
    pub errors: u32,
}

impl Playback {
    pub fn idle() -> Self {
        Self {
            phase: Phase::Idle,
            paused: false,
            playback_id: None,
            start_ms: 0,
            start_at: None,
            anchor: None,
            ready_deadline: None,
            end_deadline: None,
            end_remaining: None,
            errors: 0,
        }
    }

    pub fn is_current(&self, playback_id: &str) -> bool {
        self.playback_id.as_deref() == Some(playback_id)
    }

    /// Track position at `now` according to the anchor.
    pub fn position_at(&self, now: i64) -> i64 {
        match self.anchor {
            Some(a) if !self.paused => a.ms + (now - a.at).max(0),
            Some(a) => a.ms,
            None => self.start_ms,
        }
    }
}

/// Bounded FIFO memory of processed command ids and their results.
#[derive(Debug)]
pub struct ResultCache<T> {
    cap: usize,
    order: VecDeque<String>,
    map: HashMap<String, T>,
}

impl<T: Clone> ResultCache<T> {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            order: VecDeque::with_capacity(cap),
            map: HashMap::with_capacity(cap),
        }
    }

    pub fn get(&self, id: &str) -> Option<T> {
        self.map.get(id).cloned()
    }

    pub fn insert(&mut self, id: String, value: T) {
        if self.map.insert(id.clone(), value).is_some() {
            return;
        }
        self.order.push_back(id);
        if self.order.len() > self.cap
            && let Some(old) = self.order.pop_front()
        {
            self.map.remove(&old);
        }
    }
}

pub type AckCache = ResultCache<Ack>;
pub type ImportCache = ResultCache<ImportResponse>;

/// What survives a restart (PROTOCOL §4.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub revision: u64,
    pub queue_revision: u64,
    pub queue: Vec<Entry>,
    pub current: Option<Entry>,
    pub history: Vec<Entry>,
    pub playback_counter: u64,
    pub position_ms: Option<i64>,
    pub paused: bool,
}

impl Snapshot {
    pub fn empty() -> Self {
        Self {
            revision: 0,
            queue_revision: 0,
            queue: Vec::new(),
            current: None,
            history: Vec::new(),
            playback_counter: 0,
            position_ms: None,
            paused: false,
        }
    }
}
