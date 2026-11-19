import {spawn, execFile, type ChildProcessWithoutNullStreams} from 'node:child_process';
import {basename} from 'node:path';

export interface ProcessInfo {pid: number; ppid: number; name: string}
export type ProcessTable = Map<number, ProcessInfo>;
/** Returns a snapshot of the local process table. Only pids, parent pids, and executable names are read. */
export type Snapshot = () => Promise<ProcessTable>;

/** Wrappers that sit between an agent and its hook process; never the agent itself. */
const WRAPPERS = /^(bash|sh|zsh|fish|dash|ksh|cmd|powershell|pwsh|conhost|openconsole|env|sudo|timeout|winpty|winpty-agent|mintty)(\.exe)?$/i;

/** The process chain from `pid` upwards (inclusive), stopping at cycles, unknown parents, or `max` entries. */
export function ancestry(table: ProcessTable, pid: number, max = 16): ProcessInfo[] {
  const chain: ProcessInfo[] = [];
  const seen = new Set<number>();
  for (let current = table.get(pid); current && chain.length < max && !seen.has(current.pid); current = table.get(current.ppid)) {
    seen.add(current.pid); chain.push(current);
    if (current.ppid === current.pid || current.ppid === 0) break;
  }
  return chain;
}

/** The agent is the nearest ancestor of the hook's parent that is not a shell or console wrapper. */
export function findAgent(chain: ProcessInfo[]): ProcessInfo | undefined {
  return chain.find(p => !WRAPPERS.test(p.name));
}

export function parseTable(text: string, separator: RegExp): ProcessTable {
  const table: ProcessTable = new Map();
  for (const line of text.split(/\r?\n/)) {
    const match = line.trim().match(separator);
    if (!match) continue;
    const pid = Number(match[1]), ppid = Number(match[2]);
    if (Number.isSafeInteger(pid) && Number.isSafeInteger(ppid)) table.set(pid, {pid, ppid, name: basename(match[3].trim())});
  }
  return table;
}

// Toolhelp32 snapshot compiled once inside a long-lived Windows PowerShell; ~20 ms per query afterwards.
// Uses only components that ship with Windows. Reads pid, parent pid, and executable name.
const WINDOWS_INIT = `Add-Type -TypeDefinition @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class CadProc {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  struct PE32 { public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID; public uint th32ModuleID; public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase; public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst=260)] public string szExeFile; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateToolhelp32Snapshot(uint f, uint p);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern bool Process32FirstW(IntPtr h, ref PE32 e);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern bool Process32NextW(IntPtr h, ref PE32 e);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  public static string Snapshot() {
    var sb = new StringBuilder(); IntPtr h = CreateToolhelp32Snapshot(2, 0);
    var e = new PE32(); e.dwSize = (uint)Marshal.SizeOf(typeof(PE32));
    if (Process32FirstW(h, ref e)) { do { sb.Append(e.th32ProcessID).Append('\\t').Append(e.th32ParentProcessID).Append('\\t').Append(e.szExeFile).Append('\\n'); } while (Process32NextW(h, ref e)); }
    CloseHandle(h); return sb.ToString();
  }
}
"@

[Console]::Out.Write("<<CAD-END>>")`;
// The blank line after "@ matters: in -Command - mode PowerShell runs a multi-line block only after an empty line.
const END = '<<CAD-END>>';

/** One persistent, hidden PowerShell process answering snapshot queries over stdin/stdout. */
export class WindowsProcessTable {
  private child?: ChildProcessWithoutNullStreams;
  private ready?: Promise<void>;
  private buffer = '';
  private waiters: Array<{resolve: (text: string) => void; reject: (error: Error) => void}> = [];
  private queue: Promise<unknown> = Promise.resolve();
  private cache?: {at: number; table: ProcessTable};
  constructor(private readonly cacheMs = 250) {}
  private start(): Promise<void> {
    const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', '-'],
      {stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true});
    this.child = child; this.buffer = '';
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (data: string) => {
      this.buffer += data;
      if (this.buffer.length > 4 * 1024 * 1024) { child.kill(); return; }
      let index;
      while ((index = this.buffer.indexOf(END)) >= 0) {
        const text = this.buffer.slice(0, index); this.buffer = this.buffer.slice(index + END.length);
        this.waiters.shift()?.resolve(text);
      }
    });
    child.stderr.resume();
    const fail = (error: Error) => {
      if (this.child === child) { this.child = undefined; this.ready = undefined; }
      for (const waiter of this.waiters.splice(0)) waiter.reject(error);
    };
    child.on('error', fail);
    child.on('exit', () => fail(new Error('Process helper exited')));
    return this.send(WINDOWS_INIT).then(() => undefined);
  }
  private send(command: string): Promise<string> {
    return new Promise((resolve, reject) => {
      if (!this.child) return reject(new Error('Process helper is not running'));
      this.waiters.push({resolve, reject});
      this.child.stdin.write(command.replace(/\r?\n/g, '\n') + '\n');
    });
  }
  /** Start the helper ahead of the first query. */
  warm(): Promise<void> { return this.ready ??= this.start(); }
  snapshot: Snapshot = () => {
    if (this.cache && Date.now() - this.cache.at < this.cacheMs) return Promise.resolve(this.cache.table);
    const run = this.queue.then(async () => {
      await this.warm();
      const table = parseTable(await this.send(`[Console]::Out.Write([CadProc]::Snapshot() + "${END}")`), /^(\d+)\t(\d+)\t(.+)$/);
      this.cache = {at: Date.now(), table};
      return table;
    });
    this.queue = run.catch(() => {});
    return run;
  };
  close(): void { this.child?.stdin.end(); this.child?.kill(); this.child = undefined; this.ready = undefined; }
}

export function posixSnapshot(): Promise<ProcessTable> {
  return new Promise((resolve, reject) => {
    execFile('ps', ['-A', '-o', 'pid=,ppid=,comm='], {maxBuffer: 4 * 1024 * 1024}, (error, stdout) => {
      if (error) reject(error); else resolve(parseTable(stdout, /^(\d+)\s+(\d+)\s+(.+)$/));
    });
  });
}

export function systemProcessTable(): {snapshot: Snapshot; warm(): Promise<void>; close(): void} {
  if (process.platform === 'win32') return new WindowsProcessTable();
  return {snapshot: posixSnapshot, warm: async () => {}, close() {}};
}
