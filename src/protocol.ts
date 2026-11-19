export const agents = ['claude-code', 'codex', 'vscode'] as const;
export type Agent = typeof agents[number];
export type State = 'idle' | 'working' | 'waiting' | 'error' | 'closed';
export interface PresenceEvent {
  version: 1;
  agent: Agent;
  sessionId: string;
  state: State;
  /** Time of the last state event; heartbeats retain this value. */
  updatedAt: number;
  /** Only long-lived reporters running in the tool's own process should supply this. */
  pid?: number;
  heartbeat?: boolean;
  /** A tool or permission event continues the current task rather than starting a new one. */
  continuation?: boolean;
  /** A deliberate user action (prompt, session start, editor activity). Drives selection. */
  interaction?: boolean;
  /**
   * Parent pid of a short-lived hook process. Transient: the service uses it once to find the
   * long-lived agent process (for liveness and focus) and never retains it.
   */
  ppid?: number;
  /**
   * Folder name only (never a path). Kept locally; shown in Discord only when the
   * user enables `showProject`.
   */
  project?: string;
  /** Which build of the editor reported this (VS Code family only). Absent means VS Code stable. */
  edition?: Edition;
}
export const labels: Record<Agent, string> = {'claude-code': 'Claude Code', codex: 'Codex', vscode: 'VS Code'};
export const editions = ['insiders', 'vscodium', 'cursor'] as const;
export type Edition = typeof editions[number];
const editionLabels: Record<Edition, string> = {insiders: 'VS Code Insiders', vscodium: 'VSCodium', cursor: 'Cursor'};
/** Display name for a session's tool, including the editor edition. */
export function toolLabel(e: Pick<PresenceEvent, 'agent' | 'edition'>): string {
  return e.agent === 'vscode' && e.edition ? editionLabels[e.edition] : labels[e.agent];
}
/** Map VS Code's `env.appName` to an edition; unknown names count as stable. */
export function editionFromAppName(appName: string): Edition | undefined {
  if (/insiders/i.test(appName)) return 'insiders';
  if (/vscodium/i.test(appName)) return 'vscodium';
  if (/cursor/i.test(appName)) return 'cursor';
  return undefined;
}
const states: readonly State[] = ['idle', 'working', 'waiting', 'error', 'closed'];
/** Reduce a path to a safe display name: last segment, no control characters, bounded. */
export function projectName(value: unknown): string | undefined {
  if (typeof value !== 'string' || !value) return;
  const name = value.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? '';
  const clean = Array.from(name.replace(/[\u0000-\u001f\u007f]/g, '')).slice(0, 64).join('').trim();
  return clean && !/^[A-Za-z]:$/.test(clean) ? clean : undefined;
}
export function parseEvent(value: unknown, now = Date.now()): PresenceEvent {
  if (!value || typeof value !== 'object') throw new Error('Expected event object');
  const e = value as Record<string, unknown>;
  if (e.version !== 1 || !agents.includes(e.agent as Agent)) throw new Error('Unsupported protocol or agent');
  if (typeof e.sessionId !== 'string' || !e.sessionId.length || e.sessionId.length > 256) throw new Error('Invalid sessionId');
  if (!states.includes(e.state as State)) throw new Error('Invalid state');
  if (typeof e.updatedAt !== 'number' || !Number.isSafeInteger(e.updatedAt) || e.updatedAt < 0 || e.updatedAt > now + 60_000) throw new Error('Invalid updatedAt');
  if (e.pid !== undefined && (!Number.isSafeInteger(e.pid) || (e.pid as number) <= 0)) throw new Error('Invalid pid');
  if (e.heartbeat !== undefined && typeof e.heartbeat !== 'boolean') throw new Error('Invalid heartbeat');
  for (const key of ['continuation', 'interaction'] as const) if (e[key] !== undefined && typeof e[key] !== 'boolean') throw new Error(`Invalid ${key}`);
  if (e.ppid !== undefined && (!Number.isSafeInteger(e.ppid) || (e.ppid as number) <= 0)) throw new Error('Invalid ppid');
  if (e.project !== undefined && (typeof e.project !== 'string' || projectName(e.project) !== e.project)) throw new Error('Invalid project');
  if (e.edition !== undefined && (e.agent !== 'vscode' || !editions.includes(e.edition as Edition))) throw new Error('Invalid edition');
  // Whitelist fields: never retain prompts, paths, file names, or tool arguments.
  return {version: 1, agent: e.agent as Agent, sessionId: e.sessionId, state: e.state as State, updatedAt: e.updatedAt,
    pid: e.pid as number | undefined, heartbeat: e.heartbeat as boolean | undefined,
    continuation: e.continuation as boolean | undefined, interaction: e.interaction as boolean | undefined,
    ppid: e.ppid as number | undefined, project: e.project as string | undefined, edition: e.edition as Edition | undefined};
}
export function sessionKey(e: Pick<PresenceEvent, 'agent' | 'sessionId'>): string {
  return `${e.agent}:${e.sessionId}`;
}
