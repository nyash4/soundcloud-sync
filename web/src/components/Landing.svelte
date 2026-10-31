<script lang="ts">
  import { ApiError, createRoom } from '../lib/api';
  import { navigate } from '../lib/router.svelte';
  import { saveRoomCreds } from '../lib/storage';
  import SimplePage from './SimplePage.svelte';

  let busy = $state(false);
  let error = $state<string | null>(null);

  async function create() {
    busy = true;
    error = null;
    try {
      const { roomId, token } = await createRoom();
      saveRoomCreds(roomId, { token, clientId: crypto.randomUUID(), name: '' });
      navigate(`/r/${roomId}#${token}`);
    } catch (e) {
      error = e instanceof ApiError ? e.message : 'Не удалось создать комнату.';
      busy = false;
    }
  }
</script>

<SimplePage wide>
  <div class="layout">
    <div>
      <h1>Слушать SoundCloud вместе</h1>
      <p class="lead">
        Комната по ссылке, общая очередь и общий старт каждого трека — звук у каждого свой, прямо из SoundCloud.
      </p>
      <div class="action">
        <button class="solid-button" onclick={create} disabled={busy}>
          {busy ? 'Создаём…' : 'Создать комнату'}
        </button>
        {#if error}
          <p class="error" role="alert">{error}</p>
        {/if}
      </div>
    </div>

    <ol class="steps mono">
      <li><span class="n">01</span>создайте комнату</li>
      <li><span class="n">02</span>отправьте ссылку</li>
      <li><span class="n">03</span>добавьте трек, альбом или плейлист</li>
      <li><span class="n">04</span>play — у всех одновременно</li>
    </ol>
  </div>
</SimplePage>

<style>
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 560px) minmax(0, 360px);
    justify-content: space-between;
    align-items: end;
    gap: 64px;
  }

  h1 {
    font-size: clamp(28px, 4vw, 40px);
    line-height: 1.1;
    letter-spacing: -0.02em;
  }

  .lead {
    margin-top: 20px;
    max-width: 42ch;
    font-size: 17px;
    line-height: 1.5;
    color: var(--muted);
  }

  .action {
    margin-top: 36px;
  }

  .error {
    margin-top: 12px;
    font-size: 13px;
  }

  .steps {
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--muted);
  }

  @media (max-width: 959px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
    }

    .steps {
      max-width: 400px;
    }
  }

  .steps li {
    display: flex;
    gap: 20px;
    padding: 10px 0;
    border-top: 1px solid var(--rule);
  }

  .steps li:last-child {
    border-bottom: 1px solid var(--rule);
  }

  .n {
    color: var(--ink);
  }
</style>
