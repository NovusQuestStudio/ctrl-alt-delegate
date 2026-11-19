# Next steps (as of 2026-11-19)

## Where things stand
- Committed: Stages 0–4 (last commit `d9f9136`).
- **Not committed yet: Stage 5 Phase A, the Rust core.** Everything below is in the working tree.
  - `apps/tray` is now one Rust app: the tray, the service and the Discord connection (`ctrl-alt-delegate.exe`), plus `cad.exe` for hooks and the command line.
  - 32 Rust service tests and 3 tray tests pass. Memory use is about 13 MB, down from about 160 MB.
  - Verified live: VS Code Stable and Insiders, Claude Code through the bundled `cad.exe`, Start with Windows, Open log, Open settings file.
  - Fixed: a late heartbeat now brings back a VS Code session whose lease expired, and a restart keeps live windows' sessions.
  - The Claude plugin hooks call `${CLAUDE_PLUGIN_ROOT}/bin/cad.exe`. The `bin/` copy is gitignored; copy `apps/tray/target/release/cad.exe` there after each build.
- Note: launch the tray yourself (double-click), not from a Claude tool session, which runs sandboxed.

## To do, in order
1. Review the diff, then commit Phase A.
2. Retire the Node service. Remove `src/daemon.ts`, `cli.ts`, `store.ts`, `selector.ts`, `discord.ts`, `process-table.ts`, `persist.ts`, `presentation.ts`, `settings.ts`, `background.ts` and `adapters/`, plus their tests and the root build scripts.
   - Keep `src/client.ts` and `src/protocol.ts`, which the VS Code extension uses. Add `windsurf`/`devin` to `editionFromAppName` in `src/protocol.ts`, since the Rust side already has them.
   - Delete the stale `apps/tray/target/release/ctrl-alt-delegate-tray.exe`.
3. **Phase B:** an installable Claude plugin (local marketplace `.claude-plugin/marketplace.json`), and a Codex CLI plugin. Codex CLI isn't installed here, and whether hooks fire on Windows needs a live check.
4. **Phase C:** the tray items are done; still to do:
   - **App icon on the .exe files**, so Task Manager, Startup apps, Explorer and the taskbar show the Ctrl Alt Delegate icon instead of a generic one. Build a multi-size `.ico` (16–256 px) from the approved icon. Embed it in `ctrl-alt-delegate.exe` and `cad.exe` with a `build.rs` resource step. That needs either Windows SDK `rc.exe` or a small build crate such as `embed-resource`, and the crate needs approval. Also add version info: product name and description.
   - Upload the icon under Discord **Rich Presence → Art Assets** and set `largeImage` in the settings file.
   - Update the README for the Rust app and `cad.exe`.
5. Stage 6 (v0.1.0 release): CI, a portable zip with SHA-256 checksums, CHANGELOG, SECURITY/privacy note, then a GitHub release. Nothing is published without approval.

## Deferred
Codex Desktop (check whether hooks fire), Devin Desktop/Windsurf (the extension plus Cascade hooks), Devin cloud (opt-in API), macOS.
