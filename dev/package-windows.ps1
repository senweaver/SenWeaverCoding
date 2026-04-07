# SenWeaverCoding Windows MSI Installer Builder
# Usage:
#   .\dev\package-windows.ps1             # 构建 release + MSI
#   .\dev\package-windows.ps1 -SkipBuild  # 跳过 cargo build，直接打包
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$ProjectDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $ProjectDir

$Version = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches.Groups[1].Value

$WixExe = @(
    "$env:ProgramFiles\WiX Toolset v6.0\bin\wix.exe",
    "${env:ProgramFiles(x86)}\WiX Toolset v6.0\bin\wix.exe",
    "$env:LOCALAPPDATA\Programs\WiX Toolset v6.0\bin\wix.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $WixExe) {
    Write-Error "WiX Toolset v6 not found. Install via: winget install WiXToolset.WiXCLI"
    exit 1
}

Write-Host "`n==> SenWeaverCoding v$Version MSI builder" -ForegroundColor Cyan

if (-not $SkipBuild) {
    Write-Host "==> Building release binary..." -ForegroundColor Blue
    cargo build --release
    if ($LASTEXITCODE -ne 0) { Write-Error "Build failed"; exit 1 }
}

if (-not (Test-Path "target\release\sen.exe")) {
    Write-Error "Binary not found at target\release\sen.exe — run without -SkipBuild"
    exit 1
}

New-Item -ItemType Directory -Force -Path "dist" | Out-Null

$MsiOut = "dist\SenWeaverCoding-$Version.msi"

Write-Host "==> Compiling MSI with WiX Toolset..." -ForegroundColor Blue
& $WixExe build -ext WixToolset.UI.wixext wix\main.wxs -o $MsiOut
if ($LASTEXITCODE -ne 0) { Write-Error "WiX build failed"; exit 1 }

if (Test-Path $MsiOut) {
    $Size = (Get-Item $MsiOut).Length / 1MB
    Write-Host "`n==> MSI installer ready!" -ForegroundColor Green
    Write-Host "    File: $MsiOut" -ForegroundColor Green
    Write-Host "    Size: $([math]::Round($Size, 1)) MB" -ForegroundColor Green
} else {
    Write-Error "Expected output not found: $MsiOut"
    exit 1
}
