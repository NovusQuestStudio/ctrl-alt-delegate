// Tests for the shared reporter and protocol code (src/client.ts, src/protocol.ts) that the extension ships.
const test = require('node:test');
const assert = require('node:assert/strict');
const {parseEvent, projectName, toolLabel, editionFromAppName} = require('../dist/src/protocol.js');
const {Reporter} = require('../dist/src/client.js');

const e = (extra = {}) => ({version: 1, agent: 'vscode', sessionId: 'w', state: 'working', updatedAt: 1000, ...extra});

test('protocol keeps only whitelisted fields and rejects bad data', () => {
  const parsed = parseEvent(e({prompt: 'secret', cwd: '/secret', tool: 'Bash'}), 1000);
  for (const key of ['prompt', 'cwd', 'tool']) assert.equal(key in parsed, false);
  for (const bad of [{version: 2}, {agent: 'opencode'}, {state: 'x'}, {pid: -1}, {ppid: 0}, {project: 'a/b'}, {edition: 'nightly'}]) {
    assert.throws(() => parseEvent(e(bad), 1000));
  }
  assert.throws(() => parseEvent({...e({edition: 'insiders'}), agent: 'codex'}, 1000), /Invalid edition/);
});

test('folder names and editor editions', () => {
  assert.equal(projectName('C:\\Users\\me\\Code\\my-app\\'), 'my-app');
  assert.equal(projectName('C:\\'), undefined);
  // C1 control characters are dropped, as the tray's Rust check expects.
  assert.equal(projectName('C:\\Code\\my\u0085app'), 'myapp');
  assert.equal(editionFromAppName('Visual Studio Code - Insiders'), 'insiders');
  assert.equal(editionFromAppName('Visual Studio Code'), undefined);
  assert.equal(editionFromAppName('Windsurf'), 'windsurf');
  assert.equal(editionFromAppName('Antigravity IDE'), 'antigravity');
  assert.equal(editionFromAppName('Devin'), 'devin');
  assert.equal(toolLabel({agent: 'vscode', edition: 'devin'}), 'Devin Desktop');
  assert.equal(toolLabel({agent: 'vscode'}), 'VS Code');
});

test('reporter coalesces updates, marks interactions once, sends ppid and edition, and closes last', async () => {
  const sent = []; let release;
  const reporter = new Reporter('vscode', 's', async event => {
    sent.push(event);
    if (sent.length === 1) await new Promise(resolve => { release = resolve; });
  });
  reporter.ppid = 42; reporter.edition = 'insiders'; reporter.interaction = true;
  reporter.set('working'); reporter.set('waiting'); reporter.set('idle');
  const closed = reporter.close(); release(); await closed;
  assert.deepEqual(sent.map(x => x.state), ['working', 'closed']);
  assert.equal(sent[0].interaction, true); assert.equal(sent[1].interaction, undefined);
  assert.equal(sent[0].ppid, 42); assert.equal(sent[0].edition, 'insiders');
  const failing = new Reporter('vscode', 's', async () => { throw new Error('offline'); });
  failing.set('working'); await failing.close();
});
