param(
    [string]$SourceExe = '',
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\ScreenShotFX",
    [switch]$NoStart,
    [switch]$NoAutostart,
    [switch]$NoShortcut
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'ScreenShot FX 目前只提供 Windows x64 版本。'
}

if ([string]::IsNullOrWhiteSpace($SourceExe)) {
    $candidates = @(
        (Join-Path $root 'fxshot.exe'),
        (Join-Path $root 'dist\fxshot.exe'),
        (Join-Path $root 'target\release\fxshot.exe')
    )
    $SourceExe = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $SourceExe) {
        throw '找不到 fxshot.exe，请先运行 scripts\build.ps1，或在解压后的发布包中运行本脚本。'
    }
}
if (-not (Test-Path $SourceExe)) {
    throw "找不到源程序：$SourceExe"
}

Write-Host "==> 停止正在运行的 fxshot"
Get-Process fxshot -ErrorAction SilentlyContinue | ForEach-Object {
    try { Stop-Process -Id $_.Id -Force -ErrorAction Stop } catch {
        Write-Warning "无法停止进程 $($_.Id)：$($_.Exception.Message)"
    }
}

Write-Host "==> 安装到 $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$target = Join-Path $InstallDir 'fxshot.exe'
Copy-Item $SourceExe $target -Force

if (-not $NoShortcut) {
    try {
        $programs = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
        New-Item -ItemType Directory -Force -Path $programs | Out-Null
        $shortcutPath = Join-Path $programs 'ScreenShot FX.lnk'
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut($shortcutPath)
        $shortcut.TargetPath = $target
        $shortcut.WorkingDirectory = $InstallDir
        $shortcut.Description = 'ScreenShot FX'
        $shortcut.Save()
        Write-Host "==> 已创建开始菜单快捷方式"
    }
    catch {
        Write-Warning "创建快捷方式失败：$($_.Exception.Message)"
    }
}

if (-not $NoAutostart) {
    Write-Host '==> 写入开机自启'
    & $target --set-autostart on
    if ($LASTEXITCODE -ne 0) {
        Write-Warning '写入开机自启失败；首次正常启动时会再次尝试。'
    }
}

if (-not $NoStart) {
    Write-Host '==> 启动 ScreenShot FX'
    Start-Process -FilePath $target -WorkingDirectory $InstallDir
}

Write-Host ''
Write-Host '安装完成。'
Write-Host '  Shift+F1  选区截图'
Write-Host '  Shift+F2  快速截图（鼠标所在显示器）'
Write-Host "  程序位置  $target"
Write-Host '  卸载脚本  scripts\uninstall.ps1'
