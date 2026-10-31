<script lang="ts">
  import Landing from './components/Landing.svelte';
  import Message from './components/Message.svelte';
  import RoomGate from './components/RoomGate.svelte';
  import { router } from './lib/router.svelte';
</script>

{#if router.route.name === 'landing'}
  <Landing />
{:else if router.route.name === 'room'}
  {#key router.route.roomId}
    <RoomGate roomId={router.route.roomId} />
  {/key}
{:else}
  <Message title="Такой страницы нет" text="Похоже, адрес набран с ошибкой." />
{/if}
