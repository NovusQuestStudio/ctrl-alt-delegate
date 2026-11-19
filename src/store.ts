import {parseEvent, sessionKey, type PresenceEvent} from './protocol.js';

export interface Session extends Omit<PresenceEvent, 'ppid' | 'interaction' | 'heartbeat'> {
  startedAt: number;
  lastSeen: number;
  /** Time of the last deliberate user action in this session. */
  lastInteraction: number;
  /** Long-lived agent process found from a hook's ancestry; used for liveness and focus. */
  agentPid?: number;
  agentName?: string;
  /** Agent pid and its ancestors (terminal, editor, desktop app), used to match the focused window. */
  ancestors?: number[];
}
export interface AgentProcess {agentPid: number; agentName: string; ancestors: number[]}

export class SessionStore {
  readonly sessions = new Map<string, Session>();
  private readonly ended = new Map<string, number>();
  pinned: string | null = null;
  /** Bumped on every change worth persisting. */
  revision = 0;
  constructor(readonly staleMs = 30 * 60_000, private readonly alive = processAlive) {}
  /** Apply an event. Returns the updated session, or undefined when it was ignored or closed. */
  update(value: unknown, now = Date.now()): Session | undefined {
    const e = parseEvent(value, now);
    const key = sessionKey(e);
    const prior = this.sessions.get(key);
    if (e.updatedAt <= (this.ended.get(key) ?? -1) || (prior && e.updatedAt < prior.updatedAt)) return;
    if (e.state === 'closed') { this.drop(key, e.updatedAt); return; }
    // Bound memory even when a client invents a new session per event.
    if (!prior && this.sessions.size >= 256) throw new Error('Session limit reached');
    const {ppid: _ppid, interaction, heartbeat, ...fields} = e;
    const session: Session = {...fields, project: e.project ?? prior?.project,
      startedAt: prior?.startedAt ?? e.updatedAt, lastSeen: now,
      lastInteraction: interaction || !prior ? Math.max(e.updatedAt, prior?.lastInteraction ?? 0) : prior.lastInteraction,
      agentPid: prior?.agentPid, agentName: prior?.agentName, ancestors: prior?.ancestors};
    this.sessions.set(key, session);
    if (!heartbeat || !prior) this.revision++;
    return session;
  }
  attach(key: string, agent: AgentProcess): void {
    const session = this.sessions.get(key);
    if (!session) return;
    Object.assign(session, agent); this.revision++;
  }
  drop(key: string, at = Date.now()): void {
    if (this.sessions.delete(key)) this.revision++;
    this.ended.set(key, at);
    if (this.pinned === key) this.pinned = null;
  }
  sweep(now = Date.now()): void {
    for (const [key, s] of this.sessions) {
      // Reporters (pid) renew a 90 s lease with heartbeats; hook sessions expire after staleMs without events.
      const expired = now - s.lastSeen > (s.pid ? 90_000 : this.staleMs);
      const owner = s.pid ?? s.agentPid;
      if (expired || (owner && !this.alive(owner))) this.drop(key, s.updatedAt);
    }
    for (const [key, at] of this.ended) if (now - at > this.staleMs) this.ended.delete(key);
    while (this.ended.size > 1024) this.ended.delete(this.ended.keys().next().value!);
  }
  pin(key: string | null): void {
    if (key !== null && !this.sessions.has(key)) throw new Error('Unknown session');
    this.pinned = key; this.revision++;
  }
  snapshot(): {sessions: Session[]; pinned: string | null} {
    return {sessions: [...this.sessions.values()].map(s => ({...s})), pinned: this.pinned};
  }
  /** Restore sessions saved by a previous run. Invalid entries are dropped; stale ones are swept. */
  restore(saved: unknown, now = Date.now()): void {
    if (!saved || typeof saved !== 'object') return;
    const {sessions, pinned} = saved as {sessions?: unknown; pinned?: unknown};
    if (!Array.isArray(sessions)) return;
    for (const raw of sessions.slice(0, 256)) {
      try {
        const s = raw as Record<string, unknown>;
        const e = parseEvent({...s, version: 1}, now);
        const time = (v: unknown) => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 && v <= now + 60_000;
        if (e.state === 'closed' || !time(s.startedAt) || !time(s.lastSeen) || !time(s.lastInteraction)) continue;
        const agentPid = Number.isSafeInteger(s.agentPid) && (s.agentPid as number) > 0 ? s.agentPid as number : undefined;
        const ancestors = Array.isArray(s.ancestors) && s.ancestors.length <= 16 && s.ancestors.every(p => Number.isSafeInteger(p) && p > 0) ? s.ancestors as number[] : undefined;
        const agentName = typeof s.agentName === 'string' && s.agentName.length <= 260 ? s.agentName : undefined;
        const {ppid: _p, interaction: _i, heartbeat: _h, ...fields} = e;
        this.sessions.set(sessionKey(e), {...fields, startedAt: s.startedAt as number, lastSeen: s.lastSeen as number,
          lastInteraction: s.lastInteraction as number, agentPid, agentName, ancestors: agentPid ? ancestors : undefined});
      } catch { /* Skip entries this version cannot read. */ }
    }
    if (typeof pinned === 'string' && this.sessions.has(pinned)) this.pinned = pinned;
    this.sweep(now);
  }
}

function processAlive(pid: number): boolean {
  try { process.kill(pid, 0); return true; }
  catch (error) { return (error as NodeJS.ErrnoException).code === 'EPERM'; }
}
