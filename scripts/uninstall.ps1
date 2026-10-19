param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\ScreenShotFX",
    [switch]$PurgeData,
    [switch]$SkipRegistry,
    [switch]$SkipShortcut
)

$ErrorActionPreference = 'Stop'

Write-Host '==> 停止 fxshot'
Get-Process fxshot -ErrorAction SilentlyContinue | ForEach-Object {
    try { Stop-Process -Id $_.Id -Force -ErrorAction Stop } catch {
        Write-Warning "无法停止进程 $($_.Id)：$($_.Exception.Message)"
    }
}

if (-not $SkipRegistry) {
    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    try {
        Remove-ItemProperty -Path $runKey -Name 'ScreenShotFX' -ErrorAction Stop
        Write-Host '==> 已移除开机自启'
    }
    catch {
        Write-Host '==> 没有找到开机自启项，跳过'
    }
}

if (-not $SkipShortcut) {
    $shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\ScreenShot FX.lnk'
    if (Test-Path $shortcut) {
        Remove-Item $shortcut -Force
        Write-Host '==> 已移除开始菜单快捷方式'
    }
}

if (Test-Path $InstallDir) {
    $resolved = (Resolve-Path $InstallDir).Path
    if ((Test-Path (Join-Path $resolved 'fxshot.exe')) -or ($resolved -like '*ScreenShotFX*')) {
        Remove-Item $resolved -Recurse -Force
        Write-Host "==> 已删除安装目录 $resolved"
    }
    else {
        Write-Warning "安装目录内容不像 ScreenShot FX，未删除：$resolved"
    }
}

if ($PurgeData) {
    foreach ($dir in @(
        (Join-Path $env:APPDATA 'ScreenShotFX'),
        (Join-Path $env:LOCALAPPDATA 'ScreenShotFX')
    )) {
        if (Test-Path $dir) {
            Remove-Item $dir -Recurse -Force
            Write-Host "==> 已删除数据目录 $dir"
        }
    }
}

Write-Host ''
Write-Host '卸载完成。截图目录（Pictures\fx）没有被删除。'
