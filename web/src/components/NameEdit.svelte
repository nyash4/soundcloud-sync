<script lang="ts">
  import { app, rename } from '../lib/state.svelte';

  let editing = $state(false);
  let draft = $state('');
  let saving = $state(false);

  function start() {
    draft = app.creds?.name ?? '';
    editing = true;
  }

  async function save() {
    const name = draft.trim();
    if (!name || name === app.creds?.name) {
      editing = false;
      return;
    }
    saving = true;
    if (await rename(name)) editing = false;
    saving = false;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') editing = false;
  }
</script>

<span class="name">
  <span class="mono muted">вы</span>
  {#if editing}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <label class="visually-hidden" for="self-name">Ваше имя</label>
      <input
        id="self-name"
        class="line-input"
        bind:value={draft}
        maxlength="32"
        disabled={saving}
        {onkeydown}
        onblur={() => void save()}
        {@attach (el) => { el.focus(); el.select(); }}
      />
    </form>
  {:else}
    <button class="text-button" onclick={start} title="Изменить имя">{app.creds?.name}</button>
  {/if}
</span>

<style>
  .name {
    display: inline-flex;
    align-items: baseline;
    gap: 8px;
    white-space: nowrap;
  }

  .text-button {
    font-weight: 500;
    max-width: 16ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  input {
    width: 16ch;
    height: 28px;
    font-weight: 500;
  }
</style>
