# Writes the winget manifests for a release into .\release\winget\<version>\, ready for
# `winget validate` and a pull request to github.com/microsoft/winget-pkgs (see docs/winget.md).
# Run after scripts\package.ps1, so the zip's SHA-256 is known. The zip must be published at
# -Url before winget can install it.
param(
  [string]$Url,
  [string]$Identifier = 'NovusQuestStudio.CtrlAltDelegate',
  [string]$Repository = 'https://github.com/NovusQuestStudio/ctrl-alt-delegate'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path (Join-Path $root 'apps\tray\Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "CtrlAltDelegate-$version-windows-x64"
$zip = Join-Path $root "release\$name.zip"
if (-not (Test-Path $zip)) { throw "Run scripts\package.ps1 first: $zip is missing" }
if (-not $Url) { $Url = "$Repository/releases/download/v$version/$name.zip" }
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash
$out = Join-Path $root "release\winget\$version"
New-Item -ItemType Directory -Force $out | Out-Null
$schema = '1.28.0'

$files = @{
  "$Identifier.yaml" = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.$schema.schema.json

PackageIdentifier: $Identifier
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@
  "$Identifier.installer.yaml" = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json

PackageIdentifier: $Identifier
PackageVersion: $version
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: CtrlAltDelegate\ctrl-alt-delegate.exe
  PortableCommandAlias: ctrl-alt-delegate
- RelativeFilePath: CtrlAltDelegate\cad.exe
  PortableCommandAlias: cad
# The app finds its plugins next to its own .exe, so put the real folder on PATH instead of links.
ArchiveBinariesDependOnPath: true
Commands:
- ctrl-alt-delegate
- cad
Installers:
- Architecture: x64
  InstallerUrl: $Url
  InstallerSha256: $hash
ManifestType: installer
ManifestVersion: $schema
"@
  "$Identifier.locale.en-US.yaml" = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json

PackageIdentifier: $Identifier
PackageVersion: $version
PackageLocale: en-US
Publisher: NovusQuest Studio
PublisherUrl: https://github.com/NovusQuestStudio
PublisherSupportUrl: $Repository/issues
PackageName: Ctrl Alt Delegate
PackageUrl: $Repository
License: MIT
LicenseUrl: $Repository/blob/main/LICENSE
ShortDescription: Discord presence for most of your coding tools.
Description: |-
  A small Windows tray app that shows what your editors and AI agents are doing on one Discord card:
  VS Code, Claude Code, Codex, Devin and Antigravity. It shows the state ("Working", "Waiting for
  input"), never your prompts, code or file names, and has no telemetry. Run ctrl-alt-delegate after
  installing; its setup window connects your tools.
Moniker: ctrl-alt-delegate
Tags:
- ai-agents
- claude-code
- codex
- discord
- rich-presence
- vscode
ReleaseNotesUrl: $Repository/releases/tag/v$version
ManifestType: defaultLocale
ManifestVersion: $schema
"@
}
foreach ($file in $files.Keys) {
  # UTF-8 without a byte order mark, LF line endings, as winget-pkgs expects.
  [System.IO.File]::WriteAllText((Join-Path $out $file), ($files[$file] -replace "`r`n", "`n") + "`n", (New-Object System.Text.UTF8Encoding $false))
}
Write-Output "Wrote $out"
