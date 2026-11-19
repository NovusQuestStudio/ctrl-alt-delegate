# Third-party notices

Ctrl Alt Delegate is derived from **agent-discord-presence** by LitoMore,
https://github.com/LitoMore/agent-discord-presence, commit `eaed633`
(tagged locally as `upstream-eaed633`), used under the MIT License. The original
copyright notice is retained in [LICENSE](LICENSE).

Carried over, with modifications: the Discord IPC client, the session store, the
event protocol, the local service and client, and the hook adapter. These were
later ported from TypeScript to Rust (`apps/tray/src`), keeping the upstream design.
The shared event protocol and reporter client remain in TypeScript (`src/protocol.ts`,
`src/client.ts`) for the VS Code extension.

Removed from the upstream code: AI summary generation and its prompts and presets,
the Go desktop app, the OpenCode, Pi, and DeepSeek Harness adapters, and the
hook setup/uninstall tooling. Upstream agent icons, which carry separate licenses,
are not included.

Other reference projects reviewed (no code copied so far):
- tsanva/cc-discord-presence (MIT)
- trifonovsdev/codex-discord-presence (MIT)
- LeonardSSH/vscord (MIT)

If code from any of these is adopted later, its copyright notice will be added here.

## Giving back

All changes in this project are under the same MIT License. The upstream project and
its maintainers are welcome to use any of them, such as the Windows named-pipe test
fixes, without asking.
