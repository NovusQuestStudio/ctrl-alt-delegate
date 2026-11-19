# Security and privacy

## What Ctrl Alt Delegate shares

Discord only ever receives:
- the tool name and its state, for example "Claude Code · Working"
- the time the session started
- the app's own name and image
- the project folder name, but only if you turn on **Show project name**

It never reads or sends prompts, responses, code, file names, file paths, tool arguments, or
transcripts. There are no AI summaries, no telemetry, and no network connections. The only
connection is to the Discord desktop app on your PC, over Discord's local IPC pipe. No Discord bot
token, client secret or OAuth is used. The Discord Application ID built into the app is public by
design.

## What stays on your PC

| File | Contents |
|---|---|
| `%APPDATA%\CtrlAltDelegate\config.json` | Your settings |
| `%LOCALAPPDATA%\CtrlAltDelegate\sessions.json` | Open sessions, so timers survive restarts. Includes tool, state, times, process IDs and folder names. |
| `%LOCALAPPDATA%\CtrlAltDelegate\ctrl-alt-delegate.log` | Start/stop and error messages only. Capped at 512 KB. |

"Start with Windows" adds one entry under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Turning it off removes the entry.

## Local pipe

Agents and the VS Code extension talk to the app over a named pipe
(`\\.\pipe\ctrl-alt-delegate-<id>`):
- Only your Windows account and SYSTEM can open it.
- It refuses connections from other computers.
- Messages are size-limited and checked against a fixed list of allowed fields; anything else is
  rejected.

Other programs running under your own account can send events to it, just like any program you run
can affect your Discord status. They can't read anything sensitive through it.

## Agent hooks

- **Claude Code and Codex:** the plugins add hooks that run `cad.exe hook claude-code|codex`. These
  hooks read only the session ID, event name and working folder name, and never print anything or
  block the agent.
- **Your existing hooks:** installing the plugins doesn't change them.
- **Codex:** it asks you to approve the plugin's hooks before they run.
- **Removing the plugins:** run `claude plugin uninstall ctrl-alt-delegate@ctrl-alt-delegate` or
  `codex plugin remove ctrl-alt-delegate`.

## Unsigned downloads

Release files aren't code-signed yet, so Windows SmartScreen may warn about an unknown publisher.
Check the download against `SHA256SUMS.txt` and the GitHub build provenance attached to each release.

## Reporting a problem

Please report security issues privately through GitHub's **Report a vulnerability** (Security tab of
the repository) rather than in a public issue.
