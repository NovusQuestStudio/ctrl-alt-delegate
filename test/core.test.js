import test from 'node:test';
import assert from 'node:assert/strict';
import {SessionStore} from '../dist/store.js';
import {parseEvent, projectName} from '../dist/protocol.js';
import {activityFor} from '../dist/presentation.js';
import {hookEvent} from '../dist/adapters/hooks.js';
import {Reporter} from '../dist/client.js';
import {validateSettings, DEFAULT_SETTINGS} from '../dist/settings.js';
const e = (sessionId, state, updatedAt = 1000, extra = {}) => ({version: 1, agent: 'codex', sessionId, state, updatedAt, ...extra});

test('working outranks idle; latest active wins; heartbeat does not steal selection; pin overrides', () => {
  const store = new SessionStore();
  store.update(e('a', 'working'), 1000);
  store.update(e('b', 'idle', 2000), 2000);
  assert.equal(store.select().sessionId, 'a');
  store.update(e('b', 'working', 3000), 3000);
  store.update(e('a', 'working', 1000, {heartbeat: true}), 4000);
  assert.equal(store.select().sessionId, 'b');
  store.pin('codex:a'); assert.equal(store.select().sessionId, 'a');
  store.update(e('a', 'closed', 5000), 5000);
  assert.equal(store.pinned, null); assert.equal(store.select().sessionId, 'b');
});

test('simultaneous agents: each keeps its own session and timer', () => {
  const store = new SessionStore();
  store.update({...e('c1', 'working', 1000), agent: 'claude-code'}, 1000);
  store.update(e('x1', 'working', 5000), 5000);
  assert.equal(store.select().agent, 'codex');
  store.update({...e('c1', 'working', 9000), agent: 'claude-code'}, 9000);
  assert.equal(store.select().agent, 'claude-code');
  assert.equal(store.select().startedAt, 1000);
  assert.equal(store.sessions.size, 2);
});

test('late events cannot regress or resurrect a closed session', () => {
  const store = new SessionStore();
  store.update(e('a', 'idle', 2000), 2000);
  store.update(e('a', 'working', 1000), 2001);
  assert.equal(store.select().state, 'idle');
  store.update(e('a', 'closed', 3000), 3000);
  store.update(e('a', 'working', 2000, {heartbeat: true}), 3001);
  assert.equal(store.select(), undefined);
  store.update(e('a', 'working', 4000), 4000);
  assert.equal(store.select().startedAt, 4000);
});

test('stale sessions: leases and hook expiry clear presence; process death clears immediately', () => {
  const store = new SessionStore(300_000, pid => pid !== 999);
  store.update(e('hook', 'working'), 1000);
  store.update(e('extension', 'working', 1000, {pid: 123}), 1000);
  store.update(e('dead', 'working', 1000, {pid: 999}), 1000);
  store.sweep(2000); assert.equal(store.sessions.has('codex:dead'), false);
  store.sweep(92_000); assert.equal(store.sessions.has('codex:extension'), false);
  assert.equal(store.select().sessionId, 'hook');
  store.sweep(302_000); assert.equal(store.select(), undefined);
});

test('protocol whitelists fields and rejects bad data', () => {
  const parsed = parseEvent(e('a', 'idle', 1000, {prompt: 'secret', cwd: '/secret', response: 'x', tool: 'Bash'}), 1000);
  for (const key of ['prompt', 'cwd', 'response', 'tool']) assert.equal(key in parsed, false);
  for (const extra of [{version: 2}, {agent: 'x'}, {agent: 'opencode'}, {state: 'x'}, {updatedAt: Infinity}, {pid: -1}, {sessionId: ''}, {project: 'a/b'}, {project: 'C:\\x'}]) {
    assert.throws(() => parseEvent({...e('a', 'idle'), ...extra}, 1000));
  }
});

test('projectName keeps only the folder name', () => {
  assert.equal(projectName('C:\\Users\\me\\Code\\my-app\\'), 'my-app');
  assert.equal(projectName('/home/me/secret-client/project'), 'project');
  assert.equal(projectName('C:\\'), undefined);
  assert.equal(projectName(42), undefined);
});

test('a fresh daemon recovers long-running reporter state on heartbeat (application restart)', () => {
  const store = new SessionStore(300_000, () => true);
  store.update(e('long-run', 'working', 1000, {pid: 123, heartbeat: true}), 1_000_000);
  assert.equal(store.select().state, 'working');
  assert.equal(store.select().startedAt, 1000);
  store.sweep(1_010_000);
  assert.equal(store.select().sessionId, 'long-run');
});

test('activity is branded, hides the project by default, and never contains session identifiers', () => {
  const store = new SessionStore();
  store.update({...e('secret-id', 'working'), agent: 'claude-code', project: 'client-x'}, 1000);
  store.update({...e('secret-id', 'idle', 3000), agent: 'claude-code'}, 3000);
  const hidden = activityFor(store.select());
  assert.deepEqual(hidden, {type: 0, name: 'Ctrl Alt Delegate', details: 'Claude Code · Ready for a prompt', state: 'Delegating to an AI agent', timestamps: {start: 1}});
  assert.equal(JSON.stringify(hidden).includes('client-x'), false);
  assert.equal(JSON.stringify(hidden).includes('secret-id'), false);
  const shown = activityFor(store.select(), {showProject: true, largeImage: 'icon'});
  assert.equal(shown.state, 'client-x');
  assert.deepEqual(shown.assets, {large_image: 'icon', large_text: 'Ctrl Alt Delegate'});
  assert.equal(activityFor(), null);
});

test('hook mapping: states, compaction, closure, and only the folder name of cwd', () => {
  const ev = (name, extra = {}) => hookEvent('claude-code', {session_id: 's', hook_event_name: name, cwd: 'C:\\work\\my-app', prompt: 'secret', ...extra}, 1000);
  assert.equal(ev('PostToolUse').state, 'working');
  assert.equal(ev('SessionStart', {source: 'compact'}).state, 'working');
  assert.equal(ev('PermissionRequest').state, 'waiting');
  assert.equal(ev('Stop').state, 'idle'); assert.equal(ev('SessionEnd').state, 'closed');
  assert.equal(ev('StopFailure').state, 'error');
  assert.equal(ev('SubagentStop'), undefined);
  assert.equal(ev('Notification', {notification_type: 'auth_success'}), undefined);
  assert.equal(ev('UserPromptSubmit').project, 'my-app');
  assert.equal(JSON.stringify(ev('UserPromptSubmit')).includes('secret'), false);
});

test('settings default to private values and reject unknown or unsafe values', () => {
  assert.deepEqual(validateSettings({}), DEFAULT_SETTINGS);
  assert.equal(DEFAULT_SETTINGS.showProject, false);
  for (const bad of [{summaries: true}, {largeImage: 'http://x'}, {largeImage: 'a b'}, {staleMinutes: 0}, {enabled: 'yes'}]) assert.throws(() => validateSettings(bad));
  assert.equal(validateSettings({largeImage: 'ctrl_alt_delegate'}).largeImage, 'ctrl_alt_delegate');
});

test('reporter coalesces updates, closes last, and tolerates an unavailable service', async () => {
  const sent = []; let release;
  const reporter = new Reporter('vscode', 's', async event => {
    sent.push(event);
    if (sent.length === 1) await new Promise(resolve => { release = resolve; });
  });
  reporter.set('working'); reporter.set('waiting'); reporter.set('idle');
  const closed = reporter.close(); release(); await closed;
  assert.deepEqual(sent.map(e => e.state), ['working', 'closed']);
  assert.ok(sent[1].updatedAt > sent[0].updatedAt);
  const failing = new Reporter('vscode', 's', async () => { throw new Error('offline'); });
  failing.set('working'); await failing.close();
});
