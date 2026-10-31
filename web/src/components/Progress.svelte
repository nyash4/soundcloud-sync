<script lang="ts">
  import { toServerTime } from '../lib/clock';
  import { anchorPosition, formatTime } from '../lib/format';
  import { app, seekTrack } from '../lib/state.svelte';

  const pb = $derived(app.room?.playback ?? null);
  const cur = $derived(app.room?.current ?? null);
  const view = $derived(app.player);

  /** Our own widget's position when it is part of this playback; otherwise the room timeline. */
  const local = $derived(
    !!view.sample && view.playbackId === pb?.playbackId && ['playing', 'paused', 'ended'].includes(view.mode),
  );
  const moving = $derived(
    local ? view.mode === 'playing' : !!pb && !!pb.anchor && !pb.paused && pb.phase !== 'preparing',
  );

  let now = $state(Date.now());
  let draft = $state<{ playbackId: string; ms: number } | null>(null);

  $effect(() => {
    now = Date.now();
    if (!moving) return;
    let frame = requestAnimationFrame(function tick() {
      now = Date.now();
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  });

  const duration = $derived(view.playbackId === pb?.playbackId ? view.durationMs : (cur?.durationMs ?? null));

  const position = $derived.by(() => {
    if (!cur || !pb || pb.phase === 'idle') return null;
    let ms: number;
    if (local && view.sample) {
      ms = view.mode === 'playing' ? view.sample.ms + (now - view.sample.at) : view.sample.ms;
    } else {
      ms = anchorPosition(pb, toServerTime(now)) ?? pb.startMs;
    }
    return Math.max(0, duration !== null ? Math.min(ms, duration) : ms);
  });

  const shownPosition = $derived(draft?.playbackId === pb?.playbackId ? draft?.ms ?? position : position);
  const ratio = $derived(shownPosition !== null && duration ? shownPosition / duration : 0);
  const canSeek = $derived(!!cur && !!pb?.playbackId && !!duration && app.conn === 'open');

  function commit(e: Event): void {
    const ms = Number((e.currentTarget as HTMLInputElement).value);
    draft = null;
    void seekTrack(ms);
  }
</script>

<div class="progress">
  <span class="mono time">{formatTime(shownPosition)}</span>
  <input
    class="track"
    type="range"
    min="0"
    max={duration ? Math.max(0, duration - 1) : 1}
    step="1"
    value={shownPosition ?? 0}
    disabled={!canSeek}
    aria-label="Перемотать трек для всех"
    aria-valuetext={shownPosition !== null ? `${formatTime(shownPosition)} из ${formatTime(duration)}` : 'ничего не играет'}
    style:--pct="{ratio * 100}%"
    oninput={(e) => { if (pb?.playbackId) draft = { playbackId: pb.playbackId, ms: Number(e.currentTarget.value) }; }}
    onchange={commit}
    onpointercancel={() => { draft = null; }}
    onkeydown={(e) => {
      if (!canSeek || !duration || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'PageUp', 'PageDown'].includes(e.key)) return;
      e.preventDefault();
      const direction = ['ArrowLeft', 'ArrowDown', 'PageDown'].includes(e.key) ? -1 : 1;
      const step = e.key.startsWith('Page') ? 30_000 : 5000;
      void seekTrack(Math.min(duration - 1, Math.max(0, (shownPosition ?? 0) + direction * step)));
    }}
  />
  <span class="mono time end">{formatTime(duration)}</span>
</div>

<style>
  .progress {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 12px;
  }

  .time {
    min-width: 4ch;
  }

  .end {
    text-align: right;
    color: var(--muted);
  }

  .track {
    width: 100%;
    min-width: 0;
    height: 24px;
    margin: 0;
    padding: 0;
    background: transparent;
    appearance: none;
    cursor: pointer;
  }

  .track:disabled {
    cursor: default;
  }

  .track::-webkit-slider-runnable-track {
    height: 2px;
    background: linear-gradient(to right, var(--accent) var(--pct), var(--rule) var(--pct));
  }

  .track::-webkit-slider-thumb {
    width: 10px;
    height: 10px;
    margin-top: -4px;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    appearance: none;
  }

  .track::-moz-range-track {
    height: 2px;
    background: var(--rule);
  }

  .track::-moz-range-progress {
    height: 2px;
    background: var(--accent);
  }

  .track::-moz-range-thumb {
    width: 10px;
    height: 10px;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
  }
</style>
