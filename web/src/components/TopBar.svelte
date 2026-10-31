<script lang="ts">
  import { app } from '../lib/state.svelte';
  import InviteButton from './InviteButton.svelte';
  import NameEdit from './NameEdit.svelte';
  import Wordmark from './Wordmark.svelte';

  const connText = $derived(
    app.conn === 'reconnecting' ? 'связь потеряна, переподключаемся…' : app.conn === 'connecting' ? 'подключаемся…' : null,
  );
</script>

<header>
  <div class="inner">
    <div class="left">
      <Wordmark />
      {#if app.roomId}<span class="mono muted room">комната {app.roomId}</span>{/if}
    </div>
    {#if connText}
      <p class="mono conn" role="status"><span class="blink" aria-hidden="true"></span>{connText}</p>
    {/if}
    <div class="right">
      <InviteButton />
      <NameEdit />
    </div>
  </div>
</header>

<style>
  header {
    border-bottom: 1px solid var(--rule);
  }

  .inner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 24px;
    max-width: 1200px;
    min-height: 56px;
    margin: 0 auto;
    padding: 10px var(--gutter);
  }

  .left,
  .right {
    display: flex;
    align-items: baseline;
    gap: 20px;
  }

  .right {
    margin-left: auto;
  }

  .room {
    white-space: nowrap;
  }

  .conn {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }

  .blink {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    animation: blink 1.2s steps(2, jump-none) infinite;
  }

  @keyframes blink {
    50% {
      opacity: 0.2;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .blink {
      animation: none;
    }
  }

  @media (max-width: 640px) {
    .room {
      display: none;
    }

    .conn {
      order: 3;
      width: 100%;
    }
  }
</style>
