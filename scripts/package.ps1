param(
    [string]$Version = '',
    [string]$OutputDir = 'dist',
    [string]$ExePath = '',
    [switch]$SkipBuild
)

# 生成发布包：
#   dist\fxshot.exe
#   dist\fxshot-<version>-windows-x64.zip
#   dist\SHA256SUMS.txt
#
# 本地用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package.ps1
# CI 用法：
#   .\scripts\package.ps1 -Version 0.1.1 -SkipBuild

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    if ([string]::IsNullOrWhiteSpace($Version)) {
        $toml = Get-Content -Raw (Join-Path $root 'Cargo.toml')
        if ($toml -notmatch '(?m)^\s*version\s*=\s*"([^"]+)"') {
            throw '无法从 Cargo.toml 读取 version'
        }
        $Version = $Matches[1]
    }
    $Version = $Version.Trim().TrimStart('v')
    if ($Version -notmatch '^\d+\.\d+\.\d+([-.][0-9A-Za-z.-]+)?$') {
        throw "版本号格式无效：'$Version'"
    }

    if ([string]::IsNullOrWhiteSpace($ExePath)) {
        $ExePath = Join-Path $root 'target\release\fxshot.exe'
    }
    if (-not $SkipBuild) {
        Write-Host '==> cargo build --release --locked'
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（exit $LASTEXITCODE）" }
    }
    if (-not (Test-Path $ExePath)) {
        throw "找不到 Release 可执行文件：$ExePath（请先运行 cargo build --release，或去掉 -SkipBuild）"
    }

    $outDir = Join-Path $root $OutputDir
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $outDir = (Resolve-Path $outDir).Path

    $packageName = "fxshot-$Version-windows-x64"
    $stage = Join-Path $outDir $packageName
    $resolvedOut = (Resolve-Path $outDir).Path.TrimEnd('\')
    $resolvedStageParent = (Split-Path -Parent $stage)
    if ($resolvedStageParent -ne $resolvedOut) {
        throw "打包暂存目录不在输出目录内，已中止：$stage"
    }
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    $files = @(
        @{ Source = $ExePath; Target = 'fxshot.exe' },
        @{ Source = (Join-Path $root 'config.example.toml'); Target = 'config.example.toml' },
        @{ Source = (Join-Path $root 'README.md'); Target = 'README.md' },
        @{ Source = (Join-Path $root 'LICENSE'); Target = 'LICENSE' },
        @{ Source = (Join-Path $root 'scripts\install.ps1'); Target = 'scripts\install.ps1' },
        @{ Source = (Join-Path $root 'scripts\uninstall.ps1'); Target = 'scripts\uninstall.ps1' }
    )
    foreach ($file in $files) {
        if (-not (Test-Path $file.Source)) { throw "缺少打包文件：$($file.Source)" }
        $target = Join-Path $stage $file.Target
        $parent = Split-Path -Parent $target
        if (-not (Test-Path $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
        Copy-Item $file.Source $target -Force
    }

    $versionFile = Join-Path $stage 'VERSION.txt'
    Set-Content -Path $versionFile -Value "ScreenShot FX $Version`r`nWindows x64" -Encoding ASCII

    $zipPath = Join-Path $outDir "$packageName.zip"
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zipPath -CompressionLevel Optimal

    $releaseExe = Join-Path $outDir 'fxshot.exe'
    Copy-Item $ExePath $releaseExe -Force

    $hashFiles = @(
        (Get-Item $zipPath),
        (Get-Item $releaseExe)
    )
    $lines = foreach ($file in $hashFiles) {
        $hash = (Get-FileHash $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        '{0}  {1}' -f $hash, $file.Name
    }
    $sumsPath = Join-Path $outDir 'SHA256SUMS.txt'
    Set-Content -Path $sumsPath -Value $lines -Encoding ASCII

    Write-Host ''
    Write-Host ("版本   : {0}" -f $Version)
    Write-Host ("ZIP    : {0} ({1:N0} bytes)" -f $zipPath, (Get-Item $zipPath).Length)
    Write-Host ("EXE    : {0} ({1:N0} bytes)" -f $releaseExe, (Get-Item $releaseExe).Length)
    Write-Host 'SHA256 :'
    $lines | ForEach-Object { Write-Host "  $_" }
    Write-Host ''
    Write-Host '发布包内容：'
    Get-ChildItem $stage -Recurse -File | ForEach-Object {
        Write-Host ("  {0}" -f $_.FullName.Substring($stage.Length + 1))
    }
}
finally {
    Pop-Location
}
