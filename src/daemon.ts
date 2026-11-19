import {createServer, type Socket} from 'node:net';
import {mkdir, chmod, lstat, unlink} from 'node:fs/promises';
import {dirname} from 'node:path';
import {socketPath, request} from './client.js';
import {SessionStore} from './store.js';
import {DiscordClient, activityFor, type Activity, type DiscordProfile} from './discord.js';
import {sessionKey} from './protocol.js';
import {DISCORD_CLIENT_ID} from './constants.js';
import {DEFAULT_SETTINGS, loadSettings, type Settings} from './settings.js';
export interface Transport {status: string; readonly acknowledged?: boolean; readonly user?: DiscordProfile | null; start(): void; setActivity(activity: Activity | null): void; stop(): void}
export interface DaemonOptions {
  clientId?: string; dryRun?: boolean; path?: string; staleMs?: number;
  transport?: Transport; log?: (message: string) => void;
  /** Override settings loading (tests). */
  settings?: () => Settings;
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
  let paused = false;
  let lastDry: string | undefined;
  const transport: Transport = options.transport ?? (options.dryRun ? {
    status: 'dry-run', start() {}, stop() {}, setActivity(activity) {
      const value = JSON.stringify(activity); if (lastDry !== value) { lastDry = value; log(value); }
    },
  } : new DiscordClient(clientId, undefined, undefined, log));
  const sockets = new Set<Socket>();
  function currentActivity() {
    return paused || !settings.enabled ? null : activityFor(store.select(), settings);
  }
  function refresh() { reloadSettings(); store.sweep(); transport.setActivity(currentActivity()); }
  let closing: Promise<void> | undefined;
  const server = createServer(socket => {
    socket.setEncoding('utf8');
    sockets.add(socket); socket.on('close', () => sockets.delete(socket));
    socket.on('error', () => {}); socket.setTimeout(2000, () => socket.destroy());
    let buffer = ''; let handled = false;
    socket.on('data', data => {
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
        if (message.type === 'event') store.update(message.event);
        else if (message.type === 'pin') store.pin(message.key);
        else if (message.type === 'pause') paused = message.paused === true;
        else if (message.type !== 'status') throw new Error('Unknown request type');
        refresh();
        const selected = store.select();
        socket.end(JSON.stringify(message.type === 'status' ? {
          ok: true, discord: transport.status, profile: transport.user ?? null, activityAcknowledged: transport.acknowledged ?? null,
          enabled: settings.enabled, paused, pinned: store.pinned, pid: process.pid, settingsError,
          activity: currentActivity(), selected: selected ? sessionKey(selected) : null,
          sessions: [...store.sessions.values()].map(s => ({key: sessionKey(s), agent: s.agent, state: s.state, project: s.project, startedAt: s.startedAt, updatedAt: s.updatedAt, lastSeen: s.lastSeen})),
        } : {ok: true}) + '\n');
      } catch (error) { socket.end(JSON.stringify({ok: false, error: (error as Error).message}) + '\n'); }
    });
  });
  server.maxConnections = 64;
  await new Promise<void>((resolve, reject) => {
    server.once('error', error => reject((error as NodeJS.ErrnoException).code === 'EADDRINUSE' ? new Error('Presence service is already running') : error));
    server.listen(path, () => { server.removeAllListeners('error'); resolve(); });
  });
  if (process.platform !== 'win32') await chmod(path, 0o600);
  server.on('error', error => log(`Presence service: ${error.message}`));
  transport.start(); refresh();
  const ticker = setInterval(refresh, 1000);
  function close(reply?: Socket): Promise<void> {
    return closing ??= (async () => {
      clearInterval(ticker); transport.stop();
      const closed = new Promise<void>(resolve => server.close(() => resolve()));
      for (const socket of sockets) if (socket !== reply) socket.destroy();
      reply?.end(JSON.stringify({ok: true}) + '\n', () => reply.destroy());
      await closed;
    })();
  }
  return {path, store, close: () => close()};
}
