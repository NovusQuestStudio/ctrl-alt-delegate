# Builds the Rust app and copies cad.exe into each agent plugin (plugins/*/bin is gitignored).
# Optional: -Extension also packages the VS Code extension (.vsix).
param([switch]$Extension)
$root = Split-Path $PSScriptRoot -Parent
# cargo reports progress on stderr; judge success by its exit code only.
cargo build --release --manifest-path (Join-Path $root 'apps\tray\Cargo.toml')
if ($LASTEXITCODE -ne 0) { Write-Error 'cargo build failed'; exit 1 }
$ErrorActionPreference = 'Stop'
$cad = Join-Path $root 'apps\tray\target\release\cad.exe'
foreach ($plugin in 'claude-code', 'codex') {
  $bin = Join-Path $root "plugins\$plugin\bin"
  New-Item -ItemType Directory -Force $bin | Out-Null
  Copy-Item $cad (Join-Path $bin 'cad.exe') -Force
}
if ($Extension) {
  Push-Location (Join-Path $root 'apps\vscode')
  # --ignore-scripts: packaging does not need @vscode/vsce-sign's download step.
  try {
    npm ci --ignore-scripts --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    npm run package
    if ($LASTEXITCODE -ne 0) { throw 'vsce package failed' }
  } finally { Pop-Location }
}
Write-Output "Built: apps\tray\target\release\ctrl-alt-delegate.exe and cad.exe (copied into plugins\*\bin)"
