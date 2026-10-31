<script lang="ts">
  import { serverNow } from '../lib/clock';
  import { formatTime } from '../lib/format';

  /** Server-time deadline. */
  let { until }: { until: number } = $props();

  let now = $state(serverNow());

  $effect(() => {
    const id = setInterval(() => (now = serverNow()), 250);
    return () => clearInterval(id);
  });
</script>

<span class="mono countdown" title="Сколько ещё ждём">{formatTime(Math.max(0, until - now) + 999)}</span>

<style>
  .countdown {
    color: var(--muted);
  }
</style>
