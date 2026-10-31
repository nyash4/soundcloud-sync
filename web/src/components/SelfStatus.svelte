<script lang="ts">
  import { app } from '../lib/state.svelte';
  import Countdown from './Countdown.svelte';


  const pb = $derived(app.room?.playback ?? null);
  const me = $derived(app.me);
  const view = $derived(app.player);

  const kind = $derived.by(() => {
    if (!pb || !me) return null;
    if (view.mode === 'blocked') return 'blocked';
    if (view.mode === 'failed') return 'failed';
    if (view.mode === 'ended' && pb.phase === 'waiting_end') return 'waiting';
    if (view.mode === 'ready' && pb.phase === 'preparing') return 'ready';
    if (view.mode === 'loading' || view.mode === 'warming') return 'loading';
    return null;
  });
</script>

{#if kind}
  <div class="status" role="status">
    {#if kind === 'blocked'}
      <p>Звук включится при первом нажатии на страницу.</p>
    {:else if kind === 'failed'}
      <p>Не удалось загрузить звук. При возвращении во вкладку попробуем снова.</p>
    {:else if kind === 'waiting'}
      <p class="line">
        <span>Дослушали — ждём остальных</span>
        {#if pb?.endDeadline && !pb.paused}<Countdown until={pb.endDeadline} />{/if}
      </p>
    {:else if kind === 'ready'}
      <p class="line">
        <span>{pb?.paused ? 'Трек готов. Пауза.' : 'Трек готов, ждём остальных'}</span>
        {#if pb?.readyDeadline && !pb.paused}<Countdown until={pb.readyDeadline} />{/if}
      </p>
    {:else if kind === 'loading'}
      <p class="muted">Загружаем трек…</p>
    {/if}
  </div>
{/if}

<style>
  .status {
    margin-top: 24px;
    padding-top: 14px;
    border-top: 1px solid var(--rule);
    font-size: 14px;
  }

  .line {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 16px;
  }

</style>
