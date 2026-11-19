import {emit} from '../client.js';
import {projectName, type Agent, type PresenceEvent, type State} from '../protocol.js';

export type HookAgent = Extract<Agent, 'claude-code' | 'codex'>;
const continuations = ['PreToolUse', 'PostToolUse', 'PostToolUseFailure', 'PermissionRequest', 'Notification'];

/**
 * Map a Claude Code / Codex hook payload to a presence event. Reads only the session id,
 * event name, notification type, compaction source, and the folder name of `cwd`.
 * Prompts, tool input, transcripts, and assistant text are never read.
 */
export function hookEvent(agent: HookAgent, payload: Record<string, unknown>, now = Date.now()): PresenceEvent | undefined {
  if (typeof payload.session_id !== 'string' || !payload.session_id) return;
  const name = payload.hook_event_name;
  let state: State | undefined;
  // Compaction is a continuation; it must not reset a busy session to idle.
  if (name === 'SessionStart') state = payload.source === 'compact' ? 'working' : 'idle';
  if (['UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure'].includes(name as string)) state = 'working';
  if (name === 'PermissionRequest') state = 'waiting';
  if (name === 'Notification' && ['permission_prompt', 'idle_prompt', 'elicitation_dialog'].includes(payload.notification_type as string)) state = 'waiting';
  if (name === 'Stop' || name === 'Interrupt') state = 'idle';
  if (name === 'StopFailure') state = 'error';
  if (name === 'SessionEnd') state = 'closed';
  if (!state) return;
  const project = projectName(payload.cwd);
  return {version: 1, agent, sessionId: payload.session_id, state, updatedAt: now,
    ...(continuations.includes(name as string) ? {continuation: true} : {}),
    ...(project ? {project} : {})};
}

export async function runHook(agent: HookAgent): Promise<void> {
  // Stamp before reading stdin; transport arrival order need not match hook order.
  const now = Date.now();
  const timer = setTimeout(() => process.stdin.destroy(), 750); timer.unref();
  try {
    let data = '';
    for await (const chunk of process.stdin) {
      data += chunk.toString();
      if (Buffer.byteLength(data) > 1024 * 1024) return;
    }
    const event = hookEvent(agent, JSON.parse(data), now);
    if (event) await emit(event);
  } catch { /* Hooks are advisory and must never print or block the agent. */ }
  finally { clearTimeout(timer); process.stdin.destroy(); }
}
