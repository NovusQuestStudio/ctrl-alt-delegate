# Ctrl Alt Delegate for VS Code

Reports whether this window is being edited (Editing / Idle) to the local Ctrl Alt Delegate service,
which shows one Discord Rich Presence for VS Code, Claude Code, and Codex.

- Never sends file names, file contents, or paths. The workspace folder name is sent to the local
  service only, and reaches Discord only if "Show project name" is turned on in the tray.
- Does not connect to Discord itself.

Settings: `ctrlAltDelegate.enabled`, `ctrlAltDelegate.idleMinutes`, `ctrlAltDelegate.closeAfterMinutes`.
