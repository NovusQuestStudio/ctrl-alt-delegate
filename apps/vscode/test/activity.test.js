const test = require('node:test');
const assert = require('node:assert/strict');
const {EditorActivity, countsAsActivity} = require('../dist/apps/vscode/src/activity.js');

const minute = 60_000;
function machine() {
  const reports = [];
  const activity = new EditorActivity({idleMs: 5 * minute, closeMs: 30 * minute, interactionEveryMs: 30_000}, (phase, interaction) => reports.push([phase, interaction]));
  return {activity, reports};
}

test('first use starts an interactive working session; typing re-marks it at most every 30 s', () => {
  const {activity, reports} = machine();
  activity.tick(0);
  assert.deepEqual(reports, [], 'nothing is reported before the user does anything');
  activity.touch(1000); activity.touch(2000); activity.touch(20_000);
  assert.deepEqual(reports, [['working', true]]);
  activity.touch(31_000);
  assert.deepEqual(reports, [['working', true], ['working', true]]);
});

test('quiet windows go idle, then close; new use reopens', () => {
  const {activity, reports} = machine();
  activity.touch(0);
  activity.tick(4 * minute); assert.equal(activity.phase, 'working');
  activity.tick(5 * minute); assert.equal(activity.phase, 'idle');
  activity.tick(10 * minute);
  activity.tick(30 * minute); assert.equal(activity.phase, 'closed');
  activity.tick(40 * minute);
  assert.deepEqual(reports.map(r => r[0]), ['working', 'idle', 'closed']);
  activity.touch(41 * minute);
  assert.deepEqual(reports.at(-1), ['working', true]);
});

test('stop closes an open session once', () => {
  const {activity, reports} = machine();
  activity.stop(); assert.deepEqual(reports, []);
  activity.touch(0); activity.stop(); activity.stop();
  assert.deepEqual(reports.map(r => r[0]), ['working', 'closed']);
});

test('only focused, real documents count as activity', () => {
  assert.equal(countsAsActivity(true, 'file'), true);
  assert.equal(countsAsActivity(true, 'untitled'), true);
  assert.equal(countsAsActivity(false, 'file'), false, 'background edits (for example by an agent) do not count');
  assert.equal(countsAsActivity(true, 'output'), false);
  assert.equal(countsAsActivity(true, 'vscode-settings'), false);
});
