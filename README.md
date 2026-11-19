# Ctrl Alt Delegate

One local Discord Rich Presence for your development tools: Claude Code, Codex, and VS Code.

A single background service keeps the only Discord connection. Each tool reports its
session *state* (working, waiting, idle) over a per-user local pipe, and the service
picks one activity to show.

## Privacy

- No telemetry, no network calls other than the local Discord desktop app.
- Prompts, code, tool arguments, transcripts, and file names are never read by the hooks
  or sent to Discord.
- The project folder name is hidden unless you set `showProject: true`.
- There is no AI summary generation.

## Status

Done: Discord connection, Claude Code reporting through a local plugin, and session selection.
The card shows a pinned session first, then one in the focused window, then an active one,
then the one you prompted most recently. Changes that you didn't trigger wait 5 seconds before
switching the card. Sessions clear as soon as the agent process exits, and session timers survive
restarts of the service. A Windows tray app starts the service and controls it. It also reports
the focused window. A VS Code extension reports when you're editing in a focused window. It works in VS Code Stable and Insiders, and the card names the edition ("VS Code Insiders"). Still to come: Codex.

## Try it (Windows, Node 22+)

```bash
npm install
npm run build
node dist/cli.js start
```

In another terminal, start Claude Code with the local plugin (nothing is installed):

```bash
claude --plugin-dir plugins/claude-code
```

Or build and run the tray app, which starts the service for you (Rust toolchain needed):

```bash
cargo build --release --manifest-path apps/tray/Cargo.toml
apps/tray/target/release/ctrl-alt-delegate-tray.exe
```

The tray menu shows Discord and activity status, and has controls to share or stop sharing,
pause, show the project name, pin a session, and restart the service.

VS Code (Insiders shown; use `code` for stable). Build and install the extension from a local file:

```bash
cd apps/vscode && npm install && npm run package
code-insiders --install-extension ctrl-alt-delegate-vscode.vsix
```

Useful commands: `node dist/cli.js status`, `pause`, `resume`, `pin AGENT:ID`, `unpin`, `stop`.

Check the Discord connection on its own: `npm run smoke:discord -- 1572816615429640333 [art-asset-key]`.

## Settings

The settings file lives at `%APPDATA%\CtrlAltDelegate\config.json` and is reloaded every second:

```json
{ "enabled": true, "showProject": false, "largeImage": "", "staleMinutes": 30 }
```

`largeImage` is the key of an image uploaded under Rich Presence → Art Assets in the
Discord Developer Portal.

## License

MIT. Based on agent-discord-presence by LitoMore; see [NOTICE.md](NOTICE.md).
