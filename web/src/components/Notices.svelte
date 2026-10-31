<script lang="ts">
  import { fly } from 'svelte/transition';
  import { app, dismissToast } from '../lib/state.svelte';
  import Glyph from './Glyph.svelte';

  const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
</script>

<div class="notices" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="notice" class:warn={t.level === 'warn'} transition:fly={{ y: 8, duration: reduced ? 0 : 140 }}>
      <p>{t.text}</p>
      <button aria-label="Закрыть уведомление" onclick={() => dismissToast(t.id)}>
        <Glyph name="remove" size={12} />
      </button>
    </div>
  {/each}
</div>

<style>
  .notices {
    position: fixed;
    right: var(--gutter);
    bottom: var(--gutter);
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(360px, calc(100vw - 2 * var(--gutter)));
    pointer-events: none;
  }

  .notice {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px 12px 12px 16px;
    border: 1px solid var(--ink);
    border-radius: 2px;
    background: var(--bg);
    font-size: 13px;
    pointer-events: auto;
  }

  .warn {
    border-left: 3px solid var(--accent);
  }

  p {
    flex: 1;
  }

  button {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--muted);
  }

  button:hover {
    color: var(--ink);
  }
</style>
