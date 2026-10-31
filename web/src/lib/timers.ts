import type { WorkerIn, WorkerOut } from './timer-worker';

/** Cancels a scheduled callback; safe to call more than once. */
export type Cancel = () => void;

const worker = new Worker(new URL('./timer-worker.ts', import.meta.url), { type: 'module' });
const callbacks = new Map<number, () => void>();
const sleepListeners = new Set<(gap: number) => void>();
let nextId = 1;

worker.onmessage = (e: MessageEvent<WorkerOut>) => {
  const msg = e.data;
  if (msg.type === 'sleep') {
    for (const fn of sleepListeners) fn(msg.gap);
    return;
  }
  const fn = callbacks.get(msg.id);
  callbacks.delete(msg.id);
  fn?.();
};

function post(msg: WorkerIn): void {
  worker.postMessage(msg);
}

/** Runs `fn` at client time `at` (Date.now() scale). */
export function at(time: number, fn: () => void): Cancel {
  const id = nextId++;
  callbacks.set(id, fn);
  post({ type: 'at', id, at: time });
  return () => {
    if (callbacks.delete(id)) post({ type: 'cancel', id });
  };
}

export function after(ms: number, fn: () => void): Cancel {
  return at(Date.now() + ms, fn);
}

/** Repeats `fn` every `ms` until cancelled. */
export function every(ms: number, fn: () => void): Cancel {
  let cancel: Cancel = () => {};
  let stopped = false;
  const loop = () => {
    cancel = after(ms, () => {
      if (stopped) return;
      loop();
      fn();
    });
  };
  loop();
  return () => {
    stopped = true;
    cancel();
  };
}

export function onSleep(fn: (gap: number) => void): Cancel {
  sleepListeners.add(fn);
  return () => sleepListeners.delete(fn);
}
