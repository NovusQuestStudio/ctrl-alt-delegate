import * as vscode from 'vscode';
import {Reporter} from '../../../src/client';
import {editionFromAppName, projectName} from '../../../src/protocol';
import {EditorActivity, countsAsActivity, type Phase} from './activity';

let session: {activity: EditorActivity; timer: ReturnType<typeof setInterval>; listeners: vscode.Disposable[]} | undefined;
let closing: Promise<void> | undefined;

function settings() {
  const config = vscode.workspace.getConfiguration('ctrlAltDelegate');
  const minutes = (key: string, fallback: number) => Math.max(1, Number(config.get(key, fallback)) || fallback) * 60_000;
  return {enabled: config.get('enabled', true), idleMs: minutes('idleMinutes', 5), closeMs: minutes('closeAfterMinutes', 30)};
}

function start() {
  stop();
  const options = settings();
  if (!options.enabled) return;
  // One session per window. The extension host's parent is VS Code's main process, which lets the
  // service match this window to the tray's foreground-window report.
  const reporter = new Reporter('vscode', vscode.env.sessionId);
  reporter.ppid = process.ppid;
  // "Visual Studio Code - Insiders" → insiders, so the card can tell editions apart.
  reporter.edition = editionFromAppName(vscode.env.appName);
  const report = (phase: Phase, interaction: boolean) => {
    // Folder name only; it reaches Discord only if the user turns on "Show project name".
    reporter.project = projectName(vscode.workspace.workspaceFolders?.[0]?.name);
    if (phase === 'closed') { closing = reporter.close(); return; }
    reporter.interaction = interaction;
    reporter.set(phase);
  };
  const activity = new EditorActivity({idleMs: options.idleMs, closeMs: Math.max(options.closeMs, options.idleMs), interactionEveryMs: 30_000}, report);
  // Only deliberate use of a focused window counts, so agents editing open files in the background do not.
  const touch = (scheme?: string) => { if (countsAsActivity(vscode.window.state.focused, scheme)) activity.touch(Date.now()); };
  session = {
    activity,
    timer: setInterval(() => activity.tick(Date.now()), 15_000),
    listeners: [
      vscode.workspace.onDidChangeTextDocument(e => { if (e.contentChanges.length) touch(e.document.uri.scheme); }),
      vscode.window.onDidChangeTextEditorSelection(e => touch(e.textEditor.document.uri.scheme)),
      vscode.window.onDidChangeActiveTextEditor(e => { if (e) touch(e.document.uri.scheme); }),
    ],
  };
}

function stop() {
  if (!session) return;
  clearInterval(session.timer);
  for (const listener of session.listeners) listener.dispose();
  session.activity.stop();
  session = undefined;
}

export function activate(context: vscode.ExtensionContext) {
  start();
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('ctrlAltDelegate')) start(); }),
    {dispose: stop},
  );
}

export async function deactivate() {
  stop();
  await closing;
}
