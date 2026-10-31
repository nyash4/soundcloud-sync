<script lang="ts">
  let { src, artist, size }: { src: string | null; artist: string; size: 'large' | 'small' } = $props();

  let failedUrl = $state<string | null>(null);
  const letter = $derived(artist.trim().charAt(0).toUpperCase() || '·');
  /** SoundCloud serves fixed sizes by suffix; a 40px row only needs the 100px one. */
  const url = $derived(size === 'small' && src ? src.replace('-t500x500.', '-large.') : src);
</script>

<div class="art {size}">
  {#if url && url !== failedUrl}
    <img src={url} alt="" loading="lazy" onerror={() => (failedUrl = url)} />
  {:else}
    <span class="mono letter" aria-hidden="true">{letter}</span>
  {/if}
</div>

<style>
  .art {
    position: relative;
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    background: var(--fill);
    overflow: hidden;
    flex: none;
  }

  .large {
    width: 100%;
    max-width: 320px;
    border: 1px solid var(--rule);
  }

  .small {
    width: 40px;
  }

  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .letter {
    color: var(--muted);
  }

  .large .letter {
    font-size: 64px;
    line-height: 1;
  }

  .small .letter {
    font-size: 13px;
  }
</style>
