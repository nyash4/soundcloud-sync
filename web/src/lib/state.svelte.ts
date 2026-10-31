import { ApiError, importLink as postImport } from './api';
import { expandPlaylist } from './expand';
import { Connection, type ConnStatus } from './net';
import { Player, type PlayerView } from './player';
import type { Ack, Entry, ImportResult, NoticeLevel, Participant, RoomState, UnavailableItem } from './protocol';
import { loadVolume, saveRoomCreds, saveVolume, type RoomCreds } from './storage';

export type TransportCommand = 'play' | 'pause' | 'next';

export interface Toast {
  id: number;
  level: NoticeLevel;
  text: string;
}

export interface ImportView {
  busy: boolean;
  progress: string | null;
  result: ImportResult | null;
  error: { message: string; unavailable: UnavailableItem[] } | null;
}

const TOAST_MS = { info: 6000, warn: 10_000 } as const;
const MAX_TOASTS = 4;
const LOST_COMMAND = 'Связь прерывалась — команда могла не дойти. Проверьте состояние и повторите.';

class AppState {
  roomId = $state<string | null>(null);
  creds = $state<RoomCreds | null>(null);
  conn = $state<ConnStatus>('connecting');
  fatal = $state<{ error: string; message: string } | null>(null);
  selfId = $state<string | null>(null);
  room = $state.raw<RoomState | null>(null);
  player = $state.raw<PlayerView>({
    playbackId: null,
    mode: 'idle',
    sample: null,
    durationMs: null,
    volume: loadVolume(),
    joining: false,
  });
  pending = $state<Record<TransportCommand, boolean>>({ play: false, pause: false, next: false });
  /** Queue order shown while a move is in flight, so the row doesn't jump back. */
  queueOverride = $state.raw<Entry[] | null>(null);
  removing = $state.raw<ReadonlySet<string>>(new Set());
  toasts = $state<Toast[]>([]);
  importView = $state<ImportView>({ busy: false, progress: null, result: null, error: null });

  me = $derived<Participant | null>(this.room?.participants.find((p) => p.id === this.selfId) ?? null);
  queue = $derived<Entry[]>(this.queueOverride ?? this.room?.queue ?? []);

}

export const app = new AppState();

let net: Connection | null = null;
let player: Player | null = null;
let toastSeq = 0;
/** Ack revisions a pending transport button waits for in `state`. */
const awaitedRevision: Partial<Record<TransportCommand, number>> = {};
let overrideUntil: number | null = null;

function toast(level: NoticeLevel, text: string): void {
  const id = ++toastSeq;
  app.toasts = [...app.toasts, { id, level, text }].slice(-MAX_TOASTS);
  setTimeout(() => dismissToast(id), TOAST_MS[level]);
}

export function dismissToast(id: number): void {
  app.toasts = app.toasts.filter((t) => t.id !== id);
}

// Every snapshot is accepted as-is: after a server restart revisions can go backwards.
function applyState(state: RoomState, selfId: string | null, afterWelcome: boolean): void {
  app.room = state;
  if (afterWelcome) {
    // Waits keyed to pre-reconnect revisions are meaningless now; commands still in flight
    // are replayed by the connection and settle through their own acks.
    for (const name of Object.keys(awaitedRevision) as TransportCommand[]) {
      delete awaitedRevision[name];
      app.pending[name] = false;
    }
    overrideUntil = null;
    app.queueOverride = null;
  }
  for (const name of Object.keys(awaitedRevision) as TransportCommand[]) {
    if (state.revision >= awaitedRevision[name]!) {
      delete awaitedRevision[name];
      app.pending[name] = false;
    }
  }
  if (overrideUntil !== null && state.revision >= overrideUntil) {
    overrideUntil = null;
    app.queueOverride = null;
  }
  if (app.removing.size) {
    const ids = new Set(state.queue.map((e) => e.id));
    app.removing = new Set([...app.removing].filter((id) => ids.has(id)));
  }
  player?.sync(state, selfId, afterWelcome);
}

export function enterRoom(roomId: string, creds: RoomCreds): void {
  leaveRoom();
  app.roomId = roomId;
  app.creds = creds;
  app.fatal = null;
  player = new Player(
    (msg) => net?.report(msg),
    (view) => (app.player = view),
    app.player.volume,
  );
  net = new Connection(
    { roomId, ...creds },
    {
      welcome(selfId, state) {
        app.selfId = selfId;
        applyState(state, selfId, true);
      },
      state: (state) => applyState(state, app.selfId, false),
      notice: (level, text) => toast(level, text),
      fatal(error, message) {
        app.fatal = { error, message };
        player?.destroy();
        player = null;
      },
      status: (status) => (app.conn = status),
    },
  );
  net.start();
}

export function leaveRoom(): void {
  net?.stop();
  player?.destroy();
  net = null;
  player = null;
  app.room = null;
  app.selfId = null;
  app.conn = 'connecting';
  app.queueOverride = null;
  app.removing = new Set();
  app.importView = { busy: false, progress: null, result: null, error: null };
  for (const key of Object.keys(app.pending) as TransportCommand[]) app.pending[key] = false;
}

/** Reports an ack failure; returns true when the command went through. */
function settled(ack: Ack | null): ack is Ack & { ok: true } {
  if (!ack) {
    toast('warn', LOST_COMMAND);
    return false;
  }
  if (!ack.ok) {
    toast('warn', ack.message || 'Команда не выполнена.');
    return false;
  }
  return true;
}

export async function transport(name: TransportCommand): Promise<void> {
  if (!net || app.pending[name]) return;
  app.pending[name] = true;
  const ack =
    name === 'next' ? await net.command('next', { fromEntryId: app.room?.current?.id ?? null }) : await net.command(name, {});
  // The button stays pending until the state carrying this change has arrived.
  if (settled(ack) && (app.room?.revision ?? -1) < ack.revision) {
    awaitedRevision[name] = ack.revision;
  } else {
    app.pending[name] = false;
  }
}

function reorder(queue: Entry[], entryId: string, afterId: string | null): Entry[] {
  const moving = queue.find((e) => e.id === entryId);
  if (!moving) return queue;
  const rest = queue.filter((e) => e.id !== entryId);
  const index = afterId === null ? 0 : rest.findIndex((e) => e.id === afterId) + 1;
  rest.splice(index, 0, moving);
  return rest;
}

export async function moveEntry(entryId: string, afterId: string | null): Promise<void> {
  const room = app.room;
  if (!net || !room || entryId === afterId) return;
  app.queueOverride = reorder(app.queue, entryId, afterId);
  overrideUntil = Infinity;
  const ack = await net.command('queue.move', { entryId, afterId }, room.revision);
  if (settled(ack) && (app.room?.revision ?? -1) < ack.revision) {
    overrideUntil = ack.revision;
  } else {
    overrideUntil = null;
    app.queueOverride = null;
  }
}

export async function removeEntry(entryId: string): Promise<void> {
  if (!net) return;
  app.removing = new Set([...app.removing, entryId]);
  const ack = await net.command('queue.remove', { entryId });
  // not_found means someone else removed it first — the result is the same.
  if (!(ack && !ack.ok && ack.error === 'not_found') && !settled(ack)) {
    app.removing = new Set([...app.removing].filter((id) => id !== entryId));
  }
}

export async function rename(name: string): Promise<boolean> {
  const roomId = app.roomId;
  const creds = app.creds;
  if (!net || !roomId || !creds) return false;
  const ack = await net.command('rename', { name });
  if (!settled(ack)) return false;
  app.creds = { ...creds, name };
  saveRoomCreds(roomId, app.creds);
  net.setName(name);
  return true;
}

export function setVolume(volume: number): void {
  saveVolume(volume);
  if (player) player.setVolume(volume);
  else app.player = { ...app.player, volume };
}

export async function seekTrack(ms: number): Promise<void> {
  const playbackId = app.room?.playback.playbackId;
  if (!net || app.conn !== 'open' || !playbackId || !Number.isFinite(ms)) return;
  settled(await net.command('seek', { playbackId, ms: Math.max(0, Math.round(ms)) }));
}

export async function importUrl(url: string): Promise<void> {
  const roomId = app.roomId;
  const creds = app.creds;
  if (!roomId || !creds || app.importView.busy) return;
  const iv = app.importView;
  iv.busy = true;
  iv.progress = 'Читаем ссылку…';
  iv.result = null;
  iv.error = null;
  try {
    const request = { url, clientId: creds.clientId };
    let result = await postImport(roomId, creds.token, { ...request, commandId: crypto.randomUUID() });
    if (result.needsClientExpansion && result.playlistUrl) {
      iv.progress = 'Открываем коллекцию в плеере…';
      let trackIds: number[];
      try {
        trackIds = await expandPlaylist(result.playlistUrl, (n) => {
          iv.progress = `Читаем коллекцию через плеер SoundCloud… ${n}`;
        });
      } catch {
        throw new ApiError('expand_failed', 'Не удалось прочитать состав коллекции через плеер SoundCloud.');
      }
      iv.progress = `Добавляем ${trackIds.length}…`;
      // A fresh commandId: the first request is already recorded as "needs expansion".
      result = await postImport(roomId, creds.token, { ...request, commandId: crypto.randomUUID(), trackIds });
    }
    iv.result = result;
  } catch (e) {
    iv.error =
      e instanceof ApiError
        ? { message: e.message, unavailable: e.unavailable }
        : { message: 'Не удалось добавить ссылку.', unavailable: [] };
  } finally {
    iv.busy = false;
    iv.progress = null;
  }
}

export function clearImportResult(): void {
  app.importView.result = null;
  app.importView.error = null;
}
