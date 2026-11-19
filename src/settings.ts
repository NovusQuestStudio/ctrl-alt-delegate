import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {homedir} from 'node:os';

/** User settings. Every privacy-relevant option defaults to the most private value. */
export interface Settings {
  /** Master switch. When false, Discord activity is cleared but sessions are still tracked. */
  enabled: boolean;
  /** Show the project folder name in Discord. Off by default. */
  showProject: boolean;
  /** Discord Rich Presence art-asset key (or HTTPS URL) for the large image. Empty uses none. */
  largeImage: string;
  /** Minutes without any event before a hook-only session is cleared. */
  staleMinutes: number;
}
export const DEFAULT_SETTINGS: Readonly<Settings> = Object.freeze({enabled: true, showProject: false, largeImage: '', staleMinutes: 30});

export function settingsPath(): string {
  if (process.env.CAD_CONFIG) return process.env.CAD_CONFIG;
  if (process.platform === 'win32') return join(process.env.APPDATA ?? join(homedir(), 'AppData', 'Roaming'), 'CtrlAltDelegate', 'config.json');
  if (process.platform === 'darwin') return join(homedir(), 'Library', 'Application Support', 'CtrlAltDelegate', 'config.json');
  return join(process.env.XDG_CONFIG_HOME ?? join(homedir(), '.config'), 'ctrl-alt-delegate', 'config.json');
}

export function validateSettings(value: unknown): Settings {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Expected settings object');
  const s = {...DEFAULT_SETTINGS, ...value} as Record<string, unknown>;
  const unknown = Object.keys(s).filter(k => !Object.hasOwn(DEFAULT_SETTINGS, k));
  if (unknown.length) throw new Error(`Unknown setting: ${unknown.join(', ')}`);
  if (typeof s.enabled !== 'boolean' || typeof s.showProject !== 'boolean') throw new Error('enabled and showProject must be true or false');
  if (typeof s.largeImage !== 'string' || s.largeImage.length > 256 ||
      !(s.largeImage === '' || /^[a-zA-Z0-9_-]{1,256}$/.test(s.largeImage) || /^https:\/\/[^\s@]+$/.test(s.largeImage))) {
    throw new Error('largeImage must be a Discord art-asset key or an HTTPS URL');
  }
  if (!Number.isInteger(s.staleMinutes) || (s.staleMinutes as number) < 1 || (s.staleMinutes as number) > 1440) throw new Error('staleMinutes must be 1–1440');
  return s as unknown as Settings;
}

/** A missing file yields defaults; an invalid file throws so callers can keep the last valid settings. */
export function loadSettings(path = settingsPath()): Settings {
  let text: string;
  try { text = readFileSync(path, 'utf8'); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return {...DEFAULT_SETTINGS}; throw error; }
  return validateSettings(JSON.parse(text.replace(/^﻿/, '')));
}
