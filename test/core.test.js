import test from 'node:test';
import assert from 'node:assert/strict';
import {SessionStore} from '../dist/store.js';
import {Selector, rank} from '../dist/selector.js';
import {parseEvent, projectName, toolLabel, editionFromAppName} from '../dist/protocol.js';
import {activityFor} from '../dist/presentation.js';
import {hookEvent} from '../dist/adapters/hooks.js';
import {Reporter} from '../dist/client.js';
import {validateSettings, DEFAULT_SETTINGS} from '../dist/settings.js';
import {ancestry, findAgent, parseTable} from '../dist/process-table.js';
const e = (sessionId, state, updatedAt = 1000, extra = {}) => ({version: 1, agent: 'codex', sessionId, state, updatedAt, ...extra});
const top = store => rank(store.sessions.values())[0];

test('working outranks idle; heartbeat does not steal selection; pin overrides', () => {
  const store = new SessionStore(); const selector = new Selector(0);
  const pick = () => selector.pick(store.sessions, store.pinned, null);
  store.update(e('a', 'working'), 1000);
  store.update(e('b', 'idle', 2000), 2000);
  assert.equal(pick().sessionId, 'a');
  store.update(e('b', 'working', 3000, {interaction: true}), 3000);
  store.update(e('a', 'working', 1000, {heartbeat: true}), 4000);
  assert.equal(pick().sessionId, 'b');
  store.pin('codex:a'); assert.equal(pick().sessionId, 'a');
  store.update(e('a', 'closed', 5000), 5000);
  assert.equal(store.pinned, null); assert.equal(pick().sessionId, 'b');
});

test('activity switching: a background agent finishing tools never steals from the session you prompted', () => {
  const store = new SessionStore();
  store.update({...e('bg', 'working', 1000, {interaction: true}), agent: 'claude-code'}, 1000);
  store.update(e('mine', 'working', 2000, {interaction: true}), 2000);
  store.update({...e('bg', 'working', 3000, {continuation: true}), agent: 'claude-code'}, 3000);
  assert.equal(top(store).sessionId, 'mine');
  store.update({...e('bg', 'working', 4000, {interaction: true}), agent: 'claude-code'}, 4000);
  assert.equal(top(store).sessionId, 'bg');
});

test('simultaneous agents: each keeps its own session and timer', () => {
  const store = new SessionStore();
  store.update({...e('c1', 'working', 1000, {interaction: true}), agent: 'claude-code'}, 1000);
  store.update(e('x1', 'working', 5000, {interaction: true}), 5000);
  assert.equal(top(store).agent, 'codex');
  store.update({...e('c1', 'working', 9000, {interaction: true}), agent: 'claude-code'}, 9000);
  assert.equal(top(store).agent, 'claude-code');
  assert.equal(top(store).startedAt, 1000);
  assert.equal(store.sessions.size, 2);
});

test('focus: a session in the focused window wins, by ancestor pid or reporter key', () => {
  const store = new SessionStore();
  store.update({...e('term', 'idle', 1000), agent: 'claude-code'}, 1000);
  store.attach('claude-code:term', {agentPid: 500, agentName: 'claude.exe', ancestors: [500, 400, 300]});
  store.update(e('other', 'working', 2000, {interaction: true}), 2000);
  assert.equal(rank(store.sessions.values(), null)[0].sessionId, 'other');
  assert.equal(rank(store.sessions.values(), {pid: 300, at: 0})[0].sessionId, 'term');
  assert.equal(rank(store.sessions.values(), {pid: 999, at: 0})[0].sessionId, 'other');
  assert.equal(rank(store.sessions.values(), {key: 'claude-code:term', at: 0})[0].sessionId, 'term');
});

test('hysteresis: non-interactive changes wait settleMs; interaction, disappearance, and unpin switch at once', () => {
  const store = new SessionStore(); const selector = new Selector(5000);
  const pick = (now, focus = null) => selector.pick(store.sessions, store.pinned, focus, now)?.sessionId;
  store.update(e('a', 'working', 1000, {interaction: true}), 1000);
  store.update(e('b', 'idle', 900), 1000);
  assert.equal(pick(1000), 'a');
  store.update(e('a', 'idle', 2000), 2000); store.update(e('b', 'working', 2001, {continuation: true}), 2001);
  assert.equal(pick(2001), 'a', 'state flip alone does not switch immediately');
  assert.equal(pick(6000), 'a');
  assert.equal(pick(7001), 'b', 'switches after the candidate stays on top for settleMs');
  store.update(e('a', 'working', 8000, {interaction: true}), 8000);
  assert.equal(pick(8000), 'a', 'a fresh prompt switches immediately');
  store.pin('codex:b'); assert.equal(pick(8001), 'b');
  store.pin(null); assert.equal(pick(8002), 'a', 'unpinning re-ranks immediately');
  store.update(e('a', 'closed', 9000), 9000);
  assert.equal(pick(9000), 'b', 'a closed session is replaced immediately');
});

test('late events cannot regress or resurrect a closed session', () => {
  const store = new SessionStore();
  store.update(e('a', 'idle', 2000), 2000);
  store.update(e('a', 'working', 1000), 2001);
  assert.equal(top(store).state, 'idle');
  store.update(e('a', 'closed', 3000), 3000);
  store.update(e('a', 'working', 2000, {heartbeat: true}), 3001);
  assert.equal(top(store), undefined);
  store.update(e('a', 'working', 4000), 4000);
  assert.equal(top(store).startedAt, 4000);
});

test('stale sessions: leases and hook expiry clear presence; reporter or agent death clears immediately', () => {
  const store = new SessionStore(300_000, pid => pid !== 999 && pid !== 777);
  store.update(e('hook', 'working'), 1000);
  store.update(e('extension', 'working', 1000, {pid: 123}), 1000);
  store.update(e('dead', 'working', 1000, {pid: 999}), 1000);
  store.update(e('agent', 'idle'), 1000);
  store.attach('codex:agent', {agentPid: 777, agentName: 'codex.exe', ancestors: [777]});
  store.sweep(2000);
  assert.equal(store.sessions.has('codex:dead'), false);
  assert.equal(store.sessions.has('codex:agent'), false, 'agent process exit clears the hook session');
  store.sweep(92_000); assert.equal(store.sessions.has('codex:extension'), false);
  assert.equal(top(store).sessionId, 'hook');
  store.sweep(302_000); assert.equal(top(store), undefined);
});

test('application restart: restore keeps timers, drops invalid, closed, stale, and dead entries', () => {
  const now = 1_000_000;
  const before = new SessionStore(300_000, pid => pid !== 13);
  before.update({...e('keep', 'working', 900_000, {interaction: true}), agent: 'claude-code', project: 'app'}, 900_000);
  before.attach('claude-code:keep', {agentPid: 12, agentName: 'claude.exe', ancestors: [12, 11]});
  before.pin('claude-code:keep');
  const saved = JSON.parse(JSON.stringify(before.snapshot()));
  saved.sessions.push({...saved.sessions[0], sessionId: 'dead', agentPid: 13});
  saved.sessions.push({...saved.sessions[0], sessionId: 'old', agentPid: undefined, lastSeen: 1});
  saved.sessions.push({...saved.sessions[0], sessionId: 'bad', startedAt: 'yesterday'});
  saved.sessions.push({...saved.sessions[0], sessionId: 'evil', agent: 'opencode'});
  const after = new SessionStore(300_000, pid => pid !== 13);
  after.restore(saved, now);
  assert.deepEqual([...after.sessions.keys()], ['claude-code:keep']);
  const s = after.sessions.get('claude-code:keep');
  assert.equal(s.startedAt, 900_000); assert.equal(s.agentPid, 12); assert.deepEqual(s.ancestors, [12, 11]);
  assert.equal(after.pinned, 'claude-code:keep');
  after.restore('garbage', now); after.restore({sessions: 'x'}, now);
});

test('process ancestry: the agent is the first non-shell ancestor; host apps stay in the chain', () => {
  const table = parseTable('73280\t9808\tnode.exe\n9808\t60784\tbash.exe\n60784\t34848\tbash.exe\n34848\t31000\tclaude.exe\n31000\t30000\tpwsh.exe\n30000\t4\tWindowsTerminal.exe\n4\t4\tSystem\nbad line', /^(\d+)\t(\d+)\t(.+)$/);
  const chain = ancestry(table, 9808);
  assert.deepEqual(chain.map(p => p.name), ['bash.exe', 'bash.exe', 'claude.exe', 'pwsh.exe', 'WindowsTerminal.exe', 'System']);
  assert.equal(findAgent(chain).pid, 34848);
  assert.equal(findAgent(ancestry(table, 34848)).name, 'claude.exe', 'hook run directly by the agent');
  const loop = parseTable('1 2 a\n2 1 b', /^(\d+)\s+(\d+)\s+(.+)$/);
  assert.equal(ancestry(loop, 1).length, 2);
  assert.equal(findAgent([{pid: 1, ppid: 0, name: 'cmd.exe'}]), undefined);
  assert.equal(parseTable('5 1 /usr/bin/codex', /^(\d+)\s+(\d+)\s+(.+)$/).get(5).name, 'codex');
});

test('protocol whitelists fields and rejects bad data', () => {
  const parsed = parseEvent(e('a', 'idle', 1000, {prompt: 'secret', cwd: '/secret', response: 'x', tool: 'Bash'}), 1000);
  for (const key of ['prompt', 'cwd', 'response', 'tool']) assert.equal(key in parsed, false);
  for (const extra of [{version: 2}, {agent: 'x'}, {agent: 'opencode'}, {state: 'x'}, {updatedAt: Infinity}, {pid: -1}, {ppid: 0}, {interaction: 'yes'}, {sessionId: ''}, {project: 'a/b'}, {project: 'C:\\x'}]) {
    assert.throws(() => parseEvent({...e('a', 'idle'), ...extra}, 1000));
  }
  const store = new SessionStore(); store.update(e('p', 'idle', 1000, {ppid: 42, interaction: true}), 1000);
  assert.equal('ppid' in store.sessions.get('codex:p'), false, 'the hook parent pid is never retained');
});

test('projectName keeps only the folder name', () => {
  assert.equal(projectName('C:\\Users\\me\\Code\\my-app\\'), 'my-app');
  assert.equal(projectName('/home/me/secret-client/project'), 'project');
  assert.equal(projectName('C:\\'), undefined);
  assert.equal(projectName(42), undefined);
});

test('a fresh daemon recovers long-running reporter state on heartbeat', () => {
  const store = new SessionStore(300_000, () => true);
  store.update(e('long-run', 'working', 1000, {pid: 123, heartbeat: true}), 1_000_000);
  assert.equal(top(store).state, 'working');
  assert.equal(top(store).startedAt, 1000);
  store.sweep(1_010_000);
  assert.equal(top(store).sessionId, 'long-run');
});

test('activity is branded, hides the project by default, and never contains session identifiers', () => {
  const store = new SessionStore();
  store.update({...e('secret-id', 'working'), agent: 'claude-code', project: 'client-x'}, 1000);
  store.update({...e('secret-id', 'idle', 3000), agent: 'claude-code'}, 3000);
  const hidden = activityFor(top(store));
  assert.deepEqual(hidden, {type: 0, name: 'Ctrl Alt Delegate', details: 'Claude Code · Ready for a prompt', state: 'Delegating to an AI agent', timestamps: {start: 1}});
  assert.equal(JSON.stringify(hidden).includes('client-x'), false);
  assert.equal(JSON.stringify(hidden).includes('secret-id'), false);
  const shown = activityFor(top(store), {showProject: true, largeImage: 'icon'});
  assert.equal(shown.state, 'client-x');
  assert.deepEqual(shown.assets, {large_image: 'icon', large_text: 'Ctrl Alt Delegate'});
  assert.equal(activityFor(), null);
});

test('hook mapping: states, interactions, compaction, closure, and only the folder name of cwd', () => {
  const ev = (name, extra = {}) => hookEvent('claude-code', {session_id: 's', hook_event_name: name, cwd: 'C:\\work\\my-app', prompt: 'secret', ...extra}, 1000);
  assert.equal(ev('PostToolUse').state, 'working');
  assert.equal(ev('PostToolUse').interaction, undefined);
  assert.equal(ev('UserPromptSubmit').interaction, true);
  assert.equal(ev('SessionStart').interaction, true);
  assert.equal(ev('SessionStart', {source: 'compact'}).state, 'working');
  assert.equal(ev('SessionStart', {source: 'compact'}).interaction, undefined);
  assert.equal(ev('PermissionRequest').state, 'waiting');
  assert.equal(ev('Stop').state, 'idle'); assert.equal(ev('SessionEnd').state, 'closed');
  assert.equal(ev('StopFailure').state, 'error');
  assert.equal(ev('SubagentStop'), undefined);
  assert.equal(ev('Notification', {notification_type: 'auth_success'}), undefined);
  assert.equal(ev('Notification', {notification_type: 'idle_prompt'}), undefined, 'idle reminders keep the session Ready');
  assert.equal(ev('Notification', {notification_type: 'permission_prompt'}).state, 'waiting');
  assert.equal(ev('UserPromptSubmit').project, 'my-app');
  assert.equal(JSON.stringify(ev('UserPromptSubmit')).includes('secret'), false);
});

test('settings default to private values and reject unknown or unsafe values', () => {
  assert.deepEqual(validateSettings({}), DEFAULT_SETTINGS);
  assert.equal(DEFAULT_SETTINGS.showProject, false);
  for (const bad of [{summaries: true}, {largeImage: 'http://x'}, {largeImage: 'a b'}, {staleMinutes: 0}, {enabled: 'yes'}]) assert.throws(() => validateSettings(bad));
  assert.equal(validateSettings({largeImage: 'ctrl_alt_delegate'}).largeImage, 'ctrl_alt_delegate');
});

test('reporter coalesces updates, marks interactions once, closes last, and tolerates an unavailable service', async () => {
  const sent = []; let release;
  const reporter = new Reporter('vscode', 's', async event => {
    sent.push(event);
    if (sent.length === 1) await new Promise(resolve => { release = resolve; });
  });
  reporter.interaction = true; reporter.set('working'); reporter.set('waiting'); reporter.set('idle');
  const closed = reporter.close(); release(); await closed;
  assert.deepEqual(sent.map(e => e.state), ['working', 'closed']);
  assert.equal(sent[0].interaction, true); assert.equal(sent[1].interaction, undefined);
  assert.ok(sent[1].updatedAt > sent[0].updatedAt);
  const failing = new Reporter('vscode', 's', async () => { throw new Error('offline'); });
  failing.set('working'); await failing.close();
});

test('editor editions: validated, labelled on the card, and detected from the app name', () => {
  const v = (extra = {}) => ({version: 1, agent: 'vscode', sessionId: 'w', state: 'working', updatedAt: 1000, ...extra});
  assert.equal(parseEvent(v({edition: 'insiders'}), 1000).edition, 'insiders');
  for (const bad of [{edition: 'nightly'}, {edition: 3}]) assert.throws(() => parseEvent(v(bad), 1000), /Invalid edition/);
  assert.throws(() => parseEvent({...v({edition: 'insiders'}), agent: 'codex'}, 1000), /Invalid edition/);
  assert.equal(toolLabel({agent: 'vscode'}), 'VS Code');
  assert.equal(toolLabel({agent: 'vscode', edition: 'insiders'}), 'VS Code Insiders');
  assert.equal(toolLabel({agent: 'claude-code'}), 'Claude Code');
  assert.equal(editionFromAppName('Visual Studio Code - Insiders'), 'insiders');
  assert.equal(editionFromAppName('Visual Studio Code'), undefined);
  assert.equal(editionFromAppName('VSCodium'), 'vscodium');
  const store = new SessionStore();
  store.update(v({edition: 'insiders', interaction: true}), 1000);
  store.update(v({sessionId: 'stable', updatedAt: 2000, interaction: true}), 2000);
  assert.equal(activityFor(store.sessions.get('vscode:w')).details, 'VS Code Insiders · Editing');
  assert.equal(activityFor(store.sessions.get('vscode:stable')).details, 'VS Code · Editing');
});
