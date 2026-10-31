// Server clock estimate (§5): offset = serverTime - (t0 + t1) / 2, taken from the
// sample with the smallest round trip among the last ten.

const WINDOW = 10;

interface Sample {
  rtt: number;
  offset: number;
}

let samples: Sample[] = [];
let offset = 0;

/** Starts a fresh estimate, e.g. after reconnecting (the machine may have slept). */
export function resetClock(roughServerTime: number): void {
  samples = [];
  offset = roughServerTime - Date.now();
}

export function addClockSample(t0: number, serverTime: number, t1: number = Date.now()): void {
  if (t1 < t0) return;
  samples.push({ rtt: t1 - t0, offset: serverTime - (t0 + t1) / 2 });
  if (samples.length > WINDOW) samples.shift();
  offset = samples.reduce((best, s) => (s.rtt < best.rtt ? s : best)).offset;
}

export function serverNow(): number {
  return Date.now() + offset;
}

/** Converts a local Date.now() timestamp to server time. */
export function toServerTime(clientTime: number): number {
  return clientTime + offset;
}

/** Converts a server timestamp to the local Date.now() scale. */
export function toClientTime(serverTime: number): number {
  return serverTime - offset;
}
