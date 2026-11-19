import {sessionKey} from './protocol.js';
import type {Session} from './store.js';

/**
 * What the user is looking at. `pid` comes from the tray (foreground window owner);
 * `key` comes from a reporter that knows its own window is focused (the VS Code extension).
 */
export interface Focus {pid?: number; key?: string; at: number}

const active = (s: Session) => s.state === 'working' || s.state === 'waiting' || s.state === 'error';

export function isFocused(s: Session, focus?: Focus | null): boolean {
  if (!focus) return false;
  if (focus.key !== undefined) return focus.key === sessionKey(s);
  if (focus.pid === undefined) return false;
  return s.pid === focus.pid || s.agentPid === focus.pid || (s.ancestors?.includes(focus.pid) ?? false);
}

/** Focused first, then active over idle, then the most recent user interaction, then the latest event. */
export function rank(sessions: Iterable<Session>, focus?: Focus | null): Session[] {
  return [...sessions].sort((a, b) =>
    Number(isFocused(b, focus)) - Number(isFocused(a, focus)) ||
    Number(active(b)) - Number(active(a)) ||
    b.lastInteraction - a.lastInteraction ||
    b.updatedAt - a.updatedAt ||
    sessionKey(a).localeCompare(sessionKey(b)));
}

/**
 * Chooses the session to show, with hysteresis so the card does not flicker:
 * - a pin always wins, and unpinning re-ranks immediately;
 * - if the shown session disappears, the next best is shown immediately;
 * - a fresh user interaction (prompt, session start, editor activity) switches immediately;
 * - any other change (state flips, focus changes) must stay on top for `settleMs` first.
 */
export class Selector {
  private current: string | null = null;
  private chosenAt = 0;
  private candidate: string | null = null;
  private candidateSince = 0;
  private wasPinned = false;
  constructor(readonly settleMs = 5000) {}
  pick(sessions: Map<string, Session>, pinned: string | null, focus: Focus | null, now = Date.now()): Session | undefined {
    if (pinned && sessions.has(pinned)) { this.wasPinned = true; return this.commit(pinned, now, sessions); }
    const [top] = rank(sessions.values(), focus);
    if (!top) { this.current = null; this.candidate = null; this.wasPinned = false; return; }
    const topKey = sessionKey(top);
    const shown = this.current ? sessions.get(this.current) : undefined;
    const unpinned = this.wasPinned; this.wasPinned = false;
    if (!shown || unpinned || topKey === this.current || top.lastInteraction > this.chosenAt || this.settleMs <= 0) {
      return this.commit(topKey, now, sessions);
    }
    if (this.candidate !== topKey) { this.candidate = topKey; this.candidateSince = now; }
    if (now - this.candidateSince >= this.settleMs) return this.commit(topKey, now, sessions);
    return shown;
  }
  private commit(key: string, now: number, sessions: Map<string, Session>): Session | undefined {
    if (this.current !== key) { this.current = key; this.chosenAt = now; }
    this.candidate = null;
    return sessions.get(key);
  }
}
