# Regenerates apps/tray/assets/icon-32.rgba (raw 32x32 RGBA) for the system tray.
# The full app badge is unreadable at tray size, so this draws its central ">_" terminal tile
# directly at pixel size, in the badge's colors (blue-to-teal ring, dark tile, white glyph).
# Uses only System.Drawing, which ships with Windows.
param(
  [string]$Output = (Join-Path $PSScriptRoot '..\apps\tray\assets\icon-32.rgba'),
  [int]$Size = 32,
  # Optional: also write an enlarged PNG preview of the result.
  [string]$Preview = ''
)
Add-Type -AssemblyName System.Drawing
$s = $Size / 32.0
$bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$g.Clear([System.Drawing.Color]::Transparent)

function RoundedRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $p.AddArc($x, $y, $d, $d, 180, 90); $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90); $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $p.CloseFigure(); return $p
}
$blue = [System.Drawing.Color]::FromArgb(255, 59, 142, 214)
$teal = [System.Drawing.Color]::FromArgb(255, 45, 190, 170)
$tile = RoundedRect (1.5 * $s) (3.5 * $s) (29 * $s) (25 * $s) (6 * $s)
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 14, 18, 24))), $tile)
$ring = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.PointF 0, 0), (New-Object System.Drawing.PointF $Size, $Size), $blue, $teal
$g.DrawPath((New-Object System.Drawing.Pen $ring, (2.2 * $s)), $tile)
$glyph = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), (3.2 * $s)
$glyph.StartCap = [System.Drawing.Drawing2D.LineCap]::Round; $glyph.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
$glyph.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
$g.DrawLines($glyph, [System.Drawing.PointF[]]@((New-Object System.Drawing.PointF (8 * $s), (10 * $s)), (New-Object System.Drawing.PointF (14 * $s), (16 * $s)), (New-Object System.Drawing.PointF (8 * $s), (22 * $s))))
$g.DrawLine($glyph, (17 * $s), (22 * $s), (24 * $s), (22 * $s))
$g.Dispose()

$bytes = New-Object byte[] ($Size * $Size * 4)
$i = 0
for ($y = 0; $y -lt $Size; $y++) {
  for ($x = 0; $x -lt $Size; $x++) {
    $c = $bmp.GetPixel($x, $y)
    $bytes[$i] = $c.R; $bytes[$i + 1] = $c.G; $bytes[$i + 2] = $c.B; $bytes[$i + 3] = $c.A
    $i += 4
  }
}
[System.IO.File]::WriteAllBytes((Join-Path (Resolve-Path (Split-Path $Output)) (Split-Path $Output -Leaf)), $bytes)
Write-Output "Wrote $Output ($($bytes.Length) bytes)"
if ($Preview) {
  # Real size on a dark and a light taskbar, then 6x enlarged.
  $big = New-Object System.Drawing.Bitmap (($Size * 6) + 120), ($Size * 6)
  $bg = [System.Drawing.Graphics]::FromImage($big)
  $bg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
  $bg.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
  $bg.Clear([System.Drawing.Color]::FromArgb(255, 32, 32, 32))
  $bg.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 238, 238, 238))), 0, 60, 60, 60)
  $bg.DrawImage($bmp, 14, 14, $Size, $Size); $bg.DrawImage($bmp, 14, 74, $Size, $Size)
  $bg.DrawImage($bmp, 120, 0, $Size * 6, $Size * 6)
  $bg.Dispose(); $big.Save($Preview, [System.Drawing.Imaging.ImageFormat]::Png); $big.Dispose()
  Write-Output "Preview $Preview"
}
$bmp.Dispose()
