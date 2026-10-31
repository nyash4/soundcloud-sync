<script lang="ts">
  import type { Snippet } from 'svelte';

  let { label, count, aside }: { label: string; count?: number; aside?: Snippet } = $props();
</script>

<div class="head mono">
  <h2>
    {label}{#if count !== undefined}<span class="count">{` · ${count}`}</span>{/if}
  </h2>
  {#if aside}<div class="aside">{@render aside()}</div>{/if}
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 16px;
    padding: 10px 0;
    border-top: 1px solid var(--ink);
    color: var(--muted);
  }

  h2 {
    font: inherit;
    color: var(--ink);
  }

  .count {
    color: var(--muted);
  }
</style>
