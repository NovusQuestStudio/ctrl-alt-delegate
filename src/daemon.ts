import {createServer, type Socket} from 'node:net';
import {mkdir, chmod, lstat, unlink} from 'node:fs/promises';
import {dirname} from 'node:path';
import {socketPath, request} from './client.js';
import {SessionStore} from './store.js';
import {Selector, type Focus} from './selector.js';
import {DiscordClient, activityFor, type Activity, type DiscordProfile} from './discord.js';
import {sessionKey} from './protocol.js';
import {DISCORD_CLIENT_ID} from './constants.js';
import {DEFAULT_SETTINGS, loadSettings, saveSettings, type Settings} from './settings.js';
import {ancestry, findAgent, systemProcessTable, type Snapshot} from './process-table.js';
import {readState, statePath, StateWriter} from './persist.js';
export interface Transport {status: string; readonly acknowledged?: boolean; readonly user?: DiscordProfile | null; start(): void; setActivity(activity: Activity | null): void; stop(): void}
export interface ProcessSource {snapshot: Snapshot; warm?(): Promise<unknown>; close?(): void}
export interface DaemonOptions {
  clientId?: string; dryRun?: boolean; path?: string; staleMs?: number;
  transport?: Transport; log?: (message: string) => void;
  /** Override settings loading (tests). */
  settings?: () => Settings;
  /** Process table used to find agent processes; null disables lookups. Defaults to the system table. */
  processes?: ProcessSource | null;
  /** Where sessions are saved across restarts; null disables persistence. */
  statePath?: string | null;
  /** How long a non-interactive change must stay on top before the card switches. */
  settleMs?: number;
  /** Persist a settings patch (tests). Defaults to writing the settings file. */
  saveSettings?: (patch: unknown) => Promise<unknown>;
}
const LOOKUP_TIMEOUT_MS = 1000;
function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Timed out')), ms);
    promise.then(value => { clearTimeout(timer); resolve(value); }, error => { clearTimeout(timer); reject(error); });
  });
}
export async function startDaemon(options: DaemonOptions = {}) {
  const path = options.path ?? socketPath();
  const log = options.log ?? console.log;
  const clientId = options.clientId ?? DISCORD_CLIENT_ID;
  if (!options.dryRun && !options.transport && !/^\d{17,20}$/.test(clientId)) throw new Error('Invalid Discord Application ID');
  if (process.platform !== 'win32') {
    await mkdir(dirname(path), {recursive: true, mode: 0o700});
    try {
      const info = await lstat(path);
      if (!info.isSocket() || info.uid !== process.getuid?.()) throw new Error('Refusing to replace an unowned or non-socket path');
      try { await request({type: 'status'}, path); throw new Error('Presence service is already running'); }
      catch (error) {
        const code = (error as NodeJS.ErrnoException).code;
        if (code !== 'ECONNREFUSED' && code !== 'ENOENT') throw error;
      }
      await unlink(path);
    } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; }
  }
  const readSettings = options.settings ?? (() => loadSettings());
  let settings: Settings = {...DEFAULT_SETTINGS};
  let settingsError: string | null = null;
  function reloadSettings() {
    try { settings = readSettings(); settingsError = null; }
    catch (error) { settingsError = `Invalid settings file; keeping the last valid configuration (${(error as Error).message})`; }
  }
  reloadSettings();
  const store = new SessionStore(options.staleMs ?? settings.staleMinutes * 60_000);
  const selector = new Selector(options.settleMs ?? 5000);
  const processes = options.processes === undefined ? systemProcessTable() : options.processes;
  const stateFile = options.statePath === undefined ? statePath() : options.statePath;
  const writer = stateFile ? new StateWriter(stateFile) : undefined;
  let savedRevision = -1;
  let focus: Focus | null = null;
  let paused = false;
  let lastDry: string | undefined;
  const transport: Transport = options.transport ?? (options.dryRun ? {
    status: 'dry-run', start() {}, stop() {}, setActivity(activity) {
      const value = JSON.stringify(activity); if (lastDry !== value) { lastDry = value; log(value); }
    },
  } : new DiscordClient(clientId, undefined, undefined, log));
  const sockets = new Set<Socket>();
  function selected() { return selector.pick(store.sessions, store.pinned, focus); }
  function currentActivity() {
    const session = selected();
    return paused || !settings.enabled ? null : activityFor(session, settings);
  }
  function persist() {
    if (!writer || store.revision === savedRevision) return;
    savedRevision = store.revision; void writer.save(store.snapshot());
  }
  function refresh() { reloadSettings(); store.sweep(); transport.setActivity(currentActivity()); persist(); }
  /** Find the long-lived agent process above a hook's parent, while that parent is still alive. */
  async function attachAgent(key: string, ppid: number) {
    if (!processes) return;
    try {
      const chain = ancestry(await withTimeout(processes.snapshot(), LOOKUP_TIMEOUT_MS), ppid);
      const agent = findAgent(chain);
      if (agent) store.attach(key, {agentPid: agent.pid, agentName: agent.name, ancestors: chain.slice(chain.indexOf(agent)).map(p => p.pid)});
    } catch { /* Fall back to time-based expiry. */ }
  }
  let closing: Promise<void> | undefined;
  const server = createServer(socket => {
    socket.setEncoding('utf8');
    sockets.add(socket); socket.on('close', () => sockets.delete(socket));
    socket.on('error', () => {}); socket.setTimeout(3000, () => socket.destroy());
    let buffer = ''; let handled = false;
    socket.on('data', async data => {
      if (handled) return;
      buffer += data.toString();
      if (Buffer.byteLength(buffer) > 32768) { socket.destroy(); return; }
      const end = buffer.indexOf('\n'); if (end < 0) return;
      handled = true;
      try {
        const message = JSON.parse(buffer.slice(0, end));
        if (closing) throw new Error('Presence service is stopping');
        if (message.type === 'stop') { socket.setTimeout(0); void close(socket); return; }
        store.sweep();
        if (message.type === 'event') {
          const session = store.update(message.event);
          const ppid = message.event?.ppid;
          if (session && session.agentPid === undefined && Number.isSafeInteger(ppid) && ppid > 0) await attachAgent(sessionKey(session), ppid);
        }
        else if (message.type === 'pin') store.pin(message.key);
        else if (message.type === 'pause') paused = message.paused === true;
        else if (message.type === 'settings') await (options.saveSettings ?? saveSettings)(message.patch);
        else if (message.type === 'focus') {
          const {pid, key} = message;
          if (pid !== undefined && (!Number.isSafeInteger(pid) || pid <= 0)) throw new Error('Invalid focus pid');
          if (key !== undefined && key !== null && (typeof key !== 'string' || key.length > 300)) throw new Error('Invalid focus key');
          focus = pid === undefined && (key === undefined || key === null) ? null : {...(pid !== undefined ? {pid} : {}), ...(typeof key === 'string' ? {key} : {}), at: Date.now()};
        }
        else if (message.type !== 'status') throw new Error('Unknown request type');
        refresh();
        const shown = selected();
        socket.end(JSON.stringify(message.type === 'status' ? {
          ok: true, discord: transport.status, profile: transport.user ?? null, activityAcknowledged: transport.acknowledged ?? null,
          enabled: settings.enabled, showProject: settings.showProject, paused, pinned: store.pinned, focus, pid: process.pid, settingsError,
          activity: currentActivity(), selected: shown ? sessionKey(shown) : null,
          sessions: [...store.sessions.values()].map(s => ({key: sessionKey(s), agent: s.agent, edition: s.edition ?? null, state: s.state, project: s.project,
            startedAt: s.startedAt, updatedAt: s.updatedAt, lastSeen: s.lastSeen, lastInteraction: s.lastInteraction,
            agentPid: s.agentPid ?? null, agentName: s.agentName ?? null})),
        } : {ok: true}) + '\n');
      } catch (error) { if (!socket.destroyed) socket.end(JSON.stringify({ok: false, error: (error as Error).message}) + '\n'); }
    });
  });
  server.maxConnections = 64;
  await new Promise<void>((resolve, reject) => {
    server.once('error', error => reject((error as NodeJS.ErrnoException).code === 'EADDRINUSE' ? new Error('Presence service is already running') : error));
    server.listen(path, () => { server.removeAllListeners('error'); resolve(); });
  });
  if (process.platform !== 'win32') await chmod(path, 0o600);
  server.on('error', error => log(`Presence service: ${error.message}`));
  // Restore sessions from the previous run so timers survive restarts; dead agents are swept out.
  if (stateFile) store.restore(readState(stateFile));
  savedRevision = store.revision;
  if (processes) {
    void processes.warm?.()?.catch(() => {});
    // A restored pid may have been reused by an unrelated program; drop sessions whose process name changed.
    const restored = [...store.sessions.values()].filter(s => s.agentPid);
    if (restored.length) {
      void withTimeout(processes.snapshot(), 5000).then(table => {
        for (const s of restored) if (table.get(s.agentPid!)?.name !== s.agentName) store.drop(sessionKey(s));
        refresh();
      }, () => {});
    }
  }
  transport.start(); refresh();
  const ticker = setInterval(refresh, 1000);
  function close(reply?: Socket): Promise<void> {
    return closing ??= (async () => {
      clearInterval(ticker); transport.stop(); processes?.close?.();
      persist(); await writer?.idle();
      const closed = new Promise<void>(resolve => server.close(() => resolve()));
      for (const socket of sockets) if (socket !== reply) socket.destroy();
      reply?.end(JSON.stringify({ok: true}) + '\n', () => reply.destroy());
      await closed;
    })();
  }
  return {path, store, close: () => close()};
}
