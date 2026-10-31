use serde_json::{Value, json};

use super::*;
use crate::protocol::{Ack, Anchor, Collection, CollectionKind, ImportResponse, TrackInfo};

const TIMING: Timing = Timing {
    ready_timeout_ms: 20_000,
    end_timeout_ms: 30_000,
    heartbeat_timeout_ms: 15_000,
    start_lead_ms: 1_500,
};

const A: ConnId = 1;
const B: ConnId = 2;
const C: ConnId = 3;

#[test]
fn replaced_session_is_told_to_stop_before_its_socket_closes() {
    let mut h = Harness::new();
    h.connect(A);
    let participant_id = h.state().participants[0].id.clone();
    h.input(Input::Connect {
        conn: B,
        client_id: format!("client-{A}"),
        name: name_of(A).into(),
    });
    let fatal = h
        .last
        .iter()
        .position(|out| {
            matches!(
                out,
                Output::Send(
                    A,
                    ServerMsg::Fatal {
                        error: FatalError::SessionReplaced,
                        ..
                    }
                )
            )
        })
        .expect("old session must stop reconnecting");
    let close = h
        .last
        .iter()
        .position(|out| matches!(out, Output::Close(A)))
        .expect("old socket must close");
    assert!(fatal < close);
    assert!(
        h.last
            .iter()
            .any(|out| matches!(out, Output::Send(B, ServerMsg::Welcome { .. })))
    );
    h.input(Input::Disconnect { conn: A });
    assert_eq!(h.state().participants.len(), 1);
    assert_eq!(h.state().participants[0].id, participant_id);
    assert!(h.state().participants[0].online);
}

struct Harness {
    room: Room,
    now: i64,
    commands: u32,
    /// Everything the room emitted during the last step.
    last: Vec<Output>,
}

fn name_of(conn: ConnId) -> &'static str {
    match conn {
        A => "Ника",
        B => "Лёша",
        C => "Саша",
        _ => "Гость",
    }
}

impl Harness {
    fn new() -> Self {
        Self::with_room(Room::restore(
            "Room000001".into(),
            TIMING,
            Snapshot::empty(),
        ))
    }

    fn with_room(room: Room) -> Self {
        Self {
            room,
            now: 1_700_000_000_000,
            commands: 0,
            last: Vec::new(),
        }
    }

    fn input(&mut self, input: Input) -> &[Output] {
        self.last = self.room.handle(input, self.now);
        &self.last
    }

    fn connect(&mut self, conn: ConnId) {
        self.connect_as(conn, conn);
    }

    /// Connects socket `conn` as the client identified by `client`.
    fn connect_as(&mut self, conn: ConnId, client: ConnId) {
        self.input(Input::Connect {
            conn,
            client_id: format!("client-{client}"),
            name: name_of(client).into(),
        });
    }

    fn disconnect(&mut self, conn: ConnId) {
        self.input(Input::Disconnect { conn });
    }

    fn send(&mut self, conn: ConnId, msg: Value) {
        let msg = serde_json::from_value(msg).expect("valid client message");
        self.input(Input::Client { conn, msg });
    }

    fn cmd_with_id(
        &mut self,
        conn: ConnId,
        command_id: &str,
        name: &str,
        args: Value,
        revision: Option<u64>,
    ) -> Ack {
        self.send(
            conn,
            json!({ "type": "cmd", "commandId": command_id, "name": name, "args": args, "revision": revision }),
        );
        self.last
            .iter()
            .find_map(|o| match o {
                Output::Send(c, ServerMsg::Ack(ack)) if *c == conn => Some(ack.clone()),
                _ => None,
            })
            .expect("command was acked")
    }

    fn cmd(&mut self, conn: ConnId, name: &str, args: Value) -> Ack {
        self.commands += 1;
        let id = format!("cmd-{}", self.commands);
        self.cmd_with_id(conn, &id, name, args, None)
    }

    fn pb_id(&self) -> String {
        self.room
            .pb
            .playback_id
            .clone()
            .expect("a playback is current")
    }

    fn ready(&mut self, conn: ConnId) {
        let id = self.pb_id();
        self.send(conn, json!({ "type": "ready", "playbackId": id }));
    }

    fn ended(&mut self, conn: ConnId) {
        let id = self.pb_id();
        self.send(conn, json!({ "type": "ended", "playbackId": id }));
    }

    fn error(&mut self, conn: ConnId) {
        let id = self.pb_id();
        self.send(
            conn,
            json!({ "type": "error", "playbackId": id, "code": "unavailable" }),
        );
    }

    /// Moves the clock forward, firing every deadline on the way like the actor would.
    fn advance(&mut self, ms: i64) {
        let target = self.now + ms;
        let mut outputs = Vec::new();
        while let Some(deadline) = self.room.next_deadline().filter(|&d| d <= target) {
            self.now = deadline.max(self.now);
            outputs.extend(self.room.handle(Input::Tick, self.now));
        }
        self.now = target;
        outputs.extend(self.room.handle(Input::Tick, self.now));
        self.last = outputs;
    }

    fn import(&mut self, titles: &[&str]) -> ImportResponse {
        self.commands += 1;
        let tracks = titles
            .iter()
            .enumerate()
            .map(|(i, title)| TrackInfo {
                track_id: 100 + i as u64,
                widget_url: format!("https://api.soundcloud.com/tracks/{}", 100 + i),
                permalink: format!("https://soundcloud.com/artist/{title}"),
                title: (*title).into(),
                artist: "Artist".into(),
                duration_ms: Some(200_000),
                artwork: None,
            })
            .collect();
        let batch = ImportBatch {
            command_id: format!("import-{}", self.commands),
            client_id: format!("client-{A}"),
            collection: None,
            tracks,
            unavailable: Vec::new(),
            missing: 0,
        };
        let (result, outputs) = self.room.import(batch, self.now);
        self.last = outputs;
        result.expect("queue has room")
    }

    fn state(&self) -> RoomState {
        self.room.view(self.now)
    }

    fn phase(&self) -> Phase {
        self.room.pb.phase
    }

    fn current_title(&self) -> Option<String> {
        self.room.current.as_ref().map(|e| e.title.clone())
    }

    fn status(&self, conn: ConnId) -> (Status, Option<OutReason>) {
        let name = name_of(conn);
        let view = self.state();
        let p = view
            .participants
            .iter()
            .find(|p| p.name == name)
            .expect("participant exists");
        (p.status, p.out_reason)
    }

    fn notices(&self) -> Vec<String> {
        self.last
            .iter()
            .filter_map(|o| match o {
                Output::Broadcast(ServerMsg::Notice { text, .. }) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Two participants connected, three tracks queued, `play` pressed, both ready.
    fn playing_pair() -> Self {
        let mut h = Self::new();
        h.connect(A);
        h.connect(B);
        h.import(&["one", "two", "three"]);
        h.cmd(A, "play", json!({}));
        h.ready(A);
        h.ready(B);
        assert_eq!(h.phase(), Phase::Playing);
        h
    }
}

#[test]
fn two_clients_start_together() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["one", "two"]);
    assert_eq!(h.phase(), Phase::Idle);

    let ack = h.cmd(A, "play", json!({}));
    assert!(ack.ok);
    assert_eq!(ack.revision, Some(h.room.revision()));
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Preparing);
    assert_eq!(pb.playback_id.as_deref(), Some("p1"));
    assert_eq!(pb.start_at, None);
    assert_eq!(pb.ready_deadline, Some(h.now + 20_000));
    assert_eq!(h.current_title().as_deref(), Some("one"));
    assert_eq!(h.state().queue.len(), 1);
    assert_eq!(h.status(A), (Status::Loading, None));

    h.ready(A);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.status(A), (Status::Waiting, None));
    assert_eq!(h.status(B), (Status::Loading, None));

    h.advance(800);
    h.ready(B);
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Playing);
    assert_eq!(pb.start_at, Some(h.now + 1_500));
    assert_eq!(
        pb.anchor,
        Some(Anchor {
            at: h.now + 1_500,
            ms: 0
        })
    );
    assert_eq!(pb.ready_deadline, None);
    assert_eq!(h.status(A), (Status::Listening, None));
    assert!(
        h.last
            .iter()
            .any(|o| matches!(o, Output::Broadcast(ServerMsg::State { .. })))
    );
}

#[test]
fn seek_repositions_everyone_and_rejects_old_playback_reports() {
    let mut h = Harness::playing_pair();
    let old = h.pb_id();
    let queue = h.state().queue;
    assert!(
        h.cmd(A, "seek", json!({"playbackId": old, "ms": 90_000}))
            .ok
    );
    assert_eq!(h.phase(), Phase::Preparing);
    assert_ne!(h.pb_id(), old);
    assert_eq!(h.state().playback.start_ms, 90_000);
    assert_eq!(h.state().queue, queue);
    assert!(h.state().history.is_empty());
    h.send(A, json!({"type": "ready", "playbackId": old}));
    h.send(B, json!({"type": "ended", "playbackId": old}));
    assert_eq!(h.phase(), Phase::Preparing);
    h.ready(A);
    assert_eq!(h.phase(), Phase::Preparing);
    h.ready(B);
    assert_eq!(h.phase(), Phase::Playing);
    assert_eq!(h.state().playback.anchor.unwrap().ms, 90_000);
    let fresh = h.pb_id();
    assert!(
        h.cmd(A, "seek", json!({"playbackId": old, "ms": 15_000}))
            .ok
    );
    assert_eq!(h.pb_id(), fresh);
    assert!(
        h.cmd(B, "seek", json!({"playbackId": fresh, "ms": 5000}))
            .ok
    );
    assert_eq!(h.state().playback.start_ms, 5000);
}

#[test]
fn seeking_on_pause_keeps_pause_and_clamps_to_duration() {
    let mut h = Harness::playing_pair();
    h.cmd(A, "pause", json!({}));
    let old = h.pb_id();
    assert!(
        h.cmd(A, "seek", json!({"playbackId": old, "ms": 999_999}))
            .ok
    );
    assert_eq!(h.state().playback.start_ms, 199_999);
    assert!(h.state().playback.paused);
    h.ready(A);
    h.ready(B);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.state().playback.start_at, None);
    h.cmd(A, "play", json!({}));
    assert_eq!(h.state().playback.anchor.unwrap().ms, 199_999);
}

#[test]
fn seek_from_waiting_end_clears_end_barrier_and_is_idempotent() {
    let mut h = Harness::playing_pair();
    h.ended(A);
    assert_eq!(h.phase(), Phase::WaitingEnd);
    let args = json!({"playbackId": h.pb_id(), "ms": 0});
    assert!(h.cmd_with_id(B, "seek-once", "seek", args.clone(), None).ok);
    let playback = h.pb_id();
    assert!(h.cmd_with_id(B, "seek-once", "seek", args, None).ok);
    assert_eq!(h.pb_id(), playback);
    assert_eq!(h.state().playback.end_deadline, None);
    assert_eq!(h.phase(), Phase::Preparing);
    h.ready(A);
    h.ready(B);
    assert_eq!(h.phase(), Phase::Playing);
    assert_eq!(h.status(A), (Status::Listening, None));
}

#[test]
fn seek_rejects_invalid_positions_and_allows_unknown_durations() {
    let mut h = Harness::playing_pair();
    let pid = h.pb_id();
    for ms in [json!(-1), json!(1.5), json!("10"), Value::Null] {
        let ack = h.cmd(A, "seek", json!({"playbackId": pid, "ms": ms}));
        assert_eq!(ack.error, Some(AckError::BadArgs));
        assert_eq!(h.pb_id(), pid);
    }
    h.room.current.as_mut().unwrap().duration_ms = None;
    assert!(
        h.cmd(A, "seek", json!({"playbackId": pid, "ms": 300_000}))
            .ok
    );
    assert_eq!(h.state().playback.start_ms, 300_000);
}

#[test]
fn temporary_load_failure_is_not_reported_as_unavailable() {
    let mut h = Harness::playing_pair();
    h.send(
        A,
        json!({"type": "error", "playbackId": h.pb_id(), "code": "load_failed"}),
    );
    assert_eq!(h.status(A), (Status::Out, Some(OutReason::LoadFailed)));
    // A rebuilt widget can rejoin without opening a new browser session.
    h.send(A, json!({"type": "join", "playbackId": h.pb_id()}));
    assert_eq!(h.status(A), (Status::Listening, None));
}

#[test]
fn lagging_client_is_awaited_before_next_track() {
    let mut h = Harness::playing_pair();
    h.advance(200_000);
    h.ended(A);
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::WaitingEnd);
    assert_eq!(pb.end_deadline, Some(h.now + 30_000));
    assert_eq!(h.status(A), (Status::Waiting, None));
    assert_eq!(h.status(B), (Status::Listening, None));

    h.advance(4_000);
    assert_eq!(h.phase(), Phase::WaitingEnd);
    h.ended(B);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.current_title().as_deref(), Some("two"));
    assert_eq!(h.pb_id(), "p2");
    assert_eq!(
        h.state().history.last().map(|e| e.title.as_str()),
        Some("one")
    );
    assert_eq!(h.status(A), (Status::Loading, None));
    assert_eq!(h.status(B), (Status::Loading, None));
}

#[test]
fn end_timeout_excludes_laggard() {
    let mut h = Harness::playing_pair();
    h.ended(A);
    h.advance(29_999);
    assert_eq!(h.phase(), Phase::WaitingEnd);
    h.advance(1);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.current_title().as_deref(), Some("two"));
    assert!(h.notices().iter().any(|n| n.starts_with("Лёша отстаёт")));
    // The new track gathers everyone again.
    assert_eq!(h.status(B), (Status::Loading, None));
}

#[test]
fn pause_in_waiting_end_freezes_deadline() {
    let mut h = Harness::playing_pair();
    h.advance(100_000);
    h.ended(A);
    h.advance(10_000);
    let anchor_ms = h.room.pb.position_at(h.now);

    h.cmd(B, "pause", json!({}));
    let pb = h.state().playback;
    assert!(pb.paused);
    assert_eq!(pb.end_deadline, None);
    assert_eq!(pb.start_at, None);
    assert_eq!(
        pb.anchor,
        Some(Anchor {
            at: h.now,
            ms: anchor_ms
        })
    );
    assert_eq!(h.status(A), (Status::Waiting, None));
    assert_eq!(h.status(B), (Status::Paused, None));

    h.advance(120_000);
    assert_eq!(h.phase(), Phase::WaitingEnd);

    h.cmd(A, "play", json!({}));
    let pb = h.state().playback;
    assert!(!pb.paused);
    assert_eq!(pb.end_deadline, Some(h.now + 20_000));
    assert_eq!(pb.start_at, Some(h.now + 1_500));
    assert_eq!(
        pb.anchor,
        Some(Anchor {
            at: h.now + 1_500,
            ms: anchor_ms
        })
    );

    h.advance(19_999);
    assert_eq!(h.phase(), Phase::WaitingEnd);
    h.advance(1);
    assert_eq!(h.current_title().as_deref(), Some("two"));
}

#[test]
fn pause_and_resume_keep_anchor_position() {
    let mut h = Harness::playing_pair();
    let start_at = h.room.pb.start_at.unwrap();
    h.advance(10_000);
    h.cmd(A, "pause", json!({}));
    let expected = h.now - start_at;
    assert_eq!(
        h.room.pb.anchor,
        Some(Anchor {
            at: h.now,
            ms: expected
        })
    );
    h.advance(5_000);
    assert_eq!(h.room.pb.position_at(h.now), expected);
    h.cmd(B, "play", json!({}));
    assert_eq!(
        h.room.pb.anchor,
        Some(Anchor {
            at: h.now + 1_500,
            ms: expected
        })
    );
}

#[test]
fn pause_during_preparing_cancels_start() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["one"]);
    h.cmd(A, "play", json!({}));
    h.ready(A);
    h.cmd(B, "pause", json!({}));
    assert_eq!(h.state().playback.ready_deadline, None);

    h.ready(B);
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Preparing);
    assert_eq!(pb.start_at, None);
    assert_eq!(h.status(A), (Status::Paused, None));

    // No ready timeout fires while paused.
    h.advance(60_000);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.status(B), (Status::Paused, None));

    h.cmd(A, "play", json!({}));
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Playing);
    assert_eq!(pb.start_at, Some(h.now + 1_500));
}

#[test]
fn play_in_preparing_restarts_ready_timer_when_not_all_ready() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["one"]);
    h.cmd(A, "pause", json!({}));
    h.cmd(A, "play", json!({}));
    h.cmd(A, "pause", json!({}));
    h.ready(A);
    h.advance(50_000);
    h.cmd(A, "play", json!({}));
    assert_eq!(h.state().playback.ready_deadline, Some(h.now + 20_000));
    h.advance(20_000);
    assert_eq!(h.phase(), Phase::Playing);
    assert_eq!(h.status(B), (Status::Out, Some(OutReason::ReadyTimeout)));
}

#[test]
fn next_skips_current_track_once() {
    let mut h = Harness::playing_pair();
    let current = h.room.current.as_ref().unwrap().id.clone();

    let ack = h.cmd(B, "next", json!({ "fromEntryId": current }));
    assert!(ack.ok);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.current_title().as_deref(), Some("two"));
    assert_eq!(h.pb_id(), "p2");
    assert!(h.notices().contains(&"Лёша: следующий трек".to_owned()));

    // The other participant pressed «next» for the same track at the same time.
    let ack = h.cmd(A, "next", json!({ "fromEntryId": current }));
    assert!(ack.ok);
    assert_eq!(h.current_title().as_deref(), Some("two"));
}

#[test]
fn next_keeps_paused_flag() {
    let mut h = Harness::playing_pair();
    h.cmd(A, "pause", json!({}));
    h.cmd(A, "next", json!({}));
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Preparing);
    assert!(pb.paused);
    assert_eq!(pb.ready_deadline, None);
}

#[test]
fn end_of_queue_goes_idle() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["only"]);
    h.cmd(A, "play", json!({}));
    h.ready(A);
    h.ready(B);
    h.ended(A);
    h.ended(B);
    let state = h.state();
    assert_eq!(state.playback.phase, Phase::Idle);
    assert_eq!(state.playback.playback_id, None);
    assert!(state.current.is_none());
    assert_eq!(state.history.len(), 1);
    assert!(h.notices().contains(&"Очередь закончилась".to_owned()));
    assert_eq!(h.status(A), (Status::Idle, None));

    // `play` on an empty queue does nothing.
    assert!(h.cmd(A, "play", json!({})).ok);
    assert_eq!(h.phase(), Phase::Idle);
}

#[test]
fn track_unavailable_for_everyone_is_skipped() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["broken", "fine"]);
    h.cmd(A, "play", json!({}));
    h.error(A);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.current_title().as_deref(), Some("broken"));
    h.error(B);
    assert_eq!(h.current_title().as_deref(), Some("fine"));
    assert_eq!(h.pb_id(), "p2");
    assert!(
        h.notices()
            .contains(&"Трек «broken» недоступен — пропускаем".to_owned())
    );
    assert_eq!(h.status(A), (Status::Loading, None));
}

#[test]
fn track_unavailable_for_one_plays_for_the_other() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["regional", "next"]);
    h.cmd(A, "play", json!({}));
    h.error(A);
    assert_eq!(h.status(A), (Status::Out, Some(OutReason::Unavailable)));
    h.ready(B);
    assert_eq!(h.phase(), Phase::Playing);
    assert_eq!(h.status(B), (Status::Listening, None));

    // The excluded participant does not hold up the end of the track.
    h.ended(B);
    assert_eq!(h.current_title().as_deref(), Some("next"));
}

#[test]
fn ready_timeout_excludes_participants_that_did_not_load() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["one"]);
    h.cmd(A, "play", json!({}));
    h.ready(A);
    h.advance(20_000);
    assert_eq!(h.phase(), Phase::Playing);
    assert_eq!(h.status(B), (Status::Out, Some(OutReason::ReadyTimeout)));
    assert!(
        h.notices()
            .iter()
            .any(|n| n.starts_with("Лёша не успевает загрузиться"))
    );

    // Late `ready` is ignored; the way back in is `join`.
    h.ready(B);
    assert_eq!(h.status(B), (Status::Out, Some(OutReason::ReadyTimeout)));
    let id = h.pb_id();
    h.send(B, json!({ "type": "join", "playbackId": id }));
    assert_eq!(h.status(B), (Status::Listening, None));
}

#[test]
fn nobody_ready_keeps_waiting_until_first_ready() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.send(A, json!({ "type": "audio", "blocked": true }));
    h.import(&["one"]);
    h.cmd(A, "play", json!({}));
    assert_eq!(h.status(A), (Status::Blocked, None));
    h.advance(20_000);
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Preparing);
    assert_eq!(pb.ready_deadline, None);
    assert_eq!(h.status(B), (Status::Loading, None));

    h.advance(30_000);
    h.ready(A);
    assert_eq!(h.status(A), (Status::Waiting, None));
    assert_eq!(h.state().playback.ready_deadline, Some(h.now + 20_000));
    h.advance(20_000);
    assert_eq!(h.phase(), Phase::Playing);
    assert_eq!(h.status(B), (Status::Out, Some(OutReason::ReadyTimeout)));
}

#[test]
fn offline_participant_drops_out_after_grace() {
    let mut h = Harness::playing_pair();
    h.disconnect(B);
    let view = h.state();
    let b = view.participants.iter().find(|p| p.name == "Лёша").unwrap();
    assert!(!b.online);
    assert_eq!(b.status, Status::Offline);

    h.ended(A);
    h.advance(14_999);
    assert_eq!(h.phase(), Phase::WaitingEnd);
    h.advance(1);
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.current_title().as_deref(), Some("two"));
    // Still gone when the next track is prepared, so not part of its barrier either.
    assert_eq!(h.status(B), (Status::Offline, Some(OutReason::Offline)));

    // Coming back while the track is loading puts them right back into the barrier.
    h.connect(B);
    assert_eq!(h.status(B), (Status::Loading, None));
}

#[test]
fn offline_participant_unblocks_start() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["one"]);
    h.cmd(A, "play", json!({}));
    h.ready(A);
    h.disconnect(B);
    h.advance(15_000);
    assert_eq!(h.phase(), Phase::Playing);
    assert!(h.notices().iter().any(|n| n.starts_with("Лёша: нет связи")));
}

#[test]
fn reconnect_within_grace_keeps_barrier_place() {
    let mut h = Harness::playing_pair();
    h.disconnect(B);
    h.advance(5_000);
    h.connect(B);
    assert_eq!(h.status(B), (Status::Listening, None));
    h.ended(A);
    h.advance(20_000);
    assert_eq!(h.phase(), Phase::WaitingEnd);
}

#[test]
fn late_joiner_does_not_block_until_join() {
    let mut h = Harness::playing_pair();
    h.connect(C);
    assert_eq!(h.status(C), (Status::Out, Some(OutReason::JoinedLate)));

    h.ended(A);
    h.ended(B);
    assert_eq!(h.current_title().as_deref(), Some("two"));
    // Everyone online takes part in the next track.
    assert_eq!(h.status(C), (Status::Loading, None));
    h.ready(A);
    h.ready(B);
    h.ready(C);
    assert_eq!(h.phase(), Phase::Playing);
}

#[test]
fn late_joiner_counts_after_join() {
    let mut h = Harness::playing_pair();
    h.connect(C);
    let id = h.pb_id();
    h.send(C, json!({ "type": "join", "playbackId": id }));
    assert_eq!(h.status(C), (Status::Listening, None));
    h.ended(A);
    h.ended(B);
    assert_eq!(h.phase(), Phase::WaitingEnd);
    h.ended(C);
    assert_eq!(h.current_title().as_deref(), Some("two"));
}

#[test]
fn joiner_during_preparing_enters_barrier() {
    let mut h = Harness::new();
    h.connect(A);
    h.import(&["one"]);
    h.cmd(A, "play", json!({}));
    h.connect(B);
    assert_eq!(h.status(B), (Status::Loading, None));
    h.ready(A);
    assert_eq!(h.phase(), Phase::Preparing);
    h.ready(B);
    assert_eq!(h.phase(), Phase::Playing);
}

#[test]
fn stale_and_duplicate_ended_do_not_skip_twice() {
    let mut h = Harness::playing_pair();
    let old = h.pb_id();
    h.ended(A);
    h.ended(A);
    assert_eq!(h.phase(), Phase::WaitingEnd);
    h.ended(B);
    assert_eq!(h.current_title().as_deref(), Some("two"));

    let revision = h.room.revision();
    h.send(A, json!({ "type": "ended", "playbackId": old }));
    h.send(B, json!({ "type": "ended", "playbackId": old }));
    h.send(B, json!({ "type": "ready", "playbackId": old }));
    assert_eq!(h.current_title().as_deref(), Some("two"));
    assert_eq!(h.phase(), Phase::Preparing);
    assert_eq!(h.room.revision(), revision);
}

#[test]
fn queue_move_conflict_returns_state() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.import(&["one", "two", "three"]);
    let seen = h.room.revision();
    let ids: Vec<String> = h.state().queue.iter().map(|e| e.id.clone()).collect();

    // Unrelated changes (participants, playback) do not invalidate a queue edit.
    h.send(A, json!({ "type": "audio", "blocked": true }));
    let ack = h.cmd_with_id(
        A,
        "m1",
        "queue.move",
        json!({ "entryId": ids[2], "afterId": null }),
        Some(seen),
    );
    assert!(ack.ok);
    let order: Vec<String> = h.state().queue.iter().map(|e| e.title.clone()).collect();
    assert_eq!(order, ["three", "one", "two"]);

    let ack = h.cmd_with_id(
        B,
        "m2",
        "queue.move",
        json!({ "entryId": ids[0], "afterId": ids[1] }),
        Some(seen),
    );
    assert!(!ack.ok);
    assert_eq!(ack.error, Some(AckError::Conflict));
    let state = ack.state.expect("conflict carries state");
    assert_eq!(state.queue[0].title, "three");

    let ack = h.cmd_with_id(
        B,
        "m3",
        "queue.move",
        json!({ "entryId": ids[0], "afterId": ids[1] }),
        Some(state.revision),
    );
    assert!(ack.ok);
    let order: Vec<String> = h.state().queue.iter().map(|e| e.title.clone()).collect();
    assert_eq!(order, ["three", "two", "one"]);

    let ack = h.cmd_with_id(
        B,
        "m4",
        "queue.move",
        json!({ "entryId": ids[0], "afterId": null }),
        None,
    );
    assert_eq!(ack.error, Some(AckError::BadArgs));
    let ack = h.cmd(B, "queue.remove", json!({ "entryId": "nope" }));
    assert_eq!(ack.error, Some(AckError::NotFound));
}

#[test]
fn duplicate_command_is_not_applied_twice() {
    let mut h = Harness::playing_pair();
    let first = h.cmd_with_id(A, "same", "next", json!({}), None);
    assert_eq!(h.current_title().as_deref(), Some("two"));
    let again = h.cmd_with_id(A, "same", "next", json!({}), None);
    assert_eq!(h.current_title().as_deref(), Some("two"));
    assert_eq!(first.revision, again.revision);
    assert!(
        !h.last
            .iter()
            .any(|o| matches!(o, Output::Broadcast(ServerMsg::State { .. })))
    );

    let ids: Vec<String> = h.state().queue.iter().map(|e| e.id.clone()).collect();
    h.cmd_with_id(A, "rm", "queue.remove", json!({ "entryId": ids[0] }), None);
    let again = h.cmd_with_id(A, "rm", "queue.remove", json!({ "entryId": ids[0] }), None);
    assert!(
        again.ok,
        "a retried remove returns the original ack, not not_found"
    );
}

#[test]
fn import_is_one_revision_and_idempotent() {
    let mut h = Harness::new();
    h.connect(A);
    let before = h.room.revision();
    let batch = ImportBatch {
        command_id: "imp".into(),
        client_id: format!("client-{A}"),
        collection: Some(Collection {
            title: "Soulhack".into(),
            kind: CollectionKind::Album,
        }),
        tracks: (0..12)
            .map(|i| TrackInfo {
                track_id: i,
                widget_url: format!("https://api.soundcloud.com/tracks/{i}"),
                permalink: String::new(),
                title: format!("t{i}"),
                artist: "Forss".into(),
                duration_ms: None,
                artwork: None,
            })
            .collect(),
        unavailable: Vec::new(),
        missing: 0,
    };
    let (first, outputs) = h.room.import(batch.clone(), h.now);
    let first = first.unwrap();
    h.last = outputs;
    assert_eq!(first.added, 12);
    assert_eq!(first.revision, before + 1);
    assert!(
        h.notices()
            .contains(&"Ника добавляет альбом «Soulhack» — 12 треков".to_owned())
    );
    let queue = h.state().queue;
    assert!(queue.iter().all(|e| e.added_by == "Ника"));
    assert_eq!(
        queue.iter().map(|e| e.track_id).collect::<Vec<_>>(),
        (0..12).collect::<Vec<_>>()
    );

    let (again, outputs) = h.room.import(batch, h.now);
    assert!(outputs.is_empty());
    assert_eq!(again.unwrap().revision, first.revision);
    assert_eq!(h.state().queue.len(), 12);
}

#[test]
fn restore_from_snapshot_is_paused() {
    let mut h = Harness::playing_pair();
    h.advance(90_000);
    let id = h.pb_id();
    h.send(
        A,
        json!({ "type": "position", "playbackId": id, "ms": 83_000, "playing": true }),
    );
    assert!(h.last.iter().any(|o| matches!(o, Output::PersistPosition)));
    let snapshot = h.room.snapshot(h.now);
    assert_eq!(snapshot.position_ms, Some(83_000));
    let seen_revision = h.room.revision();

    let json = serde_json::to_string(&snapshot).unwrap();
    let restored = Room::restore(
        "Room000001".into(),
        TIMING,
        serde_json::from_str(&json).unwrap(),
    );
    let mut h = Harness::with_room(restored);
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Preparing);
    assert!(pb.paused);
    assert_eq!(pb.start_ms, 83_000);
    assert_eq!(pb.playback_id.as_deref(), Some("p2"));
    assert_eq!(pb.ready_deadline, None);
    assert!(h.room.revision() > seen_revision);
    assert_eq!(h.current_title().as_deref(), Some("one"));
    assert_eq!(h.state().queue.len(), 2);

    h.connect(A);
    h.connect(B);
    h.ready(A);
    h.ready(B);
    h.advance(120_000);
    assert_eq!(h.phase(), Phase::Preparing, "music never starts by itself");

    h.cmd(A, "play", json!({}));
    let pb = h.state().playback;
    assert_eq!(pb.phase, Phase::Playing);
    assert_eq!(
        pb.anchor,
        Some(Anchor {
            at: h.now + 1_500,
            ms: 83_000
        })
    );
}

#[test]
fn reconnect_with_same_client_takes_over_socket() {
    let mut h = Harness::playing_pair();
    h.connect_as(C, A);
    assert!(h.last.iter().any(|o| matches!(o, Output::Close(A))));
    let first_to_new = h.last.iter().find_map(|o| match o {
        Output::Send(C, msg) => Some(msg),
        _ => None,
    });
    assert!(matches!(first_to_new, Some(ServerMsg::Welcome { .. })));
    assert_eq!(h.state().participants.len(), 2);

    // The old socket closing afterwards must not mark the participant offline.
    h.disconnect(A);
    assert_eq!(h.status(A), (Status::Listening, None));
}

#[test]
fn rejected_and_invalid_commands_are_acked() {
    let mut h = Harness::new();
    h.connect(A);
    h.input(Input::Rejected {
        conn: A,
        command_id: "flood".into(),
        error: AckError::RateLimited,
    });
    assert!(matches!(
        h.last.first(),
        Some(Output::Send(
            A,
            ServerMsg::Ack(Ack {
                ok: false,
                error: Some(AckError::RateLimited),
                ..
            })
        ))
    ));
    assert_eq!(h.cmd(A, "dance", json!({})).error, Some(AckError::BadArgs));
    assert_eq!(
        h.cmd(A, "rename", json!({ "name": "   " })).error,
        Some(AckError::BadArgs)
    );
    assert!(h.cmd(A, "rename", json!({ "name": " Ника 2 " })).ok);
    assert_eq!(h.state().participants[0].name, "Ника 2");
}

#[test]
fn offline_participants_are_forgotten_eventually() {
    let mut h = Harness::new();
    h.connect(A);
    h.connect(B);
    h.disconnect(B);
    h.advance(FORGET_OFFLINE_MS);
    assert_eq!(h.state().participants.len(), 1);
}

#[test]
fn russian_plurals() {
    assert_eq!(tracks(1), "1 трек");
    assert_eq!(tracks(3), "3 трека");
    assert_eq!(tracks(5), "5 треков");
    assert_eq!(tracks(11), "11 треков");
    assert_eq!(tracks(21), "21 трек");
    assert_eq!(tracks(112), "112 треков");
}
