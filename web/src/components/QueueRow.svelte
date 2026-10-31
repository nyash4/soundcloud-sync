<script lang="ts">
  import { formatTime } from '../lib/format';
  import type { Entry } from '../lib/protocol';
  import Artwork from './Artwork.svelte';
  import Glyph from './Glyph.svelte';

  interface Props {
    entry: Entry;
    index: number;
    last: boolean;
    removing: boolean;
    dragging: boolean;
    dropBefore: boolean;
    dropAfter: boolean;
    ondragstart: (e: DragEvent) => void;
    ondragover: (e: DragEvent) => void;
    ondragend: () => void;
    onup: () => void;
    ondown: () => void;
    onremove: () => void;
  }

  let { entry, index, last, removing, dragging, dropBefore, dropAfter, ...on }: Props = $props();
</script>

<li
  class="row"
  class:removing
  class:dragging
  class:drop-before={dropBefore}
  class:drop-after={dropAfter}
  data-entry={entry.id}
  draggable={!removing}
  ondragstart={on.ondragstart}
  ondragover={on.ondragover}
  ondragend={on.ondragend}
>
  <span class="idx mono" aria-hidden="true">
    <span class="num">{String(index + 1).padStart(2, '0')}</span>
    <span class="grip"><Glyph name="grip" size={12} /></span>
  </span>
  <Artwork src={entry.artwork} artist={entry.artist} size="small" />
  <div class="text">
    <span class="title">{entry.title}</span>
    <span class="sub">
      {entry.artist}{#if entry.collection}<span class="muted">{` · ${entry.collection.title}`}</span>{/if}
    </span>
  </div>
  <div class="meta">
    {#if entry.durationMs !== null}<span class="mono">{formatTime(entry.durationMs)}</span>{/if}
    <span class="by" title="Добавлено: {entry.addedBy}">{entry.addedBy}</span>
  </div>
  <div class="controls">
    <button data-control="up" aria-label="Выше: {entry.title}" disabled={index === 0 || removing} onclick={on.onup}>
      <Glyph name="up" size={14} />
    </button>
    <button data-control="down" aria-label="Ниже: {entry.title}" disabled={last || removing} onclick={on.ondown}>
      <Glyph name="down" size={14} />
    </button>
    <button data-control="remove" aria-label="Убрать из очереди: {entry.title}" disabled={removing} onclick={on.onremove}>
      <Glyph name="remove" size={14} />
    </button>
  </div>
</li>

<style>
  .row {
    position: relative;
    display: grid;
    grid-template-columns: 24px 40px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 14px;
    padding: 10px 0;
    border-top: 1px solid var(--rule);
    cursor: grab;
    transition: opacity var(--fast);
  }

  .row:last-child {
    border-bottom: 1px solid var(--rule);
  }

  .row.dragging {
    opacity: 0.4;
  }

  .row.removing {
    opacity: 0.35;
    cursor: default;
  }

  /* Insertion marker while dragging. */
  .drop-before::before,
  .drop-after::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: var(--accent);
  }

  .drop-before::before {
    top: -1px;
  }

  .drop-after::after {
    bottom: -1px;
  }

  .idx {
    position: relative;
    color: var(--muted);
  }

  .grip {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    opacity: 0;
  }

  .num {
    transition: opacity var(--fast);
  }

  .row:hover .grip {
    opacity: 1;
  }

  .row:hover .num {
    opacity: 0;
  }

  .text,
  .meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title,
  .sub,
  .by {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    font-weight: 500;
  }

  .sub {
    font-size: 13px;
  }

  .meta {
    align-items: flex-end;
    text-align: right;
  }

  .by {
    max-width: 14ch;
    font-size: 13px;
    color: var(--muted);
  }

  .controls {
    display: flex;
    opacity: 0;
    transition: opacity var(--fast);
  }

  .row:hover .controls,
  .row:focus-within .controls {
    opacity: 1;
  }

  @media (hover: none) {
    .controls {
      opacity: 1;
    }
  }

  .controls button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: 2px;
    background: none;
    color: var(--muted);
  }

  .controls button:hover:not(:disabled) {
    color: var(--ink);
    background: var(--fill);
  }

  .controls button:disabled {
    opacity: 0.3;
  }

  @media (max-width: 560px) {
    .row {
      grid-template-columns: 40px minmax(0, 1fr) auto;
      grid-template-areas:
        'art text meta'
        'art text controls';
      row-gap: 4px;
    }

    .idx {
      display: none;
    }

    .row > :global(.art) {
      grid-area: art;
    }

    .text {
      grid-area: text;
    }

    .meta {
      grid-area: meta;
    }

    .controls {
      grid-area: controls;
      justify-content: flex-end;
    }

    .by {
      display: none;
    }
  }
</style>
