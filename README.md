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

Stage 0 (foundation) is done. Claude Code reporting works through a local plugin. Still to come:
the tray app, VS Code extension, Codex, smarter focus-aware selection.

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
