// Thin promise-based wrapper over the SoundCloud Widget API
// (https://developers.soundcloud.com/docs/api/html5-widget).

import { after, type Cancel } from './timers';

export interface SCSound {
  id?: number;
}

export interface ProgressData {
  currentPosition: number;
}

type EventKey = 'READY' | 'PLAY' | 'PAUSE' | 'FINISH' | 'PLAY_PROGRESS' | 'ERROR';

interface RawWidget {
  bind(event: string, fn: (data?: ProgressData) => void): void;
  load(url: string, options: Record<string, unknown>): void;
  play(): void;
  pause(): void;
  seekTo(ms: number): void;
  setVolume(volume: number): void;
  getPosition(cb: (ms: number) => void): void;
  getDuration(cb: (ms: number) => void): void;
  getSounds(cb: (sounds: SCSound[]) => void): void;
  getCurrentSound(cb: (sound: SCSound | null) => void): void;
}

interface SCGlobal {
  Widget: ((iframe: HTMLIFrameElement) => RawWidget) & { Events: Record<EventKey, string> };
}

declare global {
  interface Window {
    SC?: SCGlobal;
  }
}

export type WidgetEvent =
  | { type: 'PLAY' | 'PAUSE' | 'FINISH' | 'PLAY_PROGRESS'; data: ProgressData | undefined }
  | { type: 'ERROR' };

const API_URL = 'https://w.soundcloud.com/player/api.js';
const PLAYER_URL = 'https://w.soundcloud.com/player/';
const LOAD_TIMEOUT_MS = 15_000;
const GETTER_TIMEOUT_MS = 4000;

/** Background throttling is not evidence that a track is unavailable. */
function whenVisibleTimeout(ms: number, fn: () => void): Cancel {
  let cancel: Cancel;
  const arm = () => {
    const started = Date.now();
    cancel = after(ms, () => {
      // Give the widget time to respond after a frozen tab wakes up.
      if (document.hidden || Date.now() - started > ms + 2000) arm();
      else fn();
    });
  };
  arm();
  return () => cancel();
}

const OPTIONS = {
  auto_play: false,
  show_recommendations: false,
  single_active: false,
  buying: false,
  sharing: false,
  download: false,
  show_artwork: false,
  show_comments: false,
  show_playcount: false,
  show_user: false,
  visual: false,
};

let apiPromise: Promise<SCGlobal> | null = null;

function loadApi(): Promise<SCGlobal> {
  apiPromise ??= new Promise<SCGlobal>((resolve, reject) => {
    if (window.SC) return resolve(window.SC);
    const script = document.createElement('script');
    const fail = () => {
      cancel();
      apiPromise = null;
      script.remove();
      reject(new Error('SC API failed to load'));
    };
    const cancel = whenVisibleTimeout(LOAD_TIMEOUT_MS, fail);
    script.src = API_URL;
    script.async = true;
    script.onload = () => {
      cancel();
      if (window.SC) resolve(window.SC);
      else fail();
    };
    script.onerror = fail;
    document.head.append(script);
  });
  return apiPromise;
}

export class WidgetLoadError extends Error {}

/** One hidden widget iframe. Every load replaces what it plays; listeners survive loads. */
export class Widget {
  private raw: RawWidget | null = null;
  private iframe: HTMLIFrameElement | null = null;
  private loading: { resolve(): void; reject(e: Error): void } | null = null;
  private listener: (e: WidgetEvent) => void = () => {};
  private generation = 0;

  onEvent(fn: (e: WidgetEvent) => void): void {
    this.listener = fn;
  }

  /** Loads `url` and resolves once the widget accepts commands. */
  async load(url: string): Promise<void> {
    const generation = ++this.generation;
    const sc = await loadApi();
    if (generation !== this.generation) throw new WidgetLoadError('superseded');
    this.loading?.reject(new WidgetLoadError('superseded'));
    let pending: NonNullable<Widget['loading']>;
    const done = new Promise<void>((resolve, reject) => {
      const cancel = whenVisibleTimeout(LOAD_TIMEOUT_MS, () => {
        if (this.loading === pending) this.loading = null;
        reject(new WidgetLoadError('timeout'));
      });
      pending = {
        resolve: () => {
          cancel();
          resolve();
        },
        reject: (e) => {
          cancel();
          reject(e);
        },
      };
      this.loading = pending;
    });
    const settle = () => {
      if (this.loading !== pending) return;
      const l = pending;
      this.loading = null;
      l?.resolve();
    };
    if (!this.raw) {
      this.raw = this.mount(sc, url, settle);
    } else {
      this.raw.load(url, { ...OPTIONS, callback: settle });
    }
    return done;
  }

  play(): void {
    this.raw?.play();
  }
  pause(): void {
    this.raw?.pause();
  }
  seekTo(ms: number): void {
    this.raw?.seekTo(Math.max(0, Math.round(ms)));
  }
  setVolume(v: number): void {
    // Give quiet listening more slider travel; preserve fractional widget values.
    const level = Number.isFinite(v) ? Math.min(100, Math.max(0, v)) : 0;
    this.raw?.setVolume((level * level) / 100);
  }

  position(): Promise<number | null> {
    return this.get((cb) => this.raw?.getPosition(cb));
  }
  duration(): Promise<number | null> {
    return this.get((cb) => this.raw?.getDuration(cb));
  }
  sounds(): Promise<SCSound[] | null> {
    return this.get((cb) => this.raw?.getSounds(cb));
  }
  currentSound(): Promise<SCSound | null> {
    return this.get<SCSound | null>((cb) => this.raw?.getCurrentSound(cb)).then((s) => s ?? null);
  }

  destroy(): void {
    this.generation++;
    this.loading?.reject(new WidgetLoadError('destroyed'));
    this.loading = null;
    this.iframe?.remove();
    this.iframe = null;
    this.raw = null;
  }

  private mount(sc: SCGlobal, url: string, onReady: () => void): RawWidget {
    const params = new URLSearchParams({ url, ...Object.fromEntries(Object.entries(OPTIONS).map(([k, v]) => [k, String(v)])) });
    const iframe = document.createElement('iframe');
    iframe.src = `${PLAYER_URL}?${params}`;
    iframe.allow = 'autoplay';
    iframe.title = 'SoundCloud';
    iframe.tabIndex = -1;
    iframe.setAttribute('aria-hidden', 'true');
    // Rendered but invisible: display:none or off-screen frames get throttled or never start audio.
    iframe.className = 'sc-widget-frame';
    document.body.append(iframe);
    this.iframe = iframe;

    const raw = sc.Widget(iframe);
    const ev = sc.Widget.Events;
    raw.bind(ev.READY, onReady);
    raw.bind(ev.ERROR, () => {
      const l = this.loading;
      this.loading = null;
      l?.reject(new WidgetLoadError('error'));
      this.listener({ type: 'ERROR' });
    });
    for (const type of ['PLAY', 'PAUSE', 'FINISH', 'PLAY_PROGRESS'] as const) {
      raw.bind(ev[type], (data) => this.listener({ type, data }));
    }
    return raw;
  }

  private get<T>(call: (cb: (v: T) => void) => void): Promise<T | null> {
    return new Promise((resolve) => {
      if (!this.raw) return resolve(null);
      const cancel = whenVisibleTimeout(GETTER_TIMEOUT_MS, () => resolve(null));
      call((v) => {
        cancel();
        resolve(v);
      });
    });
  }
}
