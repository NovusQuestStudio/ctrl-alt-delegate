import {APP_NAME} from './constants.js';
import {toolLabel} from './protocol.js';
import type {Session} from './store.js';
import type {Settings} from './settings.js';

export interface Assets {large_image?: string; large_text?: string; small_image?: string; small_text?: string}
export interface Activity {
  type: number; name: string; details?: string; state?: string;
  timestamps?: {start?: number; end?: number}; assets?: Assets;
}
type Display = Pick<Settings, 'showProject' | 'largeImage'>;
const agentPhases = {idle: 'Ready for a prompt', working: 'Working', waiting: 'Waiting for input', error: 'Needs attention', closed: 'Closed'};
const editorPhases = {idle: 'Idle', working: 'Editing', waiting: 'Idle', error: 'Idle', closed: 'Closed'};

/**
 * The only function that decides what leaves this machine for Discord.
 * It sees the tool label, phase, optional project name, and start time; nothing else.
 */
export function activityFor(session: Session | undefined, display: Display = {showProject: false, largeImage: ''}): Activity | null {
  if (!session) return null;
  const label = toolLabel(session);
  const phase = (session.agent === 'vscode' ? editorPhases : agentPhases)[session.state];
  const state = display.showProject && session.project ? session.project
    : session.agent === 'vscode' ? 'In the editor' : 'Delegating to an AI agent';
  return {
    type: 0, name: APP_NAME, details: `${label} · ${phase}`, state,
    timestamps: {start: Math.floor(session.startedAt / 1000)},
    ...(display.largeImage ? {assets: {large_image: display.largeImage, large_text: APP_NAME}} : {}),
  };
}
