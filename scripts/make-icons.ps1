# Builds every app icon from the source artwork (Ctrl Alt Delegate_Icon.png, a square PNG
# with transparent corners):
#   apps/tray/assets/app.ico       embedded in ctrl-alt-delegate.exe and cad.exe
#   apps/tray/assets/icon-32.rgba  raw 32x32 RGBA for the system tray
#   docs/images/logo-256.png       README logo
# The artwork is a bold "<_" on a plain tile, so it is scaled down for every size.
# Uses only System.Drawing, which ships with Windows.
param(
  [string]$Source = (Join-Path $PSScriptRoot '..\Ctrl Alt Delegate_Icon.png'),
  [string]$Root = (Join-Path $PSScriptRoot '..'),
  # Optional: also write a PNG showing the tray and taskbar sizes on dark and light backgrounds.
  [string]$Preview = ''
)
Add-Type -AssemblyName System.Drawing

function Scaled([int]$Size, $src) {
  $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  # Clamp edge sampling so the transparent border does not bleed a dark fringe into the tile.
  $attr = New-Object System.Drawing.Imaging.ImageAttributes
  $attr.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY)
  $g.DrawImage($src, (New-Object System.Drawing.Rectangle 0, 0, $Size, $Size), 0, 0, $src.Width, $src.Height, [System.Drawing.GraphicsUnit]::Pixel, $attr)
  $g.Dispose(); return $bmp
}

# Classic icon frame: BITMAPINFOHEADER + bottom-up 32-bit BGRA pixels + an all-zero AND mask (alpha is used).
function Dib($bmp) {
  $n = $bmp.Width
  $ms = New-Object System.IO.MemoryStream
  $w = New-Object System.IO.BinaryWriter $ms
  $w.Write([uint32]40); $w.Write([int32]$n); $w.Write([int32]($n * 2)); $w.Write([uint16]1); $w.Write([uint16]32)
  $w.Write([uint32]0); $w.Write([uint32]0); $w.Write([int32]0); $w.Write([int32]0); $w.Write([uint32]0); $w.Write([uint32]0)
  for ($y = $n - 1; $y -ge 0; $y--) {
    for ($x = 0; $x -lt $n; $x++) {
      $c = $bmp.GetPixel($x, $y)
      $w.Write([byte]$c.B); $w.Write([byte]$c.G); $w.Write([byte]$c.R); $w.Write([byte]$c.A)
    }
  }
  $maskRow = [int]([math]::Ceiling($n / 32.0) * 4)
  $w.Write((New-Object byte[] ($maskRow * $n)))
  $w.Flush(); return , $ms.ToArray()
}

function PngBytes($bmp) {
  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  return , $ms.ToArray()
}

function OutPath([string]$Relative) {
  $full = Join-Path (Resolve-Path $Root) $Relative
  return $full
}

$src = [System.Drawing.Image]::FromFile((Resolve-Path $Source))

# --- app.ico: bitmap frames up to 128 px, PNG for 256 px (the standard layout since Windows Vista).
$frames = New-Object System.Collections.ArrayList
foreach ($size in 16, 20, 24, 32, 40, 48, 64, 128, 256) {
  $bmp = Scaled $size $src
  $data = if ($size -ge 256) { PngBytes $bmp } else { Dib $bmp }
  $bmp.Dispose()
  [void]$frames.Add(@{ Size = $size; Data = [byte[]]$data })
}
$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$frames.Count)
$offset = 6 + 16 * $frames.Count
foreach ($f in $frames) {
  $dim = if ($f.Size -ge 256) { 0 } else { $f.Size }
  $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$f.Data.Length); $w.Write([uint32]$offset)
  $offset += $f.Data.Length
}
foreach ($f in $frames) { $w.Write($f.Data) }
$w.Flush()
$ico = OutPath 'apps\tray\assets\app.ico'
[System.IO.File]::WriteAllBytes($ico, $out.ToArray())
Write-Output "Wrote $ico ($($out.Length) bytes, $($frames.Count) sizes)"

# --- icon-32.rgba: raw top-down RGBA for the tray.
$tray = Scaled 32 $src
$bytes = New-Object byte[] (32 * 32 * 4)
$i = 0
for ($y = 0; $y -lt 32; $y++) {
  for ($x = 0; $x -lt 32; $x++) {
    $c = $tray.GetPixel($x, $y)
    $bytes[$i] = $c.R; $bytes[$i + 1] = $c.G; $bytes[$i + 2] = $c.B; $bytes[$i + 3] = $c.A
    $i += 4
  }
}
$rgba = OutPath 'apps\tray\assets\icon-32.rgba'
[System.IO.File]::WriteAllBytes($rgba, $bytes)
Write-Output "Wrote $rgba ($($bytes.Length) bytes)"

# --- README logo.
$logo = Scaled 256 $src
$logoPath = OutPath 'docs\images\logo-256.png'
$logo.Save($logoPath, [System.Drawing.Imaging.ImageFormat]::Png); $logo.Dispose()
Write-Output "Wrote $logoPath"

if ($Preview) {
  # 16, 24 and 32 px at real size on a dark and a light taskbar, then the tray icon 6x enlarged.
  $big = New-Object System.Drawing.Bitmap 360, 192
  $bg = [System.Drawing.Graphics]::FromImage($big)
  $bg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
  $bg.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
  $bg.Clear([System.Drawing.Color]::FromArgb(255, 32, 32, 32))
  $bg.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 238, 238, 238))), 0, 96, 168, 96)
  $x = 12
  foreach ($size in 16, 24, 32) {
    $s = Scaled $size $src
    $bg.DrawImage($s, $x, 32, $size, $size); $bg.DrawImage($s, $x, 128, $size, $size)
    $s.Dispose(); $x += $size + 16
  }
  $bg.DrawImage($tray, 168, 0, 192, 192)
  $bg.Dispose(); $big.Save($Preview, [System.Drawing.Imaging.ImageFormat]::Png); $big.Dispose()
  Write-Output "Preview $Preview"
}
$tray.Dispose()
$src.Dispose()
