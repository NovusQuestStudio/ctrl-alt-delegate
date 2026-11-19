/** Editor activity as the presence service sees it. */
export type Phase = 'working' | 'idle' | 'closed';
export interface ActivityOptions {
  /** Quiet time before the window shows as idle. */
  idleMs: number;
  /** Quiet time before the window's session is cleared. */
  closeMs: number;
  /** While editing, how often to re-mark the session as recently used (for ranking). */
  interactionEveryMs: number;
}
export type Report = (phase: Phase, interaction: boolean) => void;

/**
 * A small clock-driven state machine with no VS Code dependency, so it can be tested directly.
 * `touch` is called for deliberate editor use in a focused window; `tick` runs on a timer.
 */
export class EditorActivity {
  phase: Phase = 'closed';
  private lastActivity = 0;
  private lastInteraction = 0;
  constructor(private readonly options: ActivityOptions, private readonly report: Report) {}
  touch(now: number): void {
    this.lastActivity = now;
    if (this.phase !== 'working') {
      this.phase = 'working'; this.lastInteraction = now; this.report('working', true);
    } else if (now - this.lastInteraction >= this.options.interactionEveryMs) {
      this.lastInteraction = now; this.report('working', true);
    }
  }
  tick(now: number): void {
    if (this.phase === 'closed') return;
    const quiet = now - this.lastActivity;
    if (quiet >= this.options.closeMs) { this.phase = 'closed'; this.report('closed', false); }
    else if (quiet >= this.options.idleMs && this.phase === 'working') { this.phase = 'idle'; this.report('idle', false); }
  }
  stop(): void {
    if (this.phase === 'closed') return;
    this.phase = 'closed'; this.report('closed', false);
  }
}

/** Activity counts only for real documents in the focused window, never for output panels or settings UIs. */
export function countsAsActivity(windowFocused: boolean, scheme: string | undefined): boolean {
  return windowFocused && (scheme === undefined || ['file', 'untitled', 'vscode-remote', 'vscode-notebook-cell'].includes(scheme));
}
