# Changelog

This project follows [Semantic Versioning](https://semver.org/).

## 0.1.0 (unreleased)

First release, for Windows 11 (x64).

- Discord presence for most of your coding tools, shown as the **Ctrl Alt Delegate** application.
- **Supported tools:**
  - VS Code (Stable) and VS Code Insiders, through the extension. The card names the edition.
  - Claude Code (CLI and desktop app) and Codex Desktop, through plugin hooks. Codex CLI is expected
    to work with the same plugin.
- **How the card is chosen:**
  - A pin wins, then the focused window, then an active session, then the one you prompted most recently.
  - Changes you didn't trigger wait 5 seconds before switching, so the card doesn't flicker.
- **Sessions:** cleared as soon as their program exits. Timers survive app restarts.
- **Tray menu:** follows Windows dark or light mode. It shows status, share or stop sharing, pause, show project name, pin a session, start with
  Windows, open the settings file or log, reconnect to Discord, and quit.
- **Privacy:** no telemetry and no network use beyond the local Discord app. Prompts, code and file
  names are never read. The project name is hidden by default.
- **Discord card:** shows the Ctrl Alt Delegate icon by default.
- **Program icon:** both .exe files carry the app icon and version information.
- **Command line:** `cad.exe` (`status`, `pause`, `resume`, `pin`, `unpin`, `start`, `stop`, `paths`).

Originally based on agent-discord-presence by LitoMore (MIT); see NOTICE.md.
