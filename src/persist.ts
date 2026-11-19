import {readFileSync} from 'node:fs';
import {mkdir, writeFile, rename} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {homedir} from 'node:os';

/** Local session state, so a service restart keeps session timers. Never leaves this machine. */
export function statePath(): string {
  if (process.env.CAD_STATE) return process.env.CAD_STATE;
  if (process.platform === 'win32') return join(process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'), 'CtrlAltDelegate', 'sessions.json');
  if (process.platform === 'darwin') return join(homedir(), 'Library', 'Application Support', 'CtrlAltDelegate', 'sessions.json');
  return join(process.env.XDG_STATE_HOME ?? join(homedir(), '.local', 'state'), 'ctrl-alt-delegate', 'sessions.json');
}

export function readState(path: string): unknown {
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return undefined; }
}

/** Serialized, atomic writes (temp file + rename). Later writes replace queued ones. */
export class StateWriter {
  private pending?: string;
  private writing?: Promise<void>;
  constructor(private readonly path: string) {}
  save(data: unknown): Promise<void> {
    this.pending = JSON.stringify(data);
    return this.writing ??= this.flush().finally(() => { this.writing = undefined; });
  }
  private async flush(): Promise<void> {
    while (this.pending !== undefined) {
      const text = this.pending; this.pending = undefined;
      try {
        await mkdir(dirname(this.path), {recursive: true, mode: 0o700});
        const temp = `${this.path}.${process.pid}.tmp`;
        await writeFile(temp, text, {mode: 0o600});
        await rename(temp, this.path);
      } catch { /* Persistence is best effort; presence keeps working without it. */ }
    }
  }
  async idle(): Promise<void> { while (this.writing) await this.writing; }
}
