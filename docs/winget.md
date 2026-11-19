# Publishing to winget (maintainers)

[winget](https://learn.microsoft.com/windows/package-manager/) is the package manager built into
Windows. Once a version is in Microsoft's public
[winget-pkgs](https://github.com/microsoft/winget-pkgs) repository, anyone can install it with:

```powershell
winget install TEEllis22.CtrlAltDelegate
```

Listing is free. Files that winget installs aren't marked as downloaded from the internet, so Windows
usually doesn't show the "unknown publisher" prompt that a downloaded zip gets.

## How the package is set up

- **Type:** a zip containing a portable app. winget unpacks the whole release folder into
  `%LOCALAPPDATA%\Microsoft\WinGet\Packages\TEEllis22.CtrlAltDelegate_…\CtrlAltDelegate\` and adds
  the `ctrl-alt-delegate` and `cad` commands.
- **Real folder on PATH:** `ArchiveBinariesDependOnPath: true` puts that folder on PATH instead of
  creating links, because the app finds its plugins next to its own `.exe`.
- **Same folder every version:** the zip's inner folder is always `CtrlAltDelegate`, so after
  `winget upgrade`, the plugin links, Devin hooks and Start with Windows still point at the right
  files.

## Each release

The repository must be **public**: winget downloads the zip from the GitHub release without signing
in.

1. **Publish the GitHub release** as usual: push the `v<version>` tag, check the draft, click
   **Publish release**.
2. **Get the published zip.** The manifest's checksum must match the file people download, so use the
   release's zip, not a local build:

   ```powershell
   gh release download v<version> -p "*.zip" -D release --clobber
   ```

3. **Write and check the manifests:**

   ```powershell
   .\scripts\winget-manifest.ps1
   winget validate --manifest release\winget\<version>
   ```

4. **Optional local test.** This needs a one-time setting from an administrator PowerShell:
   `winget settings --enable LocalManifestFiles`. Then:

   ```powershell
   winget install --manifest release\winget\<version>
   ```

5. **Submit.** The simplest way is Microsoft's `wingetcreate` tool
   (`winget install Microsoft.WingetCreate`), signed in with your GitHub account:

   ```powershell
   wingetcreate submit release\winget\<version>
   ```

   This opens a pull request to `microsoft/winget-pkgs` with the files under
   `manifests/t/TEEllis22/CtrlAltDelegate/<version>/`. You can also fork winget-pkgs and open the pull
   request yourself.

6. **Review.** A bot asks you to accept Microsoft's contributor agreement on your first pull request.
   Automated checks then install the package and scan it; a new unsigned app can be held for manual
   review. Approval usually takes a few days. After that, `winget install TEEllis22.CtrlAltDelegate`
   works for everyone, and `winget upgrade` picks up later versions once you submit them the same way.
