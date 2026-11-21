# Builds apps/tray/assets/app.ico (embedded in ctrl-alt-delegate.exe and cad.exe).
# Small sizes use the drawn ">_" tile (readable in the taskbar and Task Manager);
# large sizes use the full approved badge. Uses only System.Drawing, which ships with Windows.
param(
  [string]$Badge = (Join-Path $PSScriptRoot '..\Ctrl Alt Delegate_Icon.png'),
  [string]$Output = (Join-Path $PSScriptRoot '..\apps\tray\assets\app.ico')
)
Add-Type -AssemblyName System.Drawing

function RoundedRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $p.AddArc($x, $y, $d, $d, 180, 90); $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90); $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $p.CloseFigure(); return $p
}

# Same design as scripts/make-tray-icon.ps1, drawn at any size.
function Glyph([int]$Size) {
  $s = $Size / 32.0
  $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  $blue = [System.Drawing.Color]::FromArgb(255, 59, 142, 214)
  $teal = [System.Drawing.Color]::FromArgb(255, 45, 190, 170)
  $tile = RoundedRect (1.5 * $s) (3.5 * $s) (29 * $s) (25 * $s) (6 * $s)
  $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 14, 18, 24))), $tile)
  $ring = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.PointF 0, 0), (New-Object System.Drawing.PointF $Size, $Size), $blue, $teal
  $g.DrawPath((New-Object System.Drawing.Pen $ring, (2.2 * $s)), $tile)
  $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), (3.2 * $s)
  $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round; $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
  $g.DrawLines($pen, [System.Drawing.PointF[]]@((New-Object System.Drawing.PointF (8 * $s), (10 * $s)), (New-Object System.Drawing.PointF (14 * $s), (16 * $s)), (New-Object System.Drawing.PointF (8 * $s), (22 * $s))))
  $g.DrawLine($pen, (17 * $s), (22 * $s), (24 * $s), (22 * $s))
  $g.Dispose(); return $bmp
}

function BadgeAt([int]$Size, $src) {
  $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  # Clip to the round badge so the corners are transparent.
  $clip = New-Object System.Drawing.Drawing2D.GraphicsPath
  $clip.AddEllipse(0, 0, $Size, $Size); $g.SetClip($clip)
  $g.DrawImage($src, 0, 0, $Size, $Size)
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

$src = [System.Drawing.Image]::FromFile((Resolve-Path $Badge))
$frames = New-Object System.Collections.ArrayList
foreach ($size in 16, 20, 24, 32, 40, 48, 64, 128, 256) {
  $bmp = if ($size -le 48) { Glyph $size } else { BadgeAt $size $src }
  if ($size -ge 256) {
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $data = $ms.ToArray()
  } else {
    $data = Dib $bmp
  }
  $bmp.Dispose()
  [void]$frames.Add(@{ Size = $size; Data = [byte[]]$data })
}
$src.Dispose()

# ICO container: bitmap frames up to 128 px, PNG for 256 px (the standard layout since Windows Vista).
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
[System.IO.File]::WriteAllBytes((Join-Path (Resolve-Path (Split-Path $Output)) (Split-Path $Output -Leaf)), $out.ToArray())
Write-Output "Wrote $Output ($($out.Length) bytes, $($frames.Count) sizes)"
