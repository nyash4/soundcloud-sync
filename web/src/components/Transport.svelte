<script lang="ts">
  import { app, setVolume, transport } from '../lib/state.svelte';
  import Glyph from './Glyph.svelte';
  import Progress from './Progress.svelte';

  const pb = $derived(app.room?.playback ?? null);
  const cur = $derived(app.room?.current ?? null);
  /** The button reflects only what the server says; it never flips on its own. */
  const running = $derived(!!pb && pb.phase !== 'idle' && !pb.paused);
  const togglePending = $derived(app.pending.play || app.pending.pause);
  const canToggle = $derived(running || !!cur || app.queue.length > 0);
</script>

<section class="transport" aria-label="Управление">
  <Progress />

  <div class="controls">
    <button
      class="square primary"
      aria-label={running ? 'Пауза для всех' : 'Играть для всех'}
      aria-busy={togglePending}
      disabled={!canToggle}
      onclick={() => transport(running ? 'pause' : 'play')}
    >
      <Glyph name={running ? 'pause' : 'play'} size={18} />
      {#if togglePending}<span class="pending" aria-hidden="true"></span>{/if}
    </button>
    <button
      class="square ghost"
      aria-label="Следующий трек для всех"
      aria-busy={app.pending.next}
      disabled={!cur}
      onclick={() => transport('next')}
    >
      <Glyph name="next" size={16} />
      {#if app.pending.next}<span class="pending" aria-hidden="true"></span>{/if}
    </button>

    <div class="volume">
      <label class="mono muted" for="volume">звук</label>
      <input
        id="volume"
        type="range"
        min="0"
        max="100"
        step="1"
        value={app.player.volume}
        style:--pct="{app.player.volume}%"
        oninput={(e) => setVolume(Number(e.currentTarget.value))}
      />
      <span class="mono value">{app.player.volume}</span>
    </div>
  </div>
</section>

<style>
  .transport {
    margin-top: 24px;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 20px;
  }

  .square {
    position: relative;
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    padding: 0;
    border-radius: 2px;
    transition:
      opacity var(--fast),
      background-color var(--fast),
      border-color var(--fast);
  }

  .primary {
    border: 1px solid var(--ink);
    background: var(--ink);
    color: var(--bg);
  }

  .primary:hover:not(:disabled) {
    opacity: 0.86;
  }

  .ghost {
    border: 1px solid var(--rule);
    background: transparent;
    color: var(--ink);
  }

  .ghost:hover:not(:disabled) {
    border-color: var(--ink);
  }

  .square:disabled {
    opacity: 0.35;
  }

  .square[aria-busy='true'] {
    cursor: progress;
  }

  /* Waiting for the server to confirm: a short accent bar sweeping along the bottom edge. */
  .pending {
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 2px;
    background: var(--accent);
    transform-origin: left;
    animation: sweep 0.9s ease-in-out infinite alternate;
  }

  @keyframes sweep {
    from {
      transform: scaleX(0.15);
    }
    to {
      transform: scaleX(1);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pending {
      animation: none;
    }
  }

  .volume {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 0 1 240px;
    min-width: 0;
    margin-left: auto;
    padding-left: 16px;
  }

  .value {
    min-width: 3ch;
    text-align: right;
  }

  input[type='range'] {
    flex: 1;
    min-width: 48px;
    height: 20px;
    margin: 0;
    background: transparent;
    appearance: none;
  }

  input[type='range']::-webkit-slider-runnable-track {
    height: 1px;
    background: linear-gradient(to right, var(--ink) var(--pct), var(--rule) var(--pct));
  }

  input[type='range']::-webkit-slider-thumb {
    width: 10px;
    height: 10px;
    margin-top: -4.5px;
    border: 0;
    border-radius: 0;
    background: var(--ink);
    appearance: none;
  }

  input[type='range']::-moz-range-track {
    height: 1px;
    background: var(--rule);
  }

  input[type='range']::-moz-range-progress {
    height: 1px;
    background: var(--ink);
  }

  input[type='range']::-moz-range-thumb {
    width: 10px;
    height: 10px;
    border: 0;
    border-radius: 0;
    background: var(--ink);
  }
</style>
