<script lang="ts">
  import type { Snippet } from 'svelte';
  import Wordmark from './Wordmark.svelte';

  let { children, wide = false }: { children: Snippet; wide?: boolean } = $props();
</script>

<header>
  <div class="inner"><Wordmark /></div>
</header>
<main>
  <div class="content" class:wide>
    {@render children()}
  </div>
</main>

<style>
  header {
    border-bottom: 1px solid var(--rule);
  }

  .inner,
  main {
    max-width: 1200px;
    margin: 0 auto;
    padding: 0 var(--gutter);
  }

  .inner {
    display: flex;
    align-items: center;
    height: 56px;
  }

  .content {
    max-width: 560px;
    padding: clamp(48px, 14vh, 136px) 0 64px;
  }

  .content.wide {
    max-width: none;
  }
</style>
