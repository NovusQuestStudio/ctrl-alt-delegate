import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:net';
import {spawn} from 'node:child_process';
import {join} from 'node:path';
import {DiscordClient, Decoder, frame} from '../dist/discord.js';
import {startDaemon} from '../dist/daemon.js';
import {request} from '../dist/client.js';
import {DEFAULT_SETTINGS} from '../dist/settings.js';
import {WindowsProcessTable} from '../dist/process-table.js';
import {ipcEndpoint, until, recorder} from './helpers.js';

const CLIENT_ID = '1572816615429640333';

test('decoder handles split and concatenated frames; refuses oversized frames', () => {
  const decoder = new Decoder(); const one = frame(1, {evt: 'READY'}); const two = frame(3, {nonce: 'ping'});
  assert.deepEqual(decoder.feed(one.subarray(0, 5)), []);
  assert.deepEqual(decoder.feed(Buffer.concat([one.subarray(5), two])), [{op: 1, body: {evt: 'READY'}}, {op: 3, body: {nonce: 'ping'}}]);
  const bad = Buffer.alloc(8); bad.writeUInt32LE(2 ** 30, 4); assert.throws(() => decoder.feed(bad));
});

test('Discord reconnection: handshake, ACK, ping, coalescing, replay after drop, and clear on stop', async t => {
  const {path, cleanup} = await ipcEndpoint('discord');
  const messages = []; const sockets = new Set(); let connections = 0;
  const server = createServer(socket => {
    connections++; sockets.add(socket); socket.on('close', () => sockets.delete(socket)); socket.on('error', () => {});
    const decoder = new Decoder();
    socket.on('data', data => {
      for (const packet of decoder.feed(data)) {
        messages.push(packet);
        if (packet.op === 0) {
          assert.equal(packet.body.client_id, CLIENT_ID);
          const ready = frame(1, {evt: 'READY', data: {user: {id: '123456789012345678', username: 'presence-test', email: 'must-not-retain@example.com'}}});
          socket.write(ready.subarray(0, 3)); socket.write(ready.subarray(3));
          socket.write(frame(3, {nonce: 'alive'}));
        }
        if (packet.body.cmd === 'SET_ACTIVITY') socket.write(frame(1, {cmd: 'SET_ACTIVITY', nonce: packet.body.nonce, data: null}));
      }
    });
  });
  await new Promise(resolve => server.listen(path, resolve));
  const client = new DiscordClient(CLIENT_ID, [path], 50);
  t.after(async () => { client.stop(); for (const s of sockets) s.destroy(); await new Promise(r => server.close(r)); await cleanup(); });
  const activity = {type: 0, name: 'Ctrl Alt Delegate', details: 'Codex · Working', state: 'Working', timestamps: {start: 1000}};
  client.setActivity(activity); client.start();
  await until(() => messages.some(m => m.body.cmd === 'SET_ACTIVITY'));
  assert.deepEqual(client.user, {id: '123456789012345678', username: 'presence-test'});
  await until(() => messages.some(m => m.op === 4 && m.body.nonce === 'alive'));
  client.setActivity({...activity, state: 'Waiting'}); client.setActivity({...activity, state: 'Idle'});
  await until(() => messages.some(m => m.body.args?.activity?.state === 'Idle'));
  assert.equal(messages.some(m => m.body.args?.activity?.state === 'Waiting'), false);
  for (const socket of sockets) socket.destroy(); // Discord restarts or drops the pipe.
  await until(() => connections >= 2 && messages.filter(m => m.body.args?.activity?.state === 'Idle').length >= 2);
  client.stop(); await until(() => messages.some(m => m.body.args?.activity === null));
});

test('daemon + real CLI hooks: switching, privacy, pause, pin, and closure', async t => {
  const {path, cleanup} = await ipcEndpoint('daemon');
  const {activity, transport} = recorder();
  let settings = {...DEFAULT_SETTINGS};
  const daemon = await startDaemon({path, transport, settings: () => settings, processes: null, statePath: null, settleMs: 0});
  t.after(async () => { await daemon.close(); await cleanup(); });
  await assert.rejects(startDaemon({path, transport, processes: null, statePath: null}), /already running/);
  async function hook(agent, name, extra = {}) {
    const child = spawn(process.execPath, ['dist/cli.js', 'hook', agent, '--socket', path], {stdio: ['pipe', 'pipe', 'pipe']});
    let output = ''; child.stdout.on('data', d => { output += d; }); child.stderr.on('data', d => { output += d; });
    child.stdin.end(JSON.stringify({session_id: `${agent}-1`, hook_event_name: name, cwd: 'C:\\clients\\secret-project', prompt: 'never publish this', ...extra}));
    const code = await new Promise(resolve => child.on('close', resolve)); assert.equal(code, 0); assert.equal(output, '');
  }
  await hook('claude-code', 'UserPromptSubmit');
  assert.equal((await request({type: 'status'}, path)).selected, 'claude-code:claude-code-1');
  assert.equal(activity.at(-1).details, 'Claude Code · Working');
  await hook('codex', 'UserPromptSubmit');
  assert.equal(activity.at(-1).details, 'Codex · Working');
  await hook('codex', 'Stop');
  assert.equal(activity.at(-1).details, 'Claude Code · Working', 'a working agent outranks an idle one');
  await request({type: 'pin', key: 'codex:codex-1'}, path);
  assert.equal(activity.at(-1).details, 'Codex · Ready for a prompt');
  await request({type: 'pin', key: null}, path);
  await request({type: 'pause', paused: true}, path);
  assert.equal(activity.at(-1), null);
  assert.equal((await request({type: 'status'}, path)).paused, true);
  await request({type: 'pause', paused: false}, path);
  assert.equal(activity.at(-1).details, 'Claude Code · Working');
  settings = {...settings, showProject: true};
  await until(() => activity.at(-1)?.state === 'secret-project');
  settings = {...settings, showProject: false, enabled: false};
  await until(() => activity.at(-1) === null);
  settings = {...settings, enabled: true};
  await hook('claude-code', 'SessionEnd'); await hook('codex', 'SessionEnd');
  assert.equal(activity.at(-1), null);
  await assert.rejects(request({type: 'event', event: {version: 5}}, path), /Unsupported/);
  const published = JSON.stringify(activity.filter(a => a?.state !== 'secret-project'));
  assert.equal(published.includes('secret-project'), false);
  assert.equal(JSON.stringify(activity).includes('never publish'), false);
  assert.equal(JSON.stringify(activity).includes('clients'), false);
});

test('invalid settings keep the last valid configuration', async t => {
  const {path, cleanup} = await ipcEndpoint('settings');
  const {transport} = recorder();
  let fail = false;
  const daemon = await startDaemon({path, transport, processes: null, statePath: null, settings: () => { if (fail) throw new Error('bad json'); return {...DEFAULT_SETTINGS, showProject: true}; }});
  t.after(async () => { await daemon.close(); await cleanup(); });
  fail = true;
  const status = await request({type: 'status'}, path);
  assert.match(status.settingsError, /Invalid settings/);
  await request({type: 'event', event: {version: 1, agent: 'codex', sessionId: 's', state: 'working', updatedAt: Date.now(), project: 'kept'}}, path);
  assert.equal((await request({type: 'status'}, path)).activity.state, 'kept');
});

test('CLI stop clears the transport, closes the endpoint, and succeeds when already stopped; restart works', async t => {
  const {path, cleanup} = await ipcEndpoint('stop');
  const {activity, transport} = recorder();
  const daemon = await startDaemon({path, transport, processes: null, statePath: null, settings: () => ({...DEFAULT_SETTINGS})});
  t.after(async () => { await daemon.close(); await cleanup(); });
  async function stop() {
    const child = spawn(process.execPath, ['dist/cli.js', 'stop', '--socket', path], {stdio: ['ignore', 'pipe', 'pipe']});
    let output = ''; let error = '';
    child.stdout.on('data', d => { output += d; }); child.stderr.on('data', d => { error += d; });
    assert.equal(await new Promise(resolve => child.on('close', resolve)), 0, error);
    return output;
  }
  assert.match(await stop(), /Ctrl Alt Delegate stopped/);
  await daemon.close();
  assert.equal(activity.at(-1), null);
  await assert.rejects(request({type: 'status'}, path), e => ['ENOENT', 'ECONNREFUSED'].includes(e.code));
  assert.match(await stop(), /not running/);
  const replacement = await startDaemon({path, dryRun: true, log() {}, processes: null, statePath: null, settings: () => ({...DEFAULT_SETTINGS})});
  await replacement.close();
});

test('background start releases the terminal, rejects duplicates, reports failures, and supports stop', async t => {
  const {dir, path, cleanup} = await ipcEndpoint('background');
  const env = {...process.env, CAD_STATE: join(dir, 'sessions.json'), CAD_CONFIG: join(dir, 'config.json')};
  t.after(async () => { try { await request({type: 'stop'}, path, 10_000); } catch {} await cleanup(); });
  async function cli(args) {
    const child = spawn(process.execPath, ['dist/cli.js', ...args, '--socket', path], {env, stdio: ['ignore', 'pipe', 'pipe']});
    let output = ''; let error = '';
    const timer = setTimeout(() => child.kill(), 15_000);
    child.stdout.on('data', d => { output += d; }); child.stderr.on('data', d => { error += d; });
    const code = await new Promise(resolve => child.on('close', resolve));
    clearTimeout(timer);
    return {code, output, error};
  }
  const started = await cli(['start', '-b', '--dry-run']);
  assert.equal(started.code, 0, started.error);
  assert.match(started.output, /started in background.*PID \d+/);
  const status = await request({type: 'status'}, path);
  assert.equal(status.discord, 'dry-run');
  assert.notEqual(status.pid, process.pid);
  const duplicate = await cli(['start', '--background', '--dry-run']);
  assert.equal(duplicate.code, 1);
  assert.match(duplicate.error, /already running/);
  assert.equal((await cli(['stop'])).code, 0);
  const invalid = await cli(['start', '-b', '--client-id', 'invalid']);
  assert.equal(invalid.code, 1);
  assert.match(invalid.error, /Invalid Discord Application ID/);
});

test('hook sessions attach to the agent process and clear as soon as it exits', async t => {
  const {path, cleanup} = await ipcEndpoint('agent');
  const {activity, transport} = recorder();
  // A stand-in for the agent: the hook's parent (this test process) plays the shell, `agent` plays claude.exe.
  const agent = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {stdio: 'ignore'});
  const table = new Map([[process.pid, {pid: process.pid, ppid: agent.pid, name: 'bash.exe'}], [agent.pid, {pid: agent.pid, ppid: 4, name: 'claude.exe'}], [4, {pid: 4, ppid: 0, name: 'WindowsTerminal.exe'}]]);
  let lookups = 0;
  const daemon = await startDaemon({path, transport, settings: () => ({...DEFAULT_SETTINGS}), statePath: null, settleMs: 0,
    processes: {snapshot: async () => { lookups++; return table; }}});
  t.after(async () => { agent.kill(); await daemon.close(); await cleanup(); });
  const child = spawn(process.execPath, ['dist/cli.js', 'hook', 'claude-code', '--socket', path], {stdio: ['pipe', 'ignore', 'ignore']});
  child.stdin.end(JSON.stringify({session_id: 'live', hook_event_name: 'UserPromptSubmit'}));
  await new Promise(resolve => child.on('close', resolve));
  let status = await request({type: 'status'}, path);
  // The real CLI hook's parent is this test process ("bash.exe" in the fake table), whose parent is the agent.
  assert.equal(status.selected, 'claude-code:live');
  assert.equal(status.sessions[0].agentPid, agent.pid);
  await request({type: 'event', event: {version: 1, agent: 'claude-code', sessionId: 'direct', state: 'working', updatedAt: Date.now(), ppid: process.pid}}, path);
  status = await request({type: 'status'}, path);
  const direct = status.sessions.find(s => s.key === 'claude-code:direct');
  assert.equal(direct.agentPid, agent.pid); assert.equal(direct.agentName, 'claude.exe');
  assert.ok(lookups >= 1);
  await request({type: 'event', event: {version: 1, agent: 'claude-code', sessionId: 'direct', state: 'idle', updatedAt: Date.now(), ppid: process.pid}}, path);
  const before = lookups;
  await request({type: 'event', event: {version: 1, agent: 'claude-code', sessionId: 'direct', state: 'working', updatedAt: Date.now(), ppid: process.pid}}, path);
  assert.equal(lookups, before, 'the lookup runs once per session, not per event');
  await request({type: 'focus', pid: 4}, path);
  assert.equal((await request({type: 'status'}, path)).focus.pid, 4);
  agent.kill();
  await until(async () => !(await request({type: 'status'}, path)).sessions.some(s => s.key === 'claude-code:direct'));
  assert.equal((await request({type: 'status'}, path)).sessions.length, 0, 'both sessions belonged to the exited agent');
  assert.equal(JSON.stringify(activity).includes('claude.exe'), false);
});

test('application restart keeps session timers; a reused pid is not mistaken for the agent', async t => {
  const {dir, path, cleanup} = await ipcEndpoint('restart');
  const statePath = join(dir, 'sessions.json');
  const agent = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {stdio: 'ignore'});
  t.after(async () => { agent.kill(); await cleanup(); });
  const named = name => ({snapshot: async () => new Map([[process.pid, {pid: process.pid, ppid: agent.pid, name: 'bash.exe'}], [agent.pid, {pid: agent.pid, ppid: 0, name}]])});
  const options = {path, transport: recorder().transport, settings: () => ({...DEFAULT_SETTINGS}), statePath, settleMs: 0};
  const first = await startDaemon({...options, processes: named('codex.exe')});
  const startedAt = Date.now() - 60_000;
  await request({type: 'event', event: {version: 1, agent: 'codex', sessionId: 'keep', state: 'working', updatedAt: startedAt, interaction: true, ppid: process.pid}}, path);
  await request({type: 'event', event: {version: 1, agent: 'claude-code', sessionId: 'hookonly', state: 'idle', updatedAt: startedAt}}, path);
  await request({type: 'pin', key: 'codex:keep'}, path);
  await first.close();
  const second = await startDaemon({...options, processes: named('codex.exe')});
  let status = await request({type: 'status'}, path);
  assert.deepEqual(status.sessions.map(s => s.key).sort(), ['claude-code:hookonly', 'codex:keep']);
  assert.equal(status.sessions.find(s => s.key === 'codex:keep').startedAt, startedAt);
  assert.equal(status.pinned, 'codex:keep');
  assert.equal(status.activity.timestamps.start, Math.floor(startedAt / 1000));
  await second.close();
  const third = await startDaemon({...options, processes: named('notepad.exe')});
  await until(async () => !(await request({type: 'status'}, path)).sessions.some(s => s.key === 'codex:keep'));
  status = await request({type: 'status'}, path);
  assert.deepEqual(status.sessions.map(s => s.key), ['claude-code:hookonly']);
  await third.close();
});

test('focus messages are validated and change the selection', async t => {
  const {path, cleanup} = await ipcEndpoint('focus');
  const daemon = await startDaemon({path, transport: recorder().transport, settings: () => ({...DEFAULT_SETTINGS}), processes: null, statePath: null, settleMs: 0});
  t.after(async () => { await daemon.close(); await cleanup(); });
  const now = Date.now();
  await request({type: 'event', event: {version: 1, agent: 'vscode', sessionId: 'win1', state: 'idle', updatedAt: now - 10, pid: process.pid}}, path);
  await request({type: 'event', event: {version: 1, agent: 'codex', sessionId: 'c', state: 'working', updatedAt: now, interaction: true}}, path);
  assert.equal((await request({type: 'status'}, path)).selected, 'codex:c');
  await request({type: 'focus', key: 'vscode:win1'}, path);
  assert.equal((await request({type: 'status'}, path)).selected, 'vscode:win1');
  await request({type: 'focus', key: null}, path);
  assert.equal((await request({type: 'status'}, path)).selected, 'codex:c');
  await assert.rejects(request({type: 'focus', pid: -1}, path), /Invalid focus pid/);
  await assert.rejects(request({type: 'focus', key: 42}, path), /Invalid focus key/);
});

test('Windows process table: real Toolhelp snapshot sees this process and its parent', {skip: process.platform !== 'win32'}, async () => {
  const table = new WindowsProcessTable();
  try {
    const snapshot = await table.snapshot();
    assert.ok(snapshot.size > 10);
    assert.equal(snapshot.get(process.pid)?.ppid, process.ppid);
    assert.match(snapshot.get(process.pid)?.name ?? '', /^node(\.exe)?$/i);
    await new Promise(resolve => setTimeout(resolve, 300)); // Past the 250 ms cache.
    const warm = performance.now(); await table.snapshot();
    assert.ok(performance.now() - warm < 1000, 'a warm query is fast');
  } finally { table.close(); }
});
