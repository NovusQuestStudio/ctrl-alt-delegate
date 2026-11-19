#!/usr/bin/env node
import {parseArgs} from 'node:util';
import {fileURLToPath} from 'node:url';
import {startDaemon} from './daemon.js';
import {request, socketPath} from './client.js';
import {runHook} from './adapters/hooks.js';
import {APP_NAME} from './constants.js';
import {settingsPath} from './settings.js';
const help = `${APP_NAME} — one Discord Rich Presence for your coding tools

  start                     Run the presence service in this terminal
  start --background, -b    Start in the background and return to the terminal
  start --dry-run           Print activities without connecting to Discord
  stop                      Stop the service and clear Discord activity
  status                    Show connection, sessions, and the current activity as JSON
  pause | resume            Temporarily hide or restore Discord activity
  pin AGENT:SESSION_ID      Always show one session
  unpin                     Resume automatic selection
  hook claude-code|codex    Receive a lifecycle hook on stdin (used by the plugins)
  socket-path               Print the local service endpoint (used by the tray)

Options: --client-id ID, --socket PATH
Settings file: ${settingsPath()}
Environment: CAD_SOCKET, CAD_CONFIG, DISCORD_IPC_PATH
`;
async function main() {
  const {values, positionals} = parseArgs({allowPositionals: true, options: {
    'client-id': {type: 'string'}, 'dry-run': {type: 'boolean'}, socket: {type: 'string'},
    help: {type: 'boolean', short: 'h'}, background: {type: 'boolean', short: 'b'},
  }});
  const [command, argument] = positionals;
  if (values.socket) process.env.CAD_SOCKET = values.socket;
  if (!command || values.help) { console.log(help); return; }
  if (values.background && command !== 'start') throw new Error('--background is only available for start');
  if (command === 'hook') {
    if (argument === 'codex' || argument === 'claude-code') await runHook(argument);
    return;
  }
  if (command === 'start') {
    if (values.background) {
      const {startBackground} = await import('./background.js');
      const args: string[] = [];
      if (values['client-id']) args.push('--client-id', values['client-id']);
      if (values['dry-run']) args.push('--dry-run');
      const result = await startBackground(fileURLToPath(import.meta.url), args);
      console.log(`${APP_NAME} started in background (PID ${result.pid}).\nLog: ${result.log}`);
      return;
    }
    const daemon = await startDaemon({clientId: values['client-id'] ?? process.env.DISCORD_CLIENT_ID, dryRun: values['dry-run']});
    console.log(`${APP_NAME} listening at ${daemon.path}`);
    const close = () => { void daemon.close(); };
    process.once('SIGINT', close); process.once('SIGTERM', close);
    if (process.send) process.send({type: 'ready'});
    return;
  }
  if (command === 'stop') {
    try {
      await request({type: 'stop'}, undefined, 10_000);
      console.log(`${APP_NAME} stopped.`);
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code === 'ENOENT' || code === 'ECONNREFUSED') console.log(`${APP_NAME} is not running.`);
      else throw error;
    }
  }
  else if (command === 'socket-path') console.log(socketPath());
  else if (command === 'status') console.log(JSON.stringify(await request({type: 'status'}), null, 2));
  else if (command === 'pause' || command === 'resume') await request({type: 'pause', paused: command === 'pause'});
  else if (command === 'pin' && argument) await request({type: 'pin', key: argument});
  else if (command === 'unpin') await request({type: 'pin', key: null});
  else throw new Error('Unknown command. Use --help.');
}
main().catch(error => {
  console.error((error as Error).message); process.exitCode = 1;
  if (process.send) process.send({type: 'startup-error', error: (error as Error).message});
});
