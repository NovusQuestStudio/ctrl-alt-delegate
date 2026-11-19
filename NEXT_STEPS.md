# Next steps (as of 2026-11-19)

## Where things stand
- Committed: Stages 0–4 (last commit `d9f9136`).
- **Not committed yet: Stage 5 Phase A, the Rust core.** Everything below is in the working tree.
  - `apps/tray` is now one Rust app: the tray, the service and the Discord connection (`ctrl-alt-delegate.exe`), plus `cad.exe` for hooks and the command line.
  - 32 Rust service tests and 3 tray tests pass. Memory use is about 13 MB, down from about 160 MB.
  - Verified live: VS Code Stable and Insiders, Claude Code through the bundled `cad.exe`, Start with Windows, Open log, Open settings file.
  - Fixed: a late heartbeat now brings back a VS Code session whose lease expired, and a restart keeps live windows' sessions.
  - Both agent plugins bundle `cad.exe` in `plugins/*/bin` (gitignored). Run `scripts/build.ps1` after each build to refresh it.
- Note: launch the tray yourself (double-click), not from a Claude tool session, which runs sandboxed.

## To do, in order
1. ✅ Done: Phase A committed (`67ad203`).
2. ✅ Done: retired the Node service and rewrote the README (`58c9825`).
3. ✅ Done: Phase B. The Claude plugin and the Codex plugin install from local marketplaces. Verified live in Claude Code and Codex Desktop.
4. **Phase C:** the tray items are done; still to do:
   - ✅ Done: app icon and version info embedded in both .exe files (build.rs + Windows SDK rc.exe).
   - Upload the icon under Discord **Rich Presence → Art Assets** and set `largeImage` in the settings file.
5. Stage 6 (v0.1.0 release): CI, a portable zip with SHA-256 checksums, CHANGELOG, SECURITY/privacy note, then a GitHub release. Nothing is published without approval.

## Deferred
Devin Desktop/Windsurf (the extension plus Cascade hooks), Devin cloud (opt-in API), macOS.
