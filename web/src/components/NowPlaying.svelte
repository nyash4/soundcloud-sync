<script lang="ts">
  import { app } from '../lib/state.svelte';
  import Artwork from './Artwork.svelte';
  import SectionHead from './SectionHead.svelte';

  const cur = $derived(app.room?.current ?? null);
  const pb = $derived(app.room?.playback ?? null);

  const phase = $derived.by(() => {
    if (!pb || !cur || pb.phase === 'idle') return { text: 'тишина', live: false };
    if (pb.paused) return { text: 'пауза', live: false };
    if (pb.phase === 'preparing') return { text: 'подготовка', live: false };
    if (pb.phase === 'waiting_end') return { text: 'дослушиваем', live: true };
    return { text: 'играет', live: true };
  });
</script>

<section aria-labelledby="now-title">
  <SectionHead label="сейчас">
    {#snippet aside()}
      <span class="phase" class:live={phase.live}>{phase.text}</span>
    {/snippet}
  </SectionHead>

  {#if cur}
    <div class="art"><Artwork src={cur.artwork} artist={cur.artist} size="large" /></div>
    <div class="meta">
      <h1 id="now-title" class="title">
        <a href={cur.permalink} target="_blank" rel="noopener noreferrer">{cur.title}</a>
      </h1>
      <p class="artist">{cur.artist}</p>
      {#if cur.collection}
        <p class="collection">{cur.collection.kind === 'album' ? 'альбом' : 'плейлист'} «{cur.collection.title}»</p>
      {/if}
      <p class="credits mono">
        <span class="added">добавлено: {cur.addedBy}</span>
        <a href={cur.permalink} target="_blank" rel="noopener noreferrer">soundcloud ↗</a>
      </p>
    </div>
  {:else}
    <div class="art empty" aria-hidden="true"></div>
    <div class="meta">
      <h1 id="now-title" class="title quiet">Ничего не играет</h1>
      <p class="collection">
        {app.queue.length ? 'Нажмите «Играть», чтобы начать очередь.' : 'Добавьте музыку в очередь.'}
      </p>
    </div>
  {/if}
</section>

<style>
  .phase {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .phase.live::before {
    content: '';
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }

  .art {
    margin: 8px 0 20px;
  }

  .empty {
    width: 100%;
    max-width: 320px;
    aspect-ratio: 1;
    border: 1px solid var(--rule);
    background: var(--fill);
  }

  .title {
    font-size: 20px;
    line-height: 1.3;
    letter-spacing: -0.005em;
    overflow-wrap: anywhere;
  }

  .title a {
    text-decoration-color: transparent;
    transition: text-decoration-color var(--fast);
  }

  .title a:hover {
    text-decoration-color: currentColor;
  }

  .quiet {
    color: var(--muted);
  }

  .artist {
    margin-top: 2px;
    overflow-wrap: anywhere;
  }

  .collection {
    margin-top: 2px;
    font-size: 13px;
    color: var(--muted);
  }

  .credits {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    margin-top: 12px;
    color: var(--muted);
  }

  .added {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .credits a {
    flex: none;
    text-decoration: none;
  }

  .credits a:hover {
    color: var(--ink);
    text-decoration: underline;
  }

  /* Single-column layout: artwork beside the text instead of above it. */
  @media (max-width: 959px) and (min-width: 600px) {
    section {
      display: grid;
      grid-template-columns: 200px minmax(0, 1fr);
      column-gap: 28px;
      align-items: end;
    }

    section > :global(:first-child) {
      grid-column: 1 / -1;
    }

    .art {
      margin-bottom: 0;
    }
  }

  @media (max-width: 599px) {
    .art,
    .empty {
      max-width: 240px;
    }
  }
</style>
