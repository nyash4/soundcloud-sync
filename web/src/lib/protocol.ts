// Mirrors docs/PROTOCOL.md. Change the document first, then this file.

export type CollectionKind = 'album' | 'playlist';

export interface Collection {
  title: string;
  kind: CollectionKind;
}

export interface Entry {
  id: string;
  trackId: number;
  widgetUrl: string;
  permalink: string;
  title: string;
  artist: string;
  durationMs: number | null;
  artwork: string | null;
  collection: Collection | null;
  addedBy: string;
}

export type ParticipantStatus =
  | 'listening'
  | 'paused'
  | 'loading'
  | 'waiting'
  | 'offline'
  | 'out'
  | 'blocked'
  | 'idle';

export type OutReason = 'joined_late' | 'ready_timeout' | 'end_timeout' | 'unavailable' | 'load_failed' | 'offline';

export interface Participant {
  id: string;
  name: string;
  online: boolean;
  status: ParticipantStatus;
  outReason: OutReason | null;
}

export type Phase = 'idle' | 'preparing' | 'playing' | 'waiting_end';

export interface Anchor {
  at: number;
  ms: number;
}

export interface Playback {
  phase: Phase;
  paused: boolean;
  playbackId: string | null;
  startMs: number;
  startAt: number | null;
  anchor: Anchor | null;
  readyDeadline: number | null;
  endDeadline: number | null;
}

export interface RoomState {
  roomId: string;
  revision: number;
  serverTime: number;
  participants: Participant[];
  current: Entry | null;
  queue: Entry[];
  history: Entry[];
  playback: Playback;
}

// ---- Commands (§3.1) ----

export interface CommandArgs {
  'queue.remove': { entryId: string };
  'queue.move': { entryId: string; afterId: string | null };
  play: Record<string, never>;
  pause: Record<string, never>;
  seek: { playbackId: string; ms: number };
  next: { fromEntryId: string | null };
  rename: { name: string };
}

export type CommandName = keyof CommandArgs;

export type ErrorCode = 'unavailable' | 'load_failed';

export type ClientMessage =
  | { type: 'hello'; roomId: string; token: string; clientId: string; name: string }
  | { type: 'ping'; t0: number }
  | { type: 'cmd'; commandId: string; name: CommandName; args: CommandArgs[CommandName]; revision?: number }
  | { type: 'ready'; playbackId: string }
  | { type: 'ended'; playbackId: string }
  | { type: 'error'; playbackId: string; code: ErrorCode }
  | { type: 'join'; playbackId: string }
  | { type: 'position'; playbackId: string; ms: number; playing: boolean }
  | { type: 'audio'; blocked: boolean };

/** Reports are the player-originated messages; they are never queued for retry. */
export type Report = Extract<ClientMessage, { type: 'ready' | 'ended' | 'error' | 'join' | 'position' | 'audio' }>;

// ---- Server messages (§3.2) ----

export type AckError = 'conflict' | 'not_found' | 'bad_args' | 'rate_limited';

export type Ack =
  | { type: 'ack'; commandId: string; ok: true; revision: number }
  | { type: 'ack'; commandId: string; ok: false; error: AckError; message: string; state: RoomState };

export type FatalError = 'bad_token' | 'room_not_found' | 'bad_hello' | 'session_replaced';

export type NoticeLevel = 'info' | 'warn';

export type ServerMessage =
  | { type: 'welcome'; selfId: string; state: RoomState; serverTime: number }
  | { type: 'pong'; t0: number; serverTime: number }
  | Ack
  | { type: 'state'; state: RoomState }
  | { type: 'notice'; level: NoticeLevel; text: string; at: number }
  | { type: 'fatal'; error: FatalError; message: string };

// ---- HTTP (§2) ----

export interface CreatedRoom {
  roomId: string;
  token: string;
}

export interface ImportRequest {
  url: string;
  clientId: string;
  commandId: string;
  trackIds?: number[];
}

export type UnavailableReason = 'not_embeddable' | 'blocked' | 'preview_only' | 'not_streamable' | 'unknown';

export interface UnavailableItem {
  title: string | null;
  reason: UnavailableReason;
}

export interface ImportResult {
  added: number;
  collection: Collection | null;
  unavailable: UnavailableItem[];
  missing: number;
  revision: number;
  needsClientExpansion?: boolean;
  playlistUrl?: string;
}


export interface ApiErrorBody {
  error: string;
  message: string;
  unavailable?: UnavailableItem[];
}
