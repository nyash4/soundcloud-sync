<script lang="ts">
  import { untrack } from 'svelte';
  import { hashToken } from '../lib/router.svelte';
  import { enterRoom, leaveRoom } from '../lib/state.svelte';
  import { loadRoomCreds, saveRoomCreds, type RoomCreds } from '../lib/storage';
  import Message from './Message.svelte';
  import NameGate from './NameGate.svelte';
  import Room from './Room.svelte';

  let { roomId }: { roomId: string } = $props();

  const mock = import.meta.env.DEV ? new URLSearchParams(location.search).get('mock') : null;

  function resolveCreds(): RoomCreds | null {
    const stored = loadRoomCreds(roomId);
    const token = hashToken() ?? stored?.token;
    if (!token) return null;
    const creds = { token, clientId: stored?.clientId ?? crypto.randomUUID(), name: stored?.name ?? '' };
    if (stored?.token !== token) saveRoomCreds(roomId, creds);
    return creds;
  }

  let creds = $state<RoomCreds | null>(mock ? null : resolveCreds());
  const named = $derived(!!creds?.name);

  $effect(() => {
    if (mock) {
      void import('../lib/mock').then((m) => m.applyMock(roomId, mock));
      return leaveRoom;
    }
    if (!named) return;
    untrack(() => enterRoom(roomId, $state.snapshot(creds)!));
    return leaveRoom;
  });

  function setName(name: string) {
    if (!creds) return;
    creds = { ...creds, name };
    saveRoomCreds(roomId, creds);
  }
</script>

{#if mock || named}
  <Room />
{:else if creds}
  <NameGate {roomId} onsubmit={setName} />
{:else}
  <Message
    title="Ссылка неполная"
    text="В ссылке приглашения не хватает ключа комнаты — части после знака #. Попросите прислать ссылку целиком."
  />
{/if}
