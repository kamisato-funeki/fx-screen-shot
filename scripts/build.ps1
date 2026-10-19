param(
    [switch]$Install,
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    if (-not $SkipTests) {
        Write-Host '==> cargo test --offline'
        & cargo test --offline
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed (exit $LASTEXITCODE)" }
    }

    Write-Host '==> cargo build --release --offline'
    & cargo build --release --offline
    if ($LASTEXITCODE -ne 0) {
        Write-Host 'Offline build failed; retrying with network access...'
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
    }

    $exe = Join-Path $root 'target\release\fxshot.exe'
    if (-not (Test-Path $exe)) { throw "Release executable not found: $exe" }

    $dist = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item $exe (Join-Path $dist 'fxshot.exe') -Force
    Copy-Item (Join-Path $root 'config.example.toml') (Join-Path $dist 'config.example.toml') -Force
    Copy-Item (Join-Path $root 'README.md') (Join-Path $dist 'README.md') -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $root 'LICENSE') (Join-Path $dist 'LICENSE') -Force -ErrorAction SilentlyContinue

    $item = Get-Item (Join-Path $dist 'fxshot.exe')
    $hash = (Get-FileHash $item.FullName -Algorithm SHA256).Hash
    Write-Host ""
    Write-Host ("Built: {0}" -f $item.FullName)
    Write-Host ("Size : {0:N0} bytes" -f $item.Length)
    Write-Host ("SHA256: {0}" -f $hash)

    if ($Install) {
        & (Join-Path $PSScriptRoot 'install.ps1')
    }
}
finally {
    Pop-Location
}
