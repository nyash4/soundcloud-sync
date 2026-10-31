// Dedicated-worker timers: unlike main-thread timers they are not clamped in
// background tabs, so the shared start and the clock pings stay on time.

export type WorkerIn = { type: 'at'; id: number; at: number } | { type: 'cancel'; id: number };
export type WorkerOut = { type: 'fire'; id: number } | { type: 'sleep'; gap: number };

interface WorkerScope {
  postMessage(msg: WorkerOut): void;
  onmessage: ((e: MessageEvent<WorkerIn>) => void) | null;
}

const scope = self as unknown as WorkerScope;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

const TICK_MS = 1000;
const SLEEP_GAP_MS = 10_000;

function arm(id: number, at: number): void {
  const delay = at - Date.now();
  if (delay <= 0) {
    timers.delete(id);
    scope.postMessage({ type: 'fire', id });
    return;
  }
  // Re-arm on wake-up so an early or coarse timeout never fires before `at`.
  timers.set(id, setTimeout(() => arm(id, at), delay));
}

scope.onmessage = (e) => {
  const msg = e.data;
  const existing = timers.get(msg.id);
  if (existing !== undefined) clearTimeout(existing);
  timers.delete(msg.id);
  if (msg.type === 'at') arm(msg.id, msg.at);
};

// A long gap between ticks means the machine slept or the tab was frozen.
let lastTick = Date.now();
setInterval(() => {
  const now = Date.now();
  const gap = now - lastTick;
  lastTick = now;
  if (gap > SLEEP_GAP_MS) scope.postMessage({ type: 'sleep', gap });
}, TICK_MS);
