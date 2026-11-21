# Changelog

This project follows [Semantic Versioning](https://semver.org/).

## 0.1.0 (2026-11-19)

First release, for Windows 11 (x64).

- Discord presence for most of your coding tools, shown as the **Ctrl Alt Delegate** application.
  Published by NovusQuest Studio.
- **Supported tools:**
  - **VS Code** (Stable and Insiders), through the extension. The card names the edition.
  - **Claude Code** (CLI and desktop app) and **Codex** (desktop app), through plugin hooks. The Codex
    CLI is expected to work with the same plugin.
  - **Devin:** the Devin Desktop agent sidebar through hooks in Devin's config (the Devin CLI is
    expected to work too), and the Devin Desktop editor through the extension. Claude Code hooks that
    Devin runs are counted as Devin.
  - **Antigravity:** the agent (IDE and CLI) through a plugin, and the editor through the extension.
    Antigravity has no waiting or session-end signal, so its card shows "Working" and "Ready for a
    prompt" only, and a session clears when the program exits.
  - The extension needs VS Code 1.85 or newer, so it also installs in editors built on older VS Code
    versions, such as Cursor and Windsurf.
- **Connect your tools window:** finds your tools, shows whether each is connected, and connects or
  disconnects them with one click. No terminal needed.
  - Opens on first run, from the tray menu, or when you launch the app a second time.
  - Shows whether Codex hooks are approved, and updates when you switch back from Codex.
  - Detects a moved app folder and relinks the plugins.
  - Uses each tool's own installer and adds only the Ctrl Alt Delegate plugin. For Devin, which has
    no installer, `cad devin connect` adds only our entries to its config.json and backs it up first.
  - Rows that Apply will change say what will happen, in bold, and the button counts the changes
    ("Apply 2 changes").
  - **Show commands** lists exactly what Apply will run, with a **Copy** button. Each command is
    also written to the log as it runs.
- **How the card is chosen:**
  - A pin wins, then the focused window, then an active session, then the one you prompted most recently.
  - Changes you didn't trigger wait 5 seconds before switching, so the card doesn't flicker.
- **Sessions:** cleared as soon as their program exits. Timers survive app restarts.
- **Tray menu:** follows Windows dark or light mode. It shows status, share or stop sharing, pause,
  show project name, pin a session, connect or disconnect tools, start with Windows, open the settings file or
  log, reconnect to Discord, and quit.
- **Privacy:** no telemetry and no network use beyond the local Discord app. Prompts, code and file
  names are never stored or sent. The project name is hidden by default.
- **Discord card:** shows the Ctrl Alt Delegate icon by default, with a small badge for the tool
  you're using (for example CC for Claude Code) on its corner. Turn the badge off with `toolBadges`.
- **Windows look:** modern Windows controls, sharp on scaled displays, and the app icon and version
  information on both .exe files.
- **Command line:** `cad.exe` (`status`, `pause`, `resume`, `pin`, `unpin`, `start`, `stop`,
  `devin connect`, `devin disconnect`, `paths`).
- **Release zip:** unzips to a `CtrlAltDelegate` folder that keeps its name in every version, so a new
  version can be unzipped over the old one. It includes the docs, and a checksum file comes with it.

Originally based on agent-discord-presence by LitoMore (MIT); see NOTICE.md.
