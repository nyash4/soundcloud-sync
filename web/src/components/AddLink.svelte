<script lang="ts">
  import { plural, TRACKS } from '../lib/format';
  import { UNAVAILABLE_REASON } from '../lib/labels';
  import type { ImportResult, UnavailableItem } from '../lib/protocol';
  import { app, clearImportResult, importUrl } from '../lib/state.svelte';
  import SectionHead from './SectionHead.svelte';

  let url = $state('');
  const iv = $derived(app.importView);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const value = url.trim();
    if (!value || iv.busy) return;
    await importUrl(value);
    if (!app.importView.error) url = '';
  }

  function summary(r: ImportResult): string {
    if (r.added === 0) return 'Ничего не добавлено.';
    if (!r.collection) return r.added === 1 ? 'Трек добавлен в очередь.' : `Добавлено: ${r.added} ${plural(r.added, TRACKS)}.`;
    const from = r.collection.kind === 'album' ? 'альбома' : 'плейлиста';
    return `Добавлено: ${r.added} ${plural(r.added, TRACKS)} из ${from} «${r.collection.title}».`;
  }
</script>

<section aria-label="Добавить в очередь">
  <SectionHead label="добавить" />
  <form onsubmit={submit}>
    <label class="visually-hidden" for="link">Ссылка SoundCloud</label>
    <input
      id="link"
      class="line-input"
      type="text"
      inputmode="url"
      autocomplete="off"
      spellcheck="false"
      placeholder="Ссылка на трек, альбом или плейлист SoundCloud"
      bind:value={url}
      disabled={iv.busy}
    />
    <button class="text-button" disabled={iv.busy || !url.trim()}>Добавить</button>
  </form>

  {#snippet unavailableList(items: UnavailableItem[])}
    {#if items.length}
      <details>
        <summary class="mono">недоступно · {items.length}</summary>
        <ul>
          {#each items as item, i (i)}
            <li>
              <span class="title">{item.title ?? 'Без названия'}</span>
              <span class="mono muted">{UNAVAILABLE_REASON[item.reason] ?? UNAVAILABLE_REASON.unknown}</span>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  {/snippet}

  <div class="result" aria-live="polite">
    {#if iv.progress}
      <p class="mono muted progress">{iv.progress}</p>
    {:else if iv.error}
      <p class="error">
        <span class="mono tag">не добавлено</span>
        <span>{iv.error.message}</span>
        <button class="text-button mono dismiss" onclick={clearImportResult}>скрыть</button>
      </p>
      {@render unavailableList(iv.error.unavailable)}
    {:else if iv.result}
      {@const r = iv.result}
      <p class="ok">
        <span>{summary(r)}</span>
        <button class="text-button mono dismiss" onclick={clearImportResult}>скрыть</button>
      </p>
      {@render unavailableList(r.unavailable)}
      {#if r.missing > 0}
        <p class="muted note">
          Ещё {r.missing}
          {plural(r.missing, TRACKS)} SoundCloud не отдал вовсе — удалены или скрыты автором.
        </p>
      {/if}
    {/if}
  </div>
</section>

<style>
  form {
    display: flex;
    align-items: end;
    gap: 20px;
    margin-top: 4px;
  }

  .text-button {
    flex: none;
    height: 40px;
    font-weight: 500;
  }

  .result {
    min-height: 8px;
    margin-top: 10px;
    font-size: 13px;
  }

  .progress {
    animation: fade var(--fast) ease-out;
  }

  .ok,
  .error {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 12px;
  }

  .tag {
    color: var(--accent);
  }

  .dismiss {
    margin-left: auto;
    height: auto;
    font-weight: 400;
    color: var(--muted);
  }

  details {
    margin-top: 8px;
  }

  summary {
    cursor: pointer;
    color: var(--muted);
    width: fit-content;
  }

  summary:hover {
    color: var(--ink);
  }

  ul {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 2px 16px;
    padding: 6px 0;
    border-top: 1px solid var(--rule);
  }

  .title {
    overflow-wrap: anywhere;
  }

  .note {
    margin-top: 8px;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
