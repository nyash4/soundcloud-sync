// localStorage access, tolerant of private mode / blocked storage.

export interface RoomCreds {
  token: string;
  clientId: string;
  name: string;
}

const roomKey = (roomId: string) => `scs:room:${roomId}`;
const NAME_KEY = 'scs:name';
const VOLUME_KEY = 'scs:volume';

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable: the session still works, it just won't be remembered.
  }
}

export function loadRoomCreds(roomId: string): RoomCreds | null {
  const raw = read(roomKey(roomId));
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as Partial<RoomCreds>;
    if (typeof v.token !== 'string' || typeof v.clientId !== 'string') return null;
    return { token: v.token, clientId: v.clientId, name: typeof v.name === 'string' ? v.name : '' };
  } catch {
    return null;
  }
}

export function saveRoomCreds(roomId: string, creds: RoomCreds): void {
  write(roomKey(roomId), JSON.stringify(creds));
  if (creds.name) write(NAME_KEY, creds.name);
}

/** Last name used in any room — only a default for the name field. */
export function lastName(): string {
  return read(NAME_KEY) ?? '';
}

export function loadVolume(): number {
  const v = Number(read(VOLUME_KEY));
  return read(VOLUME_KEY) !== null && Number.isFinite(v) ? Math.min(100, Math.max(0, Math.round(v))) : 10;
}

export function saveVolume(v: number): void {
  write(VOLUME_KEY, String(v));
}
