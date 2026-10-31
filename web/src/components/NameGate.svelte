<script lang="ts">
  import { lastName } from '../lib/storage';
  import SimplePage from './SimplePage.svelte';

  let { roomId, onsubmit }: { roomId: string; onsubmit: (name: string) => void } = $props();

  let name = $state(lastName());
  const trimmed = $derived(name.trim());

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (trimmed) onsubmit(trimmed);
  }
</script>

<SimplePage>
  <p class="mono muted">комната {roomId}</p>
  <h1>Как вас представить?</h1>
  <p class="lead">Имя увидят остальные в комнате. Регистрация не нужна.</p>
  <form onsubmit={submit}>
    <label class="visually-hidden" for="name">Имя</label>
    <input
      id="name"
      class="line-input"
      bind:value={name}
      maxlength="32"
      autocomplete="nickname"
      placeholder="Имя"
      {@attach (el) => el.focus()}
    />
    <button class="solid-button" disabled={!trimmed}>Войти</button>
  </form>
</SimplePage>

<style>
  h1 {
    margin-top: 12px;
    font-size: 28px;
    line-height: 1.2;
    letter-spacing: -0.01em;
  }

  .lead {
    margin-top: 12px;
    color: var(--muted);
  }

  form {
    display: flex;
    gap: 16px;
    align-items: end;
    margin-top: 36px;
    max-width: 420px;
  }

  input {
    font-size: 17px;
  }
</style>
