# Manual setup

The **Connect your tools** window does all of this for you, and **Show commands** in that window
lists the exact commands with the real paths on your PC. Use this page if you'd rather run them
yourself.

Use the unzipped release folder (or the repository root, after `scripts\build.ps1 -Extension`)
wherever a command says `<path to this folder>`. None of these commands change your existing hooks
or settings; each one adds or removes only Ctrl Alt Delegate's own plugin.

## Editors

The same extension works in VS Code and the editors built on it.

| Editor | Command |
|---|---|
| VS Code | `code --install-extension "<path to this folder>\ctrl-alt-delegate-vscode.vsix"` |
| VS Code Insiders | `code-insiders --install-extension "<path to this folder>\ctrl-alt-delegate-vscode.vsix"` |
| Devin Desktop | `devin-desktop --install-extension "<path to this folder>\ctrl-alt-delegate-vscode.vsix"` |
| Antigravity | `antigravity-ide --install-extension "<path to this folder>\ctrl-alt-delegate-vscode.vsix"` |

Reload the editor's windows afterwards. To remove it, use `--uninstall-extension
ctrl-alt-delegate.ctrl-alt-delegate-vscode` with the same command.

## Claude Code

Works for the Claude Code CLI and the Claude desktop app.

```bash
claude plugin marketplace add "<path to this folder>"
claude plugin install ctrl-alt-delegate@ctrl-alt-delegate
```

Restart open Claude Code sessions. To remove it: `claude plugin uninstall ctrl-alt-delegate@ctrl-alt-delegate`.

## Codex

Codex Desktop includes a `codex.exe` command-line tool. In PowerShell, with Codex open:

```powershell
$codex = (Get-Process codex | Select-Object -First 1).Path
& $codex plugin marketplace add "<path to this folder>"
& $codex plugin add ctrl-alt-delegate@ctrl-alt-delegate
```

Restart Codex, then approve the plugin's hooks under **Settings → Hooks**. Codex doesn't run plugin
hooks until you trust them. To remove it: `codex plugin remove ctrl-alt-delegate@ctrl-alt-delegate`.

## Devin

For Devin Desktop's agent sidebar and the Devin CLI:

```powershell
& "<path to this folder>\cad.exe" devin connect
```

This adds Ctrl Alt Delegate's hooks to the `"hooks"` section of Devin's settings file,
`%APPDATA%\devin\config.json`. Devin has no command for this, so `cad.exe` makes the change itself:

- Everything else in the file, including other hooks, is kept.
- The first change saves the original next to it as `config.json.cad-backup`.
- If the file contains comments, it's left alone and you'll get an error instead.
- Its sections may come back in a different order, which doesn't change their meaning.

Start a new chat in Devin afterwards. Devin doesn't run hooks in a folder opened in Restricted Mode.
To remove the hooks: `cad.exe devin disconnect`.

## Antigravity

For Antigravity's agent (the IDE and the `agy` CLI), with Antigravity closed:

```powershell
agy plugin install "<path to this folder>\plugins\antigravity"
```

Antigravity copies the plugin into `~\.gemini\config\plugins`. It locks that folder while it runs,
so close it before installing or removing. To remove it: `agy plugin uninstall ctrl-alt-delegate`.

Antigravity has no "waiting for permission" or "session ended" hook, so its card shows **Working** and
**Ready for a prompt** only, and a session clears when the program exits.
