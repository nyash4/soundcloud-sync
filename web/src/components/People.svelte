<script lang="ts">
  import { OUT_REASON, STATUS_LABEL } from '../lib/labels';
  import { app } from '../lib/state.svelte';
  import SectionHead from './SectionHead.svelte';
  import StatusDot from './StatusDot.svelte';

  const people = $derived(app.room?.participants ?? []);
  const online = $derived(people.filter((p) => p.online).length);
</script>

<section class="people" aria-label="Слушатели">
  <SectionHead label="слушатели" count={people.length}>
    {#snippet aside()}
      {#if online !== people.length}<span>{online} на связи</span>{/if}
    {/snippet}
  </SectionHead>
  <ul>
    {#each people as p (p.id)}
      <li class:offline={!p.online}>
        <StatusDot status={p.status} />
        <span class="name">
          {p.name}{#if p.id === app.selfId}<span class="muted">{' · вы'}</span>{/if}
        </span>
        <!-- For "out" the reason says more than the generic label; the dashed dot marks the state. -->
        <span class="mono state" title={STATUS_LABEL[p.status]}>
          {p.status === 'out' && p.outReason ? OUT_REASON[p.outReason] : STATUS_LABEL[p.status]}
        </span>
      </li>
    {/each}
  </ul>
</section>

<style>
  .people {
    margin-top: 40px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 6px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 40px;
    padding: 8px 0;
    border-top: 1px solid var(--rule);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .offline .name {
    color: var(--muted);
  }

  .state {
    color: var(--muted);
    text-align: right;
  }

</style>
