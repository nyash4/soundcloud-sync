<script lang="ts">
  import { app } from '../lib/state.svelte';
  import AddLink from './AddLink.svelte';
  import History from './History.svelte';
  import Message from './Message.svelte';
  import Notices from './Notices.svelte';
  import NowPlaying from './NowPlaying.svelte';
  import People from './People.svelte';
  import Queue from './Queue.svelte';
  import SelfStatus from './SelfStatus.svelte';
  import TopBar from './TopBar.svelte';
  import Transport from './Transport.svelte';

  const FATAL: Record<string, { title: string; text: string }> = {
    session_replaced: {
      title: 'Комната открыта в другой вкладке',
      text: 'Продолжайте слушать там. Чтобы вернуться сюда, обновите эту страницу.',
    },
    room_not_found: { title: 'Комната не найдена', text: 'Возможно, ссылка устарела или в ней опечатка.' },
    bad_token: {
      title: 'Ссылка не подходит',
      text: 'Ключ в ссылке не совпадает с этой комнатой. Попросите прислать приглашение ещё раз.',
    },
  };

  const fatal = $derived(
    app.fatal && (FATAL[app.fatal.error] ?? { title: 'Не удалось войти в комнату', text: app.fatal.message }),
  );
</script>

<svelte:head>
  <title>{app.room?.current ? `${app.room.current.title} — sc/sync` : 'sc/sync'}</title>
</svelte:head>

{#if fatal}
  <Message title={fatal.title} text={fatal.text} />
{:else}
  <TopBar />
  <main class="room">
    {#if app.room}
      <div class="side">
        <NowPlaying />
        <Transport />
        <SelfStatus />
        <People />
      </div>
      <div class="main">
        <AddLink />
        <Queue />
        <History />
      </div>
    {:else}
      <p class="mono muted connecting">подключаемся к комнате…</p>
    {/if}
  </main>
  <Notices />
{/if}

<style>
  .room {
    display: grid;
    grid-template-columns: 360px minmax(0, 1fr);
    max-width: 1200px;
    margin: 0 auto;
    padding: 0 var(--gutter);
  }

  .side {
    padding: 32px 40px 64px 0;
    border-right: 1px solid var(--rule);
    min-width: 0;
  }

  .main {
    padding: 32px 0 64px 40px;
    min-width: 0;
  }

  .connecting {
    grid-column: 1 / -1;
    padding: 48px 0;
  }

  @media (max-width: 959px) {
    .room {
      grid-template-columns: minmax(0, 1fr);
    }

    .side {
      padding: 24px 0 32px;
      border-right: 0;
      border-bottom: 1px solid var(--rule);
    }

    .main {
      padding: 24px 0 64px;
    }
  }
</style>
