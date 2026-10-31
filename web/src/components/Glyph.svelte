<script lang="ts" module>
  export type GlyphName = 'play' | 'pause' | 'next' | 'up' | 'down' | 'remove' | 'grip';
</script>

<script lang="ts">
  let { name, size = 16 }: { name: GlyphName; size?: number } = $props();
</script>

<!-- Geometric, fill-only glyphs drawn on a 16-unit grid. -->
<svg width={size} height={size} viewBox="0 0 16 16" aria-hidden="true" focusable="false">
  {#if name === 'play'}
    <path d="M4 2.5v11l9.5-5.5z" />
  {:else if name === 'pause'}
    <path d="M3.5 3h3v10h-3zM9.5 3h3v10h-3z" />
  {:else if name === 'next'}
    <path d="M3 3v10l7.5-5zM11 3h2v10h-2z" />
  {:else if name === 'up'}
    <path d="M8 4.5l4.5 5-1 1L8 6.6l-3.5 3.9-1-1z" />
  {:else if name === 'down'}
    <path d="M8 11.5l-4.5-5 1-1L8 9.4l3.5-3.9 1 1z" />
  {:else if name === 'remove'}
    <path d="M4.5 3.5L8 7l3.5-3.5 1 1L9 8l3.5 3.5-1 1L8 9l-3.5 3.5-1-1L7 8 3.5 4.5z" />
  {:else if name === 'grip'}
    <path d="M5 3h2v2H5zM9 3h2v2H9zM5 7h2v2H5zM9 7h2v2H9zM5 11h2v2H5zM9 11h2v2H9z" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    fill: currentColor;
    flex: none;
  }
</style>
