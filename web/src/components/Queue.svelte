<script lang="ts">
  import { tick } from 'svelte';
  import { formatTime } from '../lib/format';
  import { app, moveEntry, removeEntry } from '../lib/state.svelte';
  import QueueRow from './QueueRow.svelte';
  import SectionHead from './SectionHead.svelte';

  const queue = $derived(app.queue);
  const total = $derived(queue.reduce((sum, e) => sum + (e.durationMs ?? 0), 0));

  let dragId = $state<string | null>(null);
  /** Insertion point while dragging: the dragged row would land before index `drop`. */
  let drop = $state<number | null>(null);

  /** Moves `entryId` so it lands before the row currently at `index` (in the full list). */
  function moveTo(entryId: string, index: number) {
    const from = queue.findIndex((e) => e.id === entryId);
    if (from < 0 || index === from || index === from + 1) return;
    const rest = queue.filter((e) => e.id !== entryId);
    const at = index > from ? index - 1 : index;
    void moveEntry(entryId, at === 0 ? null : rest[at - 1]!.id);
  }

  async function step(entryId: string, delta: -1 | 1, control: 'up' | 'down') {
    const from = queue.findIndex((e) => e.id === entryId);
    moveTo(entryId, delta < 0 ? from - 1 : from + 2);
    // Keyed rows are re-inserted in the DOM, which drops focus; put it back on the same control.
    await tick();
    const row = document.querySelector<HTMLElement>(`[data-entry="${CSS.escape(entryId)}"]`);
    const target = row?.querySelector<HTMLButtonElement>(`[data-control="${control}"]:not(:disabled)`);
    (target ?? row?.querySelector<HTMLButtonElement>('[data-control]:not(:disabled)'))?.focus();
  }

  function onDragOver(e: DragEvent, index: number) {
    if (!dragId) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    drop = e.clientY < rect.top + rect.height / 2 ? index : index + 1;
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    if (dragId && drop !== null) moveTo(dragId, drop);
    endDrag();
  }

  function endDrag() {
    dragId = null;
    drop = null;
  }
</script>

<section class="queue" aria-label="Очередь">
  <SectionHead label="очередь" count={queue.length}>
    {#snippet aside()}
      {#if total > 0}<span>{formatTime(total)}</span>{/if}
    {/snippet}
  </SectionHead>

  {#if queue.length === 0}
    <div class="empty">
      <p>Очередь пуста. Вставьте ссылку на трек, альбом или плейлист SoundCloud.</p>
    </div>
  {:else}
    <ol ondrop={onDrop} ondragover={(e) => dragId && e.preventDefault()}>
      {#each queue as entry, i (entry.id)}
        <QueueRow
          {entry}
          index={i}
          last={i === queue.length - 1}
          removing={app.removing.has(entry.id)}
          dragging={dragId === entry.id}
          dropBefore={drop === i && dragId !== null}
          dropAfter={drop === queue.length && i === queue.length - 1 && dragId !== null}
          ondragstart={(e) => {
            dragId = entry.id;
            e.dataTransfer?.setData('text/plain', entry.id);
            if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
          }}
          ondragover={(e) => onDragOver(e, i)}
          ondragend={endDrag}
          onup={() => step(entry.id, -1, 'up')}
          ondown={() => step(entry.id, 1, 'down')}
          onremove={() => removeEntry(entry.id)}
        />
      {/each}
    </ol>
  {/if}
</section>

<style>
  .queue {
    margin-top: 40px;
  }

  .empty {
    padding: 20px 0;
    border-top: 1px solid var(--rule);
    color: var(--muted);
  }

  .empty p {
    max-width: 44ch;
  }

  ol {
    margin: 0;
    padding: 0;
    list-style: none;
  }
</style>
