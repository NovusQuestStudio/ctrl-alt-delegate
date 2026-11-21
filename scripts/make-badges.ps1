# Builds the round tool badges shown as the small image on the Discord card.
# These are original NovusQuest designs (initials on a dark disc with a coloured ring), not the
# tools' own logos: their brand guidelines don't allow third-party apps to show those.
# Upload each PNG under Rich Presence -> Art Assets with its file name (without .png) as the key.
# Uses only System.Drawing, which ships with Windows.
param(
  [string]$OutDir = (Join-Path $PSScriptRoot '..\assets\discord\badges'),
  [int]$Size = 512,
  # Optional: also write a preview sheet at Discord's badge size.
  [string]$Preview = ''
)
Add-Type -AssemblyName System.Drawing

# Key, initials, ring colour (start, end). Keys match the tool names the app sends.
$badges = @(
  @{ Key = 'tool_claude_code';      Text = 'CC'; A = '#E2895E'; B = '#C9643F' },
  @{ Key = 'tool_codex';            Text = 'CX'; A = '#D7DDE4'; B = '#8E99A6' },
  @{ Key = 'tool_devin';            Text = 'DV'; A = '#5FD3B5'; B = '#3A7FD6' },
  @{ Key = 'tool_antigravity';      Text = 'AG'; A = '#7FA8FF'; B = '#B27BFF' },
  @{ Key = 'tool_vscode';           Text = 'VS'; A = '#4FA3F7'; B = '#1F6FD1' },
  @{ Key = 'tool_vscode_insiders';  Text = 'VI'; A = '#4FD08F'; B = '#1F9E63' },
  @{ Key = 'tool_vscodium';         Text = 'VC'; A = '#6EC4F0'; B = '#2F8FC4' },
  @{ Key = 'tool_cursor';           Text = 'CU'; A = '#E6E6E6'; B = '#7A7A7A' },
  @{ Key = 'tool_windsurf';         Text = 'WS'; A = '#5ED6D6'; B = '#2A9C9C' }
)

function Hex([string]$h) { [System.Drawing.ColorTranslator]::FromHtml($h) }

function Badge($b, [int]$n) {
  $bmp = New-Object System.Drawing.Bitmap $n, $n, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.TextRenderingHint = 'AntiAliasGridFit'; $g.PixelOffsetMode = 'HighQuality'
  $g.Clear([System.Drawing.Color]::Transparent)
  $ring = [single]($n * 0.085)
  # Ring: a full disc in the gradient, then the dark face on top.
  $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.PointF 0, 0), (New-Object System.Drawing.PointF $n, $n), (Hex $b.A), (Hex $b.B)
  $g.FillEllipse($grad, 0, 0, $n - 1, $n - 1)
  $face = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 11, 14, 19))
  $g.FillEllipse($face, $ring, $ring, $n - 1 - 2 * $ring, $n - 1 - 2 * $ring)
  # Initials: heavy, white, centred optically.
  $font = New-Object System.Drawing.Font 'Segoe UI', ([single]($n * 0.34)), ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $fmt = New-Object System.Drawing.StringFormat; $fmt.Alignment = 'Center'; $fmt.LineAlignment = 'Center'
  $path.AddString($b.Text, $font.FontFamily, [int]$font.Style, $font.Size, (New-Object System.Drawing.RectangleF 0, 0, $n, $n), $fmt)
  $box = $path.GetBounds()
  $m = New-Object System.Drawing.Drawing2D.Matrix
  $m.Translate(($n / 2) - ($box.X + $box.Width / 2), ($n / 2) - ($box.Y + $box.Height / 2))
  $path.Transform($m)
  $g.FillPath([System.Drawing.Brushes]::White, $path)
  $g.Dispose(); return $bmp
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
$made = @()
foreach ($b in $badges) {
  $bmp = Badge $b $Size
  $file = Join-Path (Resolve-Path $OutDir) ($b.Key + '.png')
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  $made += $file
}
Write-Output "Wrote $($made.Count) badges to $(Resolve-Path $OutDir)"

if ($Preview) {
  # Each badge at 22 px (Discord's corner badge) on the card colour, then at 96 px for detail.
  $cols = $badges.Count; $sheet = New-Object System.Drawing.Bitmap ($cols * 110 + 10), 200
  $g = [System.Drawing.Graphics]::FromImage($sheet)
  $g.InterpolationMode = 'HighQualityBicubic'; $g.SmoothingMode = 'AntiAlias'
  $g.Clear([System.Drawing.Color]::FromArgb(255, 35, 36, 40))
  $font = New-Object System.Drawing.Font 'Segoe UI', 8
  $x = 10
  foreach ($b in $badges) {
    $img = [System.Drawing.Image]::FromFile((Join-Path (Resolve-Path $OutDir) ($b.Key + '.png')))
    $g.DrawImage($img, $x + 37, 10, 22, 22)
    $g.DrawImage($img, $x, 45, 96, 96)
    $g.DrawString(($b.Key -replace '^tool_', ''), $font, [System.Drawing.Brushes]::Gainsboro, $x, 150)
    $img.Dispose(); $x += 110
  }
  $g.Dispose(); $sheet.Save($Preview, [System.Drawing.Imaging.ImageFormat]::Png); $sheet.Dispose()
  Write-Output "Preview $Preview"
}
