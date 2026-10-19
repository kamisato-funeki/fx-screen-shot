# Generates assets/app.ico with multiple sizes. Requires Windows PowerShell/.NET drawing.
param(
    [string]$OutFile = (Join-Path $PSScriptRoot '..\assets\app.ico')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$sizes = @(16, 20, 24, 32, 40, 48, 64, 256)
$pngs = New-Object System.Collections.Generic.List[byte[]]

foreach ($s in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.Clear([System.Drawing.Color]::Transparent)

    $inset = [math]::Max(1, [int]($s * 0.04))
    $rect = New-Object System.Drawing.Rectangle($inset, $inset, ($s - 2 * $inset), ($s - 2 * $inset))
    $c1 = [System.Drawing.Color]::FromArgb(255, 64, 145, 255)
    $c2 = [System.Drawing.Color]::FromArgb(255, 20, 84, 200)
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, $c1, $c2, 55.0)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $r = [math]::Max(2, [int]($s * 0.22))
    $d = $r * 2
    $x0 = $inset; $y0 = $inset; $x1 = $s - $inset; $y1 = $s - $inset
    $path.AddArc($x0, $y0, $d, $d, 180, 90)
    $path.AddArc($x1 - $d, $y0, $d, $d, 270, 90)
    $path.AddArc($x1 - $d, $y1 - $d, $d, $d, 0, 90)
    $path.AddArc($x0, $y1 - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $g.FillPath($brush, $path)

    $penWidth = [math]::Max(1.15, $s * 0.062)
    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::White, $penWidth)
    $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $m = $s * 0.29
    $len = $s * 0.155

    $g.DrawLines($pen, @(
        (New-Object System.Drawing.PointF(($m + $len), $m)),
        (New-Object System.Drawing.PointF($m, $m)),
        (New-Object System.Drawing.PointF($m, ($m + $len)))
    ))
    $g.DrawLines($pen, @(
        (New-Object System.Drawing.PointF(($s - $m - $len), $m)),
        (New-Object System.Drawing.PointF(($s - $m), $m)),
        (New-Object System.Drawing.PointF(($s - $m), ($m + $len)))
    ))
    $g.DrawLines($pen, @(
        (New-Object System.Drawing.PointF($m, ($s - $m - $len))),
        (New-Object System.Drawing.PointF($m, ($s - $m))),
        (New-Object System.Drawing.PointF(($m + $len), ($s - $m)))
    ))
    $g.DrawLines($pen, @(
        (New-Object System.Drawing.PointF(($s - $m), ($s - $m - $len))),
        (New-Object System.Drawing.PointF(($s - $m), ($s - $m))),
        (New-Object System.Drawing.PointF(($s - $m - $len), ($s - $m)))
    ))

    $dotR = [math]::Max(0.9, $s * 0.035)
    $dotBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
    $g.FillEllipse($dotBrush, ($s / 2 - $dotR), ($s / 2 - $dotR), (2 * $dotR), (2 * $dotR))

    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngs.Add($ms.ToArray())

    $dotBrush.Dispose(); $pen.Dispose(); $brush.Dispose(); $path.Dispose(); $g.Dispose(); $bmp.Dispose(); $ms.Dispose()
}

$dir = Split-Path -Parent $OutFile
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
$fs = [System.IO.File]::Create($OutFile)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([UInt16]0)
$bw.Write([UInt16]1)
$bw.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $dim = if ($s -ge 256) { 0 } else { $s }
    $bw.Write([byte]$dim)
    $bw.Write([byte]$dim)
    $bw.Write([byte]0)
    $bw.Write([byte]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]32)
    $bw.Write([UInt32]$pngs[$i].Length)
    $bw.Write([UInt32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($png in $pngs) { $bw.Write($png) }
$bw.Flush()
$bw.Close()
$fs.Close()
Write-Host "Wrote $OutFile ($((Get-Item $OutFile).Length) bytes)"
