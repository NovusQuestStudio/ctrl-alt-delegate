import {mkdtemp, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';

/** A unique local IPC endpoint: a named pipe on Windows, a socket in a temp dir elsewhere. */
export async function ipcEndpoint(name) {
  const dir = await mkdtemp(join(tmpdir(), `cad-${name}-`));
  const path = process.platform === 'win32' ? `\\\\.\\pipe\\cad-test-${name}-${randomUUID()}` : join(dir, `${name}.sock`);
  return {dir, path, cleanup: () => rm(dir, {recursive: true, force: true})};
}

export async function until(fn, ms = 4000) {
  const start = Date.now();
  while (!(await fn())) { if (Date.now() - start > ms) throw new Error('Condition timed out'); await delay(10); }
}

export const recorder = () => {
  const activity = [];
  return {activity, transport: {status: 'test', start() {}, stop() { activity.push(null); }, setActivity(a) { activity.push(a); }}};
};
