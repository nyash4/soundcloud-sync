<script lang="ts">
  import SimplePage from './SimplePage.svelte';

  let { title, text }: { title: string; text: string } = $props();
</script>

<SimplePage>
  <h1>{title}</h1>
  <p class="text">{text}</p>
  <p class="action"><a href="/">Создать новую комнату</a></p>
</SimplePage>

<style>
  h1 {
    font-size: 28px;
    line-height: 1.2;
    letter-spacing: -0.01em;
  }

  .text {
    margin-top: 16px;
    color: var(--muted);
    max-width: 44ch;
  }

  .action {
    margin-top: 32px;
  }
</style>
