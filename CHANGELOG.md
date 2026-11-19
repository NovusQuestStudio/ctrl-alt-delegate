# Changelog

This project follows [Semantic Versioning](https://semver.org/).

## Unreleased

- **Devin support:**
  - **Agent sidebar and CLI:** the card shows "Devin · Working", "Waiting for input" and so on.
    `cad devin connect` adds the hooks to Devin's config.json, keeps everything else in it, and saves a
    backup first. `cad devin disconnect` removes them.
  - **Devin Desktop editor:** the VS Code extension works there too, and the card says "Devin Desktop".
  - Claude Code hooks that Devin runs are counted as Devin, not Claude Code.
- **Antigravity support:**
  - **Agent (IDE and CLI):** a small plugin installed with `agy plugin install` shows "Antigravity ·
    Working" and "Ready for a prompt". Antigravity has no waiting or session-end signal, so its card
    can't show "Waiting for input", and a session clears when the program exits.
  - **Editor:** the extension works there too, and the card says "Antigravity".
- **VS Code extension:** now needs VS Code 1.85 or newer (was 1.140), so it also installs in editors
  built on older VS Code versions, such as Antigravity, Cursor and Windsurf.
- **Connect your tools window:** finds VS Code, VS Code Insiders, Claude Code, Codex, Devin and Antigravity, shows
  whether each is connected, and connects or disconnects them with one click. No terminal needed.
  - Opens on first run, from the tray menu, or when you launch the app a second time.
  - Shows whether Codex hooks are approved, and updates when you switch back from Codex.
  - Detects a moved app folder and relinks the plugins.
  - Uses each tool's own installer and adds only the Ctrl Alt Delegate plugin.
  - **Show commands** lists exactly what Apply will run, with a **Copy** button. Each command is
    also written to the log as it runs.
- **Windows look:** the app now uses modern Windows controls and is sharp on scaled displays.
- **Release zip:** the folder inside is now always `CtrlAltDelegate`, so you can unzip a new version
  over the old one and keep your plugin links and Start with Windows. `scripts\winget-manifest.ps1`
  writes the manifests for listing a release in winget.
- **Docs:** a new README landing page, plus [Manual setup](docs/manual-setup.md) and
  [Reference](docs/reference.md) pages. The release zip now includes the `docs` folder.

## 0.1.0 (2026-11-19)

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
