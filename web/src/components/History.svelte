<script lang="ts">
  import { app } from '../lib/state.svelte';
  import Glyph from './Glyph.svelte';

  let open = $state(false);
  const played = $derived([...(app.room?.history ?? [])].reverse());
</script>

{#if played.length}
  <section class="history" aria-label="Сыграно">
    <button class="toggle mono" aria-expanded={open} aria-controls="history-list" onclick={() => (open = !open)}>
      <span>сыграно · {played.length}</span>
      <span class="chev" class:open><Glyph name="down" size={12} /></span>
    </button>
    {#if open}
      <ol id="history-list">
        {#each played as entry (entry.id)}
          <li>
            <a href={entry.permalink} target="_blank" rel="noopener noreferrer" class="title">{entry.title}</a>
            <span class="muted artist">{entry.artist}</span>
          </li>
        {/each}
      </ol>
    {/if}
  </section>
{/if}

<style>
  .history {
    margin-top: 40px;
  }

  .toggle {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    padding: 10px 0;
    border: 0;
    border-top: 1px solid var(--rule);
    background: none;
    color: var(--muted);
    text-align: left;
  }

  .toggle:hover {
    color: var(--ink);
  }

  .chev {
    transition: transform var(--fast);
  }

  .chev.open {
    transform: rotate(180deg);
  }

  ol {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 13px;
  }

  li {
    display: flex;
    gap: 12px;
    padding: 6px 0;
    border-top: 1px solid var(--rule);
    min-width: 0;
  }

  .title,
  .artist {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    text-decoration-color: transparent;
  }

  .title:hover {
    text-decoration-color: currentColor;
  }

  .artist {
    flex: none;
    max-width: 40%;
    margin-left: auto;
  }
</style>
