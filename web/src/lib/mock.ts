// Dev-only (`/r/<id>?mock=playing|join|blocked|waiting|empty|reconnecting`): renders the room
// without a server. Loaded through a dynamic import guarded by import.meta.env.DEV.

import type { Entry, Participant, RoomState } from './protocol';
import { app } from './state.svelte';

const soulhack = { title: 'Soulhack', kind: 'album' } as const;

function entry(id: string, title: string, artist: string, sec: number, addedBy: string, album = false): Entry {
  return {
    id,
    trackId: Number(id.replace(/\D/g, '')) || 1,
    widgetUrl: `https://api.soundcloud.com/tracks/${id}`,
    permalink: 'https://soundcloud.com/forss/flickermood',
    title,
    artist,
    durationMs: sec * 1000,
    artwork: null,
    collection: album ? soulhack : null,
    addedBy,
  };
}

export function applyMock(roomId: string, variant: string): void {
  const now = Date.now();
  const me: Participant = { id: 'u1', name: 'Ника', online: true, status: 'listening', outReason: null };
  const room: RoomState = {
    roomId,
    revision: 42,
    serverTime: now,
    participants: [
      me,
      { id: 'u2', name: 'Саша', online: true, status: 'listening', outReason: null },
      { id: 'u3', name: 'Тимофей', online: false, status: 'offline', outReason: null },
      { id: 'u4', name: 'Ваня', online: true, status: 'out', outReason: 'ready_timeout' },
    ],
    current: entry('e1', 'Flickermood', 'Forss', 213, 'Саша', true),
    queue: [
      entry('e2', 'Pleasure Is All Mine', 'Forss', 248, 'Саша', true),
      entry('e3', 'Journeyman', 'Forss', 301, 'Саша', true),
      entry('e4', 'Pandora’s Box (extended version with a very long title)', 'Forss', 289, 'Ника', true),
      entry('e5', 'Habit', 'Lusine', 262, 'Ника'),
      entry('e6', 'Two Dots', 'Lusine', 333, 'Тимофей'),
    ],
    history: [entry('e0', 'Pattern Recognition', 'Lusine', 297, 'Ника')],
    playback: {
      phase: 'playing',
      paused: false,
      playbackId: 'p17',
      startMs: 0,
      startAt: now - 84_000,
      anchor: { at: now - 84_000, ms: 0 },
      readyDeadline: null,
      endDeadline: null,
    },
  };
  let mode: typeof app.player.mode = 'playing';

  switch (variant) {
    case 'join':
      me.status = 'out';
      me.outReason = 'joined_late';
      mode = 'idle';
      break;
    case 'blocked':
      room.playback = { ...room.playback, phase: 'preparing', startAt: null, anchor: null, readyDeadline: now + 14_000 };
      me.status = 'blocked';
      mode = 'blocked';
      break;
    case 'waiting':
      room.playback = { ...room.playback, phase: 'waiting_end', endDeadline: now + 23_000 };
      me.status = 'waiting';
      mode = 'ended';
      break;
    case 'empty':
      room.current = null;
      room.queue = [];
      room.history = [];
      room.playback = { ...room.playback, phase: 'idle', playbackId: null, startAt: null, anchor: null };
      room.participants = [me];
      me.status = 'idle';
      mode = 'idle';
      break;
  }

  app.roomId = roomId;
  app.creds = { token: 'mock', clientId: 'mock', name: me.name };
  app.selfId = me.id;
  app.conn = variant === 'reconnecting' ? 'reconnecting' : 'open';
  app.room = room;
  app.player = {
    playbackId: room.playback.playbackId,
    mode,
    sample: mode === 'playing' ? { ms: 84_000, at: now } : mode === 'ended' ? { ms: 213_000, at: now } : null,
    durationMs: room.current?.durationMs ?? null,
    volume: app.player.volume,
    joining: false,
  };
  if (variant === 'playing') {
    app.importView.result = {
      added: 4,
      collection: soulhack,
      unavailable: [{ title: 'Soulhack (radio edit)', reason: 'not_embeddable' }],
      missing: 1,
      revision: 42,
    };
  }
}
