import { addClockSample, resetClock } from './clock';
import type { Ack, CommandArgs, CommandName, NoticeLevel, Report, RoomState, ServerMessage } from './protocol';
import { after, every, onSleep, type Cancel } from './timers';

export type ConnStatus = 'connecting' | 'open' | 'reconnecting' | 'fatal';

export interface Hello {
  roomId: string;
  token: string;
  clientId: string;
  name: string;
}

export interface NetEvents {
  welcome(selfId: string, state: RoomState): void;
  state(state: RoomState): void;
  notice(level: NoticeLevel, text: string): void;
  fatal(error: string, message: string): void;
  status(status: ConnStatus): void;
}

interface Pending {
  name: CommandName;
  args: CommandArgs[CommandName];
  revision: number | undefined;
  sentAt: number;
  resolve(ack: Ack | null): void;
}

const BACKOFF_MIN_MS = 500;
const BACKOFF_MAX_MS = 8000;
const SILENCE_LIMIT_MS = 15_000;
/** Commands older than this are not replayed after a reconnect: the snapshot already tells the truth. */
const REPLAY_WINDOW_MS = 10_000;
const FAST_PING_MS = 2000;
const FAST_PING_PERIOD_MS = 10_000;
const SLOW_PING_MS = 5000;
const LIVENESS_CHECK_MS = 1000;
const HANDSHAKE_LIMIT_MS = 10_000;

export class Connection {
  private ws: WebSocket | null = null;
  private attempt = 0;
  private stopped = false;
  private welcomed = false;
  private lastHeard = 0;
  private connectedAt = 0;
  private openedAt = 0;
  private pending = new Map<string, Pending>();
  private timers: Cancel[] = [];
  private reconnectTimer: Cancel | null = null;
  private pingTimer: Cancel | null = null;

  constructor(
    private hello: Hello,
    private on: NetEvents,
  ) {}

  start(): void {
    this.timers.push(
      onSleep(() => this.reconnectNow()),
      every(LIVENESS_CHECK_MS, () => {
        if (!this.ws) return;
        const now = Date.now();
        const silent = this.welcomed && now - this.lastHeard > SILENCE_LIMIT_MS;
        const stuck = !this.welcomed && now - this.openedAt > HANDSHAKE_LIMIT_MS;
        if (silent || stuck) this.reconnectNow();
      }),
    );
    document.addEventListener('visibilitychange', this.onVisible);
    window.addEventListener('online', this.onOnline);
    this.open();
  }

  stop(): void {
    this.stopped = true;
    for (const cancel of this.timers) cancel();
    this.reconnectTimer?.();
    this.pingTimer?.();
    document.removeEventListener('visibilitychange', this.onVisible);
    window.removeEventListener('online', this.onOnline);
    this.ws?.close(1000);
    this.ws = null;
  }

  setName(name: string): void {
    this.hello = { ...this.hello, name };
  }

  /**
   * Sends a command; resolves with its ack, or null when the connection was lost for too
   * long to replay it. A stale `queue.move` is retried once with fresh state.
   */
  async command<N extends CommandName>(name: N, args: CommandArgs[N], revision?: number): Promise<Ack | null> {
    const ack = await this.sendCommand(name, args, revision);
    if (ack && !ack.ok && ack.error === 'conflict' && name === 'queue.move') {
      const { entryId, afterId } = args as CommandArgs['queue.move'];
      const ids = new Set(ack.state.queue.map((e) => e.id));
      if (ids.has(entryId) && (afterId === null || ids.has(afterId))) {
        return this.sendCommand(name, args, ack.state.revision);
      }
    }
    return ack;
  }

  /** Player reports are fire-and-forget; the player re-sends what matters after `welcome`. */
  report(msg: Report): void {
    if (this.welcomed) this.send(msg);
  }

  private sendCommand(name: CommandName, args: CommandArgs[CommandName], revision?: number): Promise<Ack | null> {
    const commandId = crypto.randomUUID();
    return new Promise<Ack | null>((resolve) => {
      this.pending.set(commandId, { name, args, revision, sentAt: Date.now(), resolve });
      if (this.welcomed) this.sendPending(commandId);
    });
  }

  private sendPending(commandId: string): void {
    const p = this.pending.get(commandId);
    if (!p) return;
    this.send({ type: 'cmd', commandId, name: p.name, args: p.args, ...(p.revision !== undefined ? { revision: p.revision } : {}) });
  }

  private send(msg: object): void {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg));
  }

  private open(): void {
    this.welcomed = false;
    this.openedAt = Date.now();
    this.on.status(this.attempt === 0 ? 'connecting' : 'reconnecting');
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${proto}//${location.host}/ws`);
    this.ws = ws;
    ws.onopen = () => {
      if (ws !== this.ws) return;
      this.lastHeard = Date.now();
      this.send({ type: 'hello', ...this.hello });
    };
    ws.onmessage = (e) => {
      if (ws !== this.ws) return;
      this.lastHeard = Date.now();
      let msg: ServerMessage;
      try {
        msg = JSON.parse(String(e.data)) as ServerMessage;
      } catch {
        return;
      }
      this.handle(msg);
    };
    ws.onclose = () => {
      if (ws !== this.ws) return;
      this.dropSocket();
      this.scheduleReconnect();
    };
  }

  private handle(msg: ServerMessage): void {
    switch (msg.type) {
      case 'welcome':
        this.welcomed = true;
        this.attempt = 0;
        this.connectedAt = Date.now();
        resetClock(msg.serverTime);
        this.on.status('open');
        this.on.welcome(msg.selfId, msg.state);
        this.replayPending();
        this.ping();
        break;
      case 'pong':
        addClockSample(msg.t0, msg.serverTime);
        break;
      case 'ack': {
        const p = this.pending.get(msg.commandId);
        this.pending.delete(msg.commandId);
        if (!msg.ok) this.on.state(msg.state);
        p?.resolve(msg);
        break;
      }
      case 'state':
        this.on.state(msg.state);
        break;
      case 'notice':
        this.on.notice(msg.level, msg.text);
        break;
      case 'fatal':
        this.stopped = true;
        this.on.status('fatal');
        this.on.fatal(msg.error, msg.message);
        this.ws?.close(1000);
        break;
    }
  }

  private replayPending(): void {
    const now = Date.now();
    for (const [id, p] of this.pending) {
      if (now - p.sentAt < REPLAY_WINDOW_MS) {
        this.sendPending(id);
      } else {
        this.pending.delete(id);
        p.resolve(null);
      }
    }
  }

  private ping(): void {
    this.pingTimer?.();
    this.send({ type: 'ping', t0: Date.now() });
    const fast = Date.now() - this.connectedAt < FAST_PING_PERIOD_MS;
    this.pingTimer = after(fast ? FAST_PING_MS : SLOW_PING_MS, () => this.ping());
  }

  private dropSocket(): void {
    const ws = this.ws;
    this.ws = null;
    this.welcomed = false;
    this.pingTimer?.();
    this.pingTimer = null;
    if (ws) {
      ws.onopen = ws.onmessage = ws.onclose = null;
      if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) ws.close(4000);
    }
  }

  private scheduleReconnect(): void {
    if (this.stopped) return;
    this.on.status('reconnecting');
    const delay = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** this.attempt);
    this.attempt++;
    this.reconnectTimer?.();
    this.reconnectTimer = after(delay, () => {
      this.reconnectTimer = null;
      this.open();
    });
  }

  private reconnectNow(): void {
    if (this.stopped) return;
    this.dropSocket();
    this.reconnectTimer?.();
    this.reconnectTimer = null;
    this.attempt = Math.max(this.attempt, 1);
    this.open();
  }

  private onVisible = (): void => {
    if (document.visibilityState !== 'visible') return;
    const healthy = this.ws?.readyState === WebSocket.OPEN && Date.now() - this.lastHeard < SILENCE_LIMIT_MS;
    if (!healthy && this.ws?.readyState !== WebSocket.CONNECTING) this.reconnectNow();
  };

  private onOnline = (): void => {
    if (this.ws?.readyState !== WebSocket.OPEN) this.reconnectNow();
  };
}
