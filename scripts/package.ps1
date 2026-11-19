# Builds everything and creates the portable release zip plus SHA256SUMS.txt in .\release\.
# Used locally and by .github/workflows/release.yml.
$root = Split-Path $PSScriptRoot -Parent
& (Join-Path $PSScriptRoot 'build.ps1') -Extension
if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) { exit 1 }
$ErrorActionPreference = 'Stop'

$version = (Select-String -Path (Join-Path $root 'apps\tray\Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "CtrlAltDelegate-$version-windows-x64"
$out = Join-Path $root 'release'
$stage = Join-Path $out $name
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $stage | Out-Null

$release = Join-Path $root 'apps\tray\target\release'
Copy-Item (Join-Path $release 'ctrl-alt-delegate.exe'), (Join-Path $release 'cad.exe') $stage
Copy-Item (Join-Path $root 'apps\vscode\ctrl-alt-delegate-vscode.vsix') $stage
foreach ($doc in 'README.md', 'LICENSE', 'NOTICE.md', 'CHANGELOG.md', 'SECURITY.md') { Copy-Item (Join-Path $root $doc) $stage }
# The unzipped folder is itself the Claude Code and Codex plugin marketplace.
foreach ($dir in '.claude-plugin', '.agents', 'plugins') { Copy-Item -Recurse (Join-Path $root $dir) (Join-Path $stage $dir) }
foreach ($plugin in 'claude-code', 'codex') {
  if (-not (Test-Path (Join-Path $stage "plugins\$plugin\bin\cad.exe"))) { throw "plugins\$plugin\bin\cad.exe is missing" }
}

$zip = Join-Path $out "$name.zip"
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
Set-Content -Path (Join-Path $out 'SHA256SUMS.txt') -Value "$hash  $name.zip" -Encoding ascii
Write-Output "Created $zip"
Write-Output "SHA-256 $hash"
