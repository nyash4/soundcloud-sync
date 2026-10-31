// Drives the hidden SoundCloud widget from server state (§5). Widget events never
// turn into commands: they only produce reports (ready / ended / position / error / join).

import { serverNow, toClientTime } from './clock';
import { anchorPosition } from './format';
import type { Entry, ErrorCode, Participant, Playback, Report, RoomState } from './protocol';
import { after, at, every, type Cancel } from './timers';
import { Widget, WidgetLoadError, type ProgressData, type WidgetEvent } from './widget';

export type PlayerMode =
  | 'idle' // nothing loaded for the current playback (or nothing playing at all)
  | 'loading'
  | 'warming'
  | 'blocked' // browser refused to start audio; needs a click
  | 'ready' // loaded, paused at the start position
  | 'playing'
  | 'paused'
  | 'ended' // finished locally, waiting for the others
  | 'failed';

export interface PlayerView {
  playbackId: string | null;
  mode: PlayerMode;
  /** Last trusted local position: track was at `ms` at client time `at`. */
  sample: { ms: number; at: number } | null;
  durationMs: number | null;
  volume: number;
  /** A join to the current position is in progress or done. */
  joining: boolean;
}

type Target = { kind: 'prepare'; startMs: number } | { kind: 'join' };

/** Without PLAY this long after play(), autoplay is considered blocked. */
const BLOCK_DETECT_MS = 2500;
/**
 * Warm-up volume. Not 0: Firefox lets inaudible media autoplay, so a silent warm-up would pass
 * and the real, audible start would then be blocked. Slider level 1 maps to 0.01 % output
 * for a fraction of a second, while still counting as audible for the autoplay policy.
 */
const WARM_VOLUME = 1;
/** Warm-up that started playing but never progressed is a load failure. */
const WARM_LIMIT_MS = 15_000;
const POSITION_REPORT_MS = 5000;
/** A start missed by more than this (e.g. state arrived late) is caught up with one seek. */
const LATE_START_SEEK_MS = 1000;
/** PLAY_PROGRESS samples closer than this to the extrapolated position are ignored to avoid jitter. */
const SAMPLE_TOLERANCE_MS = 300;
const NEAR_END_MS = 1500;
const MAX_RECOVERY_ATTEMPTS = 3;

export class Player {
  private widget = new Widget();
  private view: PlayerView;
  private entry: Entry | null = null;
  private loadedEntryId: string | null = null;
  private room: RoomState | null = null;
  private member = false;
  private target: Target | null = null;
  private wasBlocked = false;
  private failCode: ErrorCode | null = null;
  /** What we asked the widget to do; anything else it does on its own is undone. */
  private intent: 'silent' | 'warm' | 'play' = 'silent';
  private playSeen = false;
  /** PLAY_PROGRESS arrived since the last play(); proves audio actually runs. */
  private progressSeen = false;
  private startTimer: Cancel | null = null;
  private scheduledFor: number | null = null;
  private warmTimers: Cancel[] = [];
  private positionTimer: Cancel | null = null;
  private recoveryTimer: Cancel | null = null;
  private recoveryAttempts = 0;
  private loadGeneration = 0;
  private modeSince = Date.now();

  constructor(
    private report: (msg: Report) => void,
    private publish: (view: PlayerView) => void,
    volume: number,
  ) {
    this.view = { playbackId: null, mode: 'idle', sample: null, durationMs: null, volume, joining: false };
    this.widget.onEvent((e) => this.onWidget(e));
    document.addEventListener('visibilitychange', this.onVisible);
    document.addEventListener('pointerdown', this.onInteraction, true);
    document.addEventListener('keydown', this.onInteraction, true);
    this.emit();
  }

  /** Applies a new room snapshot. `afterWelcome` re-sends whatever the server may have missed. */
  sync(room: RoomState, selfId: string | null, afterWelcome = false): void {
    this.room = room;
    const me = room.participants.find((p) => p.id === selfId);
    // A blocked outsider still has status "blocked"; outReason records actual membership.
    this.member = !!me && me.outReason === null && me.status !== 'out';
    const pb = room.playback;
    const cur = room.current;

    if (!cur || !pb.playbackId || pb.phase === 'idle') {
      if (this.view.playbackId !== null || this.view.mode !== 'idle') this.reset(null, null);
      return;
    }

    const restarted = pb.phase === 'preparing' && ['playing', 'paused', 'ended'].includes(this.view.mode);
    if (pb.playbackId !== this.view.playbackId || restarted) {
      this.reset(pb.playbackId, cur);
      if (pb.phase === 'preparing') {
        const target = { kind: 'prepare', startMs: pb.startMs } as const;
        if (this.loadedEntryId === cur.id) this.warm(target);
        else void this.loadAndWarm(target);
      } else {
        this.join();
      }
      return;
    }

    if ((pb.phase === 'playing' || pb.phase === 'waiting_end') && !this.member &&
        !this.view.joining && ['idle', 'ready'].includes(this.view.mode)) {
      this.join();
      return;
    }
    if (afterWelcome) this.resendAfterWelcome(me);
    this.applyPlayback(pb);
  }

  /** Must be called directly from a click handler: play() needs the user gesture. */
  enableSound(): void {
    if (this.view.mode !== 'blocked' || !this.target) return;
    // Once the track is running, the click must land on the current position, not the start.
    const phase = this.room?.playback.phase;
    const midTrack = phase === 'playing' || phase === 'waiting_end';
    if (midTrack) {
      if (!this.member) this.view.joining = true;
      this.warm({ kind: 'join' });
    } else {
      this.warm(this.target);
    }
  }

  /** Late join (§5): load if needed, seek once by the anchor, then play or stay paused. */
  join(): void {
    if (!this.view.playbackId || !this.entry) return;
    this.view.joining = true;
    if (this.loadedEntryId === this.entry.id) this.warm({ kind: 'join' });
    else void this.loadAndWarm({ kind: 'join' });
  }

  setVolume(volume: number): void {
    this.view.volume = volume;
    if (this.intent !== 'warm') this.widget.setVolume(volume);
    this.emit();
  }

  destroy(): void {
    this.loadGeneration++;
    this.recoveryTimer?.();
    document.removeEventListener('visibilitychange', this.onVisible);
    document.removeEventListener('pointerdown', this.onInteraction, true);
    document.removeEventListener('keydown', this.onInteraction, true);
    this.cancelStart();
    this.clearWarm();
    this.stopPositionReports();
    this.widget.destroy();
  }

  // ---- state transitions ----

  private reset(playbackId: string | null, entry: Entry | null): void {
    this.loadGeneration++;
    this.recoveryTimer?.();
    this.recoveryTimer = null;
    this.recoveryAttempts = 0;
    const duration = entry?.id === this.entry?.id ? this.view.durationMs : (entry?.durationMs ?? null);
    this.cancelStart();
    this.clearWarm();
    this.stopPositionReports();
    if (this.intent !== 'silent' || this.view.mode === 'playing') this.widget.pause();
    this.intent = 'silent';
    this.entry = entry;
    this.target = null;
    this.wasBlocked = false;
    this.failCode = null;
    this.view = {
      playbackId,
      mode: 'idle',
      sample: null,
      durationMs: duration,
      volume: this.view.volume,
      joining: false,
    };
    this.emit();
  }

  private applyPlayback(pb: Playback): void {
    const mode = this.view.mode;
    if (pb.paused || pb.startAt === null) {
      this.cancelStart();
      if (mode === 'playing') this.pauseLocal();
      return;
    }
    if ((mode === 'ready' || mode === 'paused') && (this.member || this.view.joining)) this.scheduleStart(pb.startAt);
  }

  private resendAfterWelcome(me: Participant | undefined): void {
    const pid = this.view.playbackId;
    if (!pid) return;
    switch (this.view.mode) {
      case 'ready':
        this.report({ type: 'ready', playbackId: pid });
        break;
      case 'blocked':
        this.report({ type: 'audio', blocked: true });
        break;
      case 'failed':
        if (this.failCode) this.report({ type: 'error', playbackId: pid, code: this.failCode });
        break;
      case 'ended':
        this.report({ type: 'ended', playbackId: pid });
        break;
      case 'playing':
      case 'paused':
        // Recover at the room's current position after a long connection loss.
        if (me?.outReason !== null && me?.outReason !== undefined) {
          this.join();
          break;
        }
        void this.reportPosition();
        break;
    }
  }

  private async loadAndWarm(target: Target): Promise<void> {
    const generation = ++this.loadGeneration;
    const pid = this.view.playbackId;
    const entry = this.entry;
    if (!pid || !entry) return;
    this.loadedEntryId = null;
    this.target = target;
    this.setMode('loading');
    try {
      await this.widget.load(entry.widgetUrl);
    } catch (e) {
      if (pid !== this.view.playbackId || generation !== this.loadGeneration) return;
      const superseded = e instanceof WidgetLoadError && (e.message === 'superseded' || e.message === 'destroyed');
      if (!superseded) this.recover(e instanceof WidgetLoadError && e.message === 'error' ? 'unavailable' : 'load_failed');
      return;
    }
    if (pid !== this.view.playbackId || generation !== this.loadGeneration) return;
    // A load can "succeed" into an empty widget when the track is gone or private.
    const sound = await this.widget.currentSound();
    if (pid !== this.view.playbackId || generation !== this.loadGeneration) return;
    // A missing/timed-out getter is a transport failure, not proof of unavailability.
    if (!sound) return this.recover('load_failed');
    if (this.view.durationMs === null) {
      const d = await this.widget.duration();
      if (pid !== this.view.playbackId || generation !== this.loadGeneration) return;
      if (d) this.view.durationMs = d;
    }
    this.loadedEntryId = entry.id;
    this.warm(target);
  }

  /** Muted play → first progress → pause & seek. Proves the browser lets us play (§5). */
  private warm(target: Target): void {
    this.clearWarm();
    this.target = target;
    this.intent = 'warm';
    this.playSeen = false;
    this.progressSeen = false;
    this.widget.setVolume(WARM_VOLUME);
    this.widget.seekTo(target.kind === 'prepare' ? target.startMs :
      (this.room ? anchorPosition(this.room.playback, serverNow()) ?? this.room.playback.startMs : 0));
    this.widget.play();
    this.setMode('warming');
    this.warmTimers.push(
      after(BLOCK_DETECT_MS, () => {
        if (this.view.mode === 'warming' && !this.playSeen) {
          if (document.hidden) this.recover('load_failed');
          else this.block();
        }
      }),
      after(WARM_LIMIT_MS, () => {
        if (this.view.mode === 'warming') this.recover('load_failed');
      }),
    );
  }

  private warmedUp(): void {
    this.recoveryAttempts = 0;
    this.clearWarm();
    const pid = this.view.playbackId;
    const target = this.target;
    if (!pid || !target) return;
    if (this.wasBlocked) {
      this.wasBlocked = false;
      this.report({ type: 'audio', blocked: false });
    }
    if (target.kind === 'prepare' && this.room?.playback.phase === 'preparing') {
      this.intent = 'silent';
      this.widget.pause();
      this.widget.seekTo(target.startMs);
      this.widget.setVolume(this.view.volume);
      this.view.sample = { ms: target.startMs, at: Date.now() };
      this.setMode('ready');
      this.report({ type: 'ready', playbackId: pid });
      if (this.room) this.applyPlayback(this.room.playback);
      return;
    }
    this.finishJoin(pid);
  }

  private finishJoin(pid: string): void {
    const pb = this.room?.playback;
    if (!pb) return;
    const now = serverNow();
    const running = !pb.paused && pb.startAt !== null && now >= pb.startAt;
    const limit = this.view.durationMs !== null ? Math.max(0, this.view.durationMs - 1000) : Infinity;
    const pos = Math.min(limit, anchorPosition(pb, now) ?? pb.startMs);
    this.widget.seekTo(pos);
    this.widget.setVolume(this.view.volume);
    this.view.sample = { ms: pos, at: Date.now() };
    if (running) {
      this.intent = 'play';
      this.setMode('playing');
      this.startPositionReports();
    } else {
      this.intent = 'silent';
      this.widget.pause();
      this.setMode('paused');
    }
    // A barrier member recovering from a blocked start is already counted; only outsiders join.
    if (!this.member) this.report({ type: 'join', playbackId: pid });
    this.member = true;
    this.applyPlayback(pb);
  }

  private block(): void {
    this.clearWarm();
    this.stopPositionReports();
    this.cancelStart();
    this.intent = 'silent';
    this.widget.pause();
    this.wasBlocked = true;
    this.setMode('blocked');
    this.report({ type: 'audio', blocked: true });
  }

  private fail(code: ErrorCode): void {
    this.loadedEntryId = null;
    this.clearWarm();
    this.cancelStart();
    this.stopPositionReports();
    this.intent = 'silent';
    this.widget.pause();
    this.failCode = code;
    this.setMode('failed');
    const pid = this.view.playbackId;
    if (pid) this.report({ type: 'error', playbackId: pid, code });
  }

  /** Recreate a stalled widget just as a page reload would, without losing the room. */
  private recover(code: ErrorCode): void {
    if (!this.view.playbackId || !this.entry || this.recoveryTimer) return;
    this.loadGeneration++;
    this.clearWarm();
    this.cancelStart();
    this.stopPositionReports();
    this.intent = 'silent';
    this.widget.pause();
    this.loadedEntryId = null;
    if (this.recoveryAttempts >= MAX_RECOVERY_ATTEMPTS) {
      // A suspended/background iframe is retried when the tab returns, never declared unavailable.
      if (document.hidden) this.setMode('loading');
      else this.fail(code);
      return;
    }
    this.recoveryAttempts++;
    this.setMode('loading');
    this.recoveryTimer = after(1000 * this.recoveryAttempts, () => {
      this.recoveryTimer = null;
      this.widget.destroy();
      const pb = this.room?.playback;
      if (!pb || !this.view.playbackId) return;
      const target: Target = pb.phase === 'preparing' ? { kind: 'prepare', startMs: pb.startMs } : { kind: 'join' };
      if (target.kind === 'join') this.view.joining = true;
      void this.loadAndWarm(target);
    });
  }

  private onInteraction = (): void => {
    if (this.view.mode === 'blocked') this.enableSound();
    else if (this.view.mode === 'failed') {
      this.recoveryAttempts = 0;
      this.recover('load_failed');
    }
  };

  private onVisible = (): void => {
    if (document.hidden) return;
    if (this.view.mode === 'blocked') {
      this.enableSound();
    } else if (this.view.mode === 'failed' ||
      (['loading', 'warming'].includes(this.view.mode) &&
        (Date.now() - this.modeSince > 5000 || this.recoveryAttempts >= MAX_RECOVERY_ATTEMPTS))) {
      this.recoveryTimer?.();
      this.recoveryTimer = null;
      this.recoveryAttempts = 0;
      this.recover('load_failed');
    }
  };

  private scheduleStart(startAt: number): void {
    if (this.scheduledFor === startAt) return;
    this.cancelStart();
    this.scheduledFor = startAt;
    this.startTimer = at(toClientTime(startAt), () => this.startNow(startAt));
  }

  private startNow(startAt: number): void {
    this.startTimer = null;
    this.scheduledFor = null;
    const pb = this.room?.playback;
    if (!pb || pb.startAt !== startAt || pb.paused) return;
    const mode = this.view.mode;
    if (mode !== 'ready' && mode !== 'paused') return;
    let from = this.currentPosition();
    // Only a fresh start is caught up; a resume always continues from our own position.
    if (mode === 'ready' && serverNow() - startAt > LATE_START_SEEK_MS) {
      const pos = anchorPosition(pb, serverNow());
      if (pos !== null) {
        this.widget.seekTo(pos);
        from = pos;
      }
    }
    this.intent = 'play';
    this.playSeen = false;
    this.progressSeen = false;
    this.widget.setVolume(this.view.volume);
    this.widget.play();
    this.view.sample = { ms: from, at: Date.now() };
    this.setMode('playing');
    this.startPositionReports();
    // The audible play can still be refused even after a successful warm-up.
    this.clearWarm();
    this.warmTimers.push(
      after(BLOCK_DETECT_MS, () => {
        if (this.view.mode === 'playing' && !this.progressSeen) {
          if (document.hidden || this.playSeen) this.recover('load_failed');
          else this.block();
        }
      }),
    );
  }

  private pauseLocal(): void {
    this.intent = 'silent';
    this.widget.pause();
    this.view.sample = { ms: this.currentPosition(), at: Date.now() };
    this.stopPositionReports();
    this.setMode('paused');
    void this.reportPosition();
  }

  // ---- widget events ----

  private onWidget(e: WidgetEvent): void {
    switch (e.type) {
      case 'PLAY':
        if (this.intent === 'silent') return this.widget.pause();
        this.playSeen = true;
        if (this.intent === 'play') void this.checkSound();
        return;
      case 'PLAY_PROGRESS':
        if (this.intent === 'silent') return this.widget.pause();
        this.progressSeen = true;
        if (this.intent === 'warm') {
          if (this.view.mode === 'warming') this.warmedUp();
          return;
        }
        this.takeSample(e.data);
        return;
      case 'PAUSE':
        if (this.intent === 'warm' && this.view.mode === 'warming') return this.block();
        if (this.intent !== 'play' || this.view.mode !== 'playing') return;
        // Right before the end the widget may pause on its own; FINISH follows.
        if (this.progressSeen && this.nearEnd(e.data)) return;
        // Either the browser refused to start audio, or something outside (a media key) stopped it.
        // Both recover the same way: a click that resumes at the room's position.
        return this.block();
      case 'FINISH':
        if (!['playing', 'paused', 'blocked'].includes(this.view.mode)) return;
        this.intent = 'silent';
        this.widget.pause();
        this.stopPositionReports();
        this.cancelStart();
        this.view.sample = this.view.durationMs !== null ? { ms: this.view.durationMs, at: Date.now() } : this.view.sample;
        this.setMode('ended');
        if (this.view.playbackId) this.report({ type: 'ended', playbackId: this.view.playbackId });
        return;
      case 'ERROR':
        if (['playing', 'paused', 'warming', 'ready'].includes(this.view.mode)) this.recover('unavailable');
        return;
    }
  }

  /** The widget must never wander off to another sound on its own. */
  private async checkSound(): Promise<void> {
    const expected = this.entry?.trackId;
    const sound = await this.widget.currentSound();
    if (sound?.id !== undefined && expected !== undefined && sound.id !== expected && this.intent === 'play') {
      this.intent = 'silent';
      this.widget.pause();
    }
  }

  private takeSample(data: ProgressData | undefined): void {
    if (!data || !Number.isFinite(data.currentPosition)) return;
    const ms = data.currentPosition;
    if (this.view.mode === 'playing' && this.view.sample) {
      const predicted = this.view.sample.ms + (Date.now() - this.view.sample.at);
      if (Math.abs(predicted - ms) < SAMPLE_TOLERANCE_MS) return;
    }
    this.view.sample = { ms, at: Date.now() };
    this.emit();
  }

  private nearEnd(data: ProgressData | undefined): boolean {
    const d = this.view.durationMs;
    const pos = data?.currentPosition ?? this.currentPosition();
    return d !== null && d - pos < NEAR_END_MS;
  }

  private currentPosition(): number {
    const s = this.view.sample;
    if (!s) return 0;
    return this.view.mode === 'playing' ? s.ms + (Date.now() - s.at) : s.ms;
  }

  // ---- reports & timers ----

  private async reportPosition(): Promise<void> {
    const pid = this.view.playbackId;
    const ms = (await this.widget.position()) ?? this.currentPosition();
    if (pid && pid === this.view.playbackId) {
      this.report({ type: 'position', playbackId: pid, ms: Math.round(ms), playing: this.view.mode === 'playing' });
    }
  }

  private startPositionReports(): void {
    this.stopPositionReports();
    this.positionTimer = every(POSITION_REPORT_MS, () => void this.reportPosition());
  }

  private stopPositionReports(): void {
    this.positionTimer?.();
    this.positionTimer = null;
  }

  private cancelStart(): void {
    this.startTimer?.();
    this.startTimer = null;
    this.scheduledFor = null;
  }

  private clearWarm(): void {
    for (const cancel of this.warmTimers) cancel();
    this.warmTimers = [];
  }

  private setMode(mode: PlayerMode): void {
    this.modeSince = Date.now();
    this.view.mode = mode;
    this.emit();
  }

  private emit(): void {
    this.publish({ ...this.view, sample: this.view.sample && { ...this.view.sample } });
  }
}
