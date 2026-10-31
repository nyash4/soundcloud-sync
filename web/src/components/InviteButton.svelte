<script lang="ts">
  import { inviteUrl } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';

  let copied = $state(false);
  /** Shown when the clipboard is unavailable (e.g. plain http on a LAN address). */
  let manual = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    if (!app.roomId || !app.creds) return;
    const url = inviteUrl(app.roomId, app.creds.token);
    try {
      await navigator.clipboard.writeText(url);
      copied = true;
      clearTimeout(timer);
      timer = setTimeout(() => (copied = false), 1800);
    } catch {
      manual = url;
    }
  }
</script>

{#if manual}
  <span class="manual">
    <label class="visually-hidden" for="invite">Ссылка приглашения</label>
    <input
      id="invite"
      class="line-input mono"
      readonly
      value={manual}
      {@attach (el) => { el.focus(); el.select(); }}
      onblur={() => (manual = null)}
    />
  </span>
{:else}
  <button class="text-button mono invite" onclick={copy} aria-live="polite">
    {copied ? 'скопировано' : 'скопировать приглашение'}
  </button>
{/if}

<style>
  .invite {
    font-size: 12px;
    white-space: nowrap;
  }

  .manual {
    width: min(420px, 70vw);
  }

  .manual input {
    height: 28px;
  }
</style>
