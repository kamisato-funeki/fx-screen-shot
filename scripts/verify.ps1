param(
    [string]$Exe = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($Exe)) {
    $Exe = Join-Path $root 'dist\fxshot.exe'
    if (-not (Test-Path $Exe)) { $Exe = Join-Path $root 'target\release\fxshot.exe' }
}
if (-not (Test-Path $Exe)) { throw "找不到 fxshot.exe：$Exe" }

Add-Type -AssemblyName System.Drawing
$outDir = Join-Path $root 'dist\verify'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

function Wait-ForFile {
    param([string]$Path, [int]$TimeoutSeconds = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $Path) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

function Test-Capture {
    param([string]$Backend)
    $path = Join-Path $outDir ("verify-{0}.png" -f $Backend)
    Remove-Item $path -ErrorAction SilentlyContinue
    Write-Host "==> 测试 $Backend ..."
    $p = Start-Process -FilePath $Exe -ArgumentList @('--capture-once', $path, '--backend', $Backend) -PassThru
    try { $p.WaitForExit(15000) | Out-Null } catch { }
    if (-not (Wait-ForFile -Path $path -TimeoutSeconds 10)) {
        throw "$Backend 截图未生成文件"
    }
    $img = [System.Drawing.Image]::FromFile($path)
    try {
        $width = $img.Width
        $height = $img.Height
    }
    finally {
        $img.Dispose()
    }
    return [PSCustomObject]@{
        Backend = $Backend
        Width   = $width
        Height  = $height
        Bytes   = (Get-Item $path).Length
        Path    = $path
    }
}

$results = @()
$failed = $false
foreach ($backend in @('gdi', 'wgc')) {
    try {
        $results += Test-Capture -Backend $backend
    }
    catch {
        $failed = $true
        Write-Host "FAIL: $($_.Exception.Message)" -ForegroundColor Red
    }
}

$results | Format-Table -AutoSize

if ($results.Count -ge 2) {
    $a = $results[0]
    $b = $results[1]
    if ($a.Width -ne $b.Width -or $a.Height -ne $b.Height) {
        Write-Host 'FAIL: GDI 与 WGC 截图尺寸不一致' -ForegroundColor Red
        $failed = $true
    }
}

if ($failed) {
    Write-Host 'VERIFY FAILED' -ForegroundColor Red
    exit 1
}
Write-Host 'VERIFY PASSED' -ForegroundColor Green
Write-Host "输出目录：$outDir"
