import {createConnection} from 'node:net';
import {join} from 'node:path';
import {tmpdir, userInfo} from 'node:os';
import {createHash} from 'node:crypto';
import type {Agent, PresenceEvent, State} from './protocol.js';
let lastTimestamp = 0;
/** Per-user identifier so two Windows/macOS accounts never share a service endpoint. */
export function userId(): string {
  return createHash('sha256').update(userInfo().username).digest('hex').slice(0, 12);
}
/** Local-only endpoint: a per-user named pipe on Windows, a 0600 socket in a 0700 directory elsewhere. */
export function socketPath(): string {
  if (process.env.CAD_SOCKET) return process.env.CAD_SOCKET;
  return process.platform === 'win32' ? `\\\\.\\pipe\\ctrl-alt-delegate-${userId()}` : join(tmpdir(), `ctrl-alt-delegate-${userId()}`, 'service.sock');
}
export function request(message: object, path = socketPath(), timeout = 500): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(path);
    socket.setEncoding('utf8');
    let buffer = '';
    const timer = setTimeout(() => finish(new Error('Presence service timed out')), timeout);
    function finish(error?: Error, result?: unknown) {
      clearTimeout(timer); socket.destroy();
      if (error) reject(error); else resolve(result);
    }
    socket.on('error', error => finish(error));
    socket.on('connect', () => socket.write(JSON.stringify(message) + '\n'));
    socket.on('end', () => finish(new Error('Presence service disconnected')));
    socket.on('data', data => {
      buffer += data.toString();
      if (buffer.length > 256 * 1024) return finish(new Error('Response too large'));
      if (!buffer.includes('\n')) return;
      try {
        const result = JSON.parse(buffer.slice(0, buffer.indexOf('\n')));
        finish(result.ok ? undefined : new Error(result.error ?? 'Request failed'), result);
      } catch { finish(new Error('Invalid service response')); }
    });
  });
}
export async function emit(event: PresenceEvent, path?: string, timeout?: number): Promise<void> {
  try { await request({type: 'event', event}, path, timeout); } catch { /* Presence never blocks the tool. */ }
}
/** A bounded, serialized queue for long-lived reporters: coalesces rapid updates and preserves close ordering. */
export class Reporter {
  private event?: PresenceEvent;
  private pending?: PresenceEvent;
  private sending?: Promise<void>;
  private timer?: ReturnType<typeof setInterval>;
  project?: string;
  /** Set before `set()` when the update is caused by the user; cleared after sending. */
  interaction = false;
  constructor(readonly agent: Agent, readonly sessionId: string,
    private readonly send: (event: PresenceEvent) => Promise<void> = emit) {}
  set(state: State): void {
    const updatedAt = Math.max(Date.now(), lastTimestamp + 1);
    lastTimestamp = updatedAt;
    this.event = {version: 1, agent: this.agent, sessionId: this.sessionId, state, updatedAt, pid: process.pid,
      ...(this.project ? {project: this.project} : {}), ...(this.interaction ? {interaction: true} : {})};
    this.interaction = false;
    this.enqueue(this.event);
    if (!this.timer && state !== 'closed') {
      this.timer = setInterval(() => {
        if (this.event) this.enqueue({...this.event, heartbeat: true});
      }, 20_000);
      this.timer.unref();
    }
  }
  private enqueue(event: PresenceEvent): void {
    this.pending = event;
    if (this.sending) return;
    this.sending = this.flush().finally(() => { this.sending = undefined; });
  }
  private async flush(): Promise<void> {
    while (this.pending) {
      const next = this.pending; this.pending = undefined;
      try { await this.send(next); } catch { /* Best effort, including custom transports. */ }
    }
  }
  async close(): Promise<void> {
    clearInterval(this.timer); this.timer = undefined;
    this.set('closed');
    await this.sending;
  }
}
