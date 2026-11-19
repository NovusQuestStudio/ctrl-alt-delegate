# Ctrl Alt Delegate

**One Developer. Multiple Agents. One Workflow.**

One Discord Rich Presence for your development tools and AI coding agents. A small Windows tray app
keeps the only connection to Discord. Your editor and agents report what state they're in (working,
waiting, idle), and the app shows the one that matters most right now.

## Supported tools

| Tool | Status |
|---|---|
| VS Code (Stable) | ✅ Supported (extension) |
| VS Code Insiders | ✅ Supported (extension) |
| Claude Code | ✅ Supported (plugin hooks) |
| Codex CLI | 🔜 Planned (hook mapping exists, not yet verified) |
| Codex Desktop, Devin Desktop / Windsurf, Devin (cloud) | 🔬 Under investigation |

Windows 11 first. macOS is planned.

## Privacy

- No telemetry. The only connection is to the Discord desktop app on your PC; nothing goes to the internet.
- Prompts, code, tool arguments, transcripts, and file names are never read or sent.
- The project folder name is hidden unless you turn on **Show project name**.
- No AI-generated summaries, no paid APIs, no Discord bot token or client secret.
- Agents talk to the app over a local named pipe that only your Windows account can open.

## How the card is chosen

Priority goes like this:

1. A pinned session always wins.
2. Next, a session running in the window you're looking at.
3. Then a session that's working beats an idle one.
4. Then the session you prompted most recently.

A background agent finishing its work doesn't take the card from the one you're using. Changes you
didn't trigger wait 5 seconds before the card switches, so it doesn't flicker. Sessions clear as soon
as their program exits, and timers survive restarts of the app.

## Build and run (from source)

Requires the Rust toolchain (and Node.js only to build the VS Code extension).

```bash
cargo build --release --manifest-path apps/tray/Cargo.toml
```

Start `apps/tray/target/release/ctrl-alt-delegate.exe`. A tray icon appears. Right-click it to see
status, share or stop sharing, pause, show the project name, pin a session, start with Windows, open
the log or settings, reconnect, or quit.

**VS Code** (use `code-insiders` for Insiders):

```bash
cd apps/vscode && npm install && npm run package
code --install-extension ctrl-alt-delegate-vscode.vsix
```

**Claude Code:** copy `apps/tray/target/release/cad.exe` into `plugins/claude-code/bin/`, then start
Claude Code with `claude --plugin-dir plugins/claude-code`.

## Command line

`cad.exe status | pause | resume | pin AGENT:ID | unpin | start | stop | paths`

## Settings

Settings live in `%APPDATA%\CtrlAltDelegate\config.json`. Open the file from the tray menu; changes
apply within a second.

```json
{ "enabled": true, "showProject": false, "largeImage": "", "staleMinutes": 30 }
```

`largeImage` is the key of an image uploaded under **Rich Presence → Art Assets** in the Discord
Developer Portal.

## Troubleshooting

- **Nothing shows in Discord:** make sure the Discord desktop app is running. Check that the tray says
  "Discord: connected", and turn on **Settings → Activity Privacy → Share your activity** in Discord.
- **Two presences fight over your status:** turn off other Discord presence extensions and plugins
  (for example `discord-vscode`).
- **Logs:** tray → **Open log** (`%LOCALAPPDATA%\CtrlAltDelegate\ctrl-alt-delegate.log`).

## License

MIT. Originally based on agent-discord-presence by LitoMore; see [NOTICE.md](NOTICE.md).

*Because sometimes the smartest move is knowing when to delegate.*
