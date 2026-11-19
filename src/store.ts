import {parseEvent, sessionKey, type PresenceEvent} from './protocol.js';
export interface Session extends PresenceEvent { startedAt: number; lastSeen: number }
const priority = {working: 3, waiting: 3, error: 2, idle: 1, closed: 0};
export class SessionStore {
  readonly sessions = new Map<string, Session>();
  private readonly ended = new Map<string, number>();
  pinned: string | null = null;
  constructor(readonly staleMs = 30 * 60_000, private readonly alive = processAlive) {}
  update(value: unknown, now = Date.now()): void {
    const e = parseEvent(value, now);
    const key = sessionKey(e);
    const prior = this.sessions.get(key);
    if (e.updatedAt <= (this.ended.get(key) ?? -1) || (prior && e.updatedAt < prior.updatedAt)) return;
    if (e.state === 'closed') {
      this.sessions.delete(key);
      this.ended.set(key, e.updatedAt);
      if (this.pinned === key) this.pinned = null;
      return;
    }
    // Bound memory even when a client invents a new session per event.
    if (!prior && this.sessions.size >= 256) throw new Error('Session limit reached');
    this.sessions.set(key, {...e, project: e.project ?? prior?.project,
      startedAt: prior?.startedAt ?? e.updatedAt, lastSeen: now});
  }
  sweep(now = Date.now()): void {
    for (const [key, s] of this.sessions) {
      const expired = now - s.lastSeen > (s.pid ? 90_000 : this.staleMs);
      if (expired || (s.pid && !this.alive(s.pid))) {
        this.sessions.delete(key);
        this.ended.set(key, s.updatedAt);
        if (this.pinned === key) this.pinned = null;
      }
    }
    for (const [key, at] of this.ended) if (now - at > this.staleMs) this.ended.delete(key);
    while (this.ended.size > 1024) this.ended.delete(this.ended.keys().next().value!);
  }
  select(): Session | undefined {
    if (this.pinned && this.sessions.has(this.pinned)) return this.sessions.get(this.pinned);
    return [...this.sessions.values()].sort((a, b) =>
      priority[b.state] - priority[a.state] || b.updatedAt - a.updatedAt || sessionKey(a).localeCompare(sessionKey(b)))[0];
  }
  pin(key: string | null): void {
    if (key !== null && !this.sessions.has(key)) throw new Error('Unknown session');
    this.pinned = key;
  }
}
function processAlive(pid: number): boolean {
  try { process.kill(pid, 0); return true; }
  catch (error) { return (error as NodeJS.ErrnoException).code === 'EPERM'; }
}
