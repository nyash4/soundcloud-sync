<script lang="ts">
  import type { ParticipantStatus } from '../lib/protocol';

  let { status }: { status: ParticipantStatus } = $props();
</script>

<span class="dot {status}" aria-hidden="true"></span>

<style>
  .dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex: none;
    background: var(--muted);
  }

  .listening {
    background: var(--accent);
  }

  .waiting {
    background: var(--ink);
  }

  .paused,
  .loading {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--ink);
  }

  .loading {
    animation: breathe 1.4s ease-in-out infinite alternate;
  }

  .blocked {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .offline {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--muted);
  }

  .out {
    background: transparent;
    border: 1px dashed var(--muted);
  }

  @keyframes breathe {
    to {
      opacity: 0.3;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .loading {
      animation: none;
    }
  }
</style>
