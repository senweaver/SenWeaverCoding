# SenWeaverCoding CLI installer — Windows (PowerShell)
# Usage: irm https://raw.githubusercontent.com/senweaver/SenWeaverCoding/master/install.ps1 | iex
#   Or:  .\install.ps1
$ErrorActionPreference = "Stop"

$Repo = "senweaver/SenWeaverCoding"
$BinaryName = "sen"
$InstallDir = if ($env:SEN_INSTALL_DIR) { $env:SEN_INSTALL_DIR } else { "$env:USERPROFILE\.senweavercoding\bin" }

function Write-Info { Write-Host "==> $args" -ForegroundColor Cyan }
function Write-Ok   { Write-Host "==> $args" -ForegroundColor Green }
function Write-Err  { Write-Host "error: $args" -ForegroundColor Red; exit 1 }

function Get-Platform {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        "X64"   { return "x86_64-pc-windows-msvc" }
        "Arm64" { return "aarch64-pc-windows-msvc" }
        default { Write-Err "Unsupported architecture: $arch" }
    }
}

function Main {
    Write-Info "Detecting platform..."
    $triple = Get-Platform
    Write-Info "Platform: $triple"

    $version = if ($env:SEN_VERSION) { $env:SEN_VERSION } else { "latest" }

    if ($version -eq "latest") {
        $apiUrl = "https://api.github.com/repos/$Repo/releases/latest"
    } else {
        $apiUrl = "https://api.github.com/repos/$Repo/releases/tags/v$version"
    }

    Write-Info "Fetching release info..."
    try {
        $release = Invoke-RestMethod -Uri $apiUrl -Headers @{ "User-Agent" = "sen-installer" }
    } catch {
        Write-Err "Failed to fetch release info: $_"
    }

    # Find asset matching platform (look for .zip for Windows)
    $asset = $release.assets | Where-Object { $_.name -match $triple } | Select-Object -First 1

    if (-not $asset) {
        Write-Err "No release asset found for $triple. Check https://github.com/$Repo/releases"
    }

    $downloadUrl = $asset.browser_download_url
    Write-Info "Downloading from $downloadUrl..."

    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "sen-install-$(Get-Random)"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        $archivePath = Join-Path $tmpDir "sen.zip"

        # Download
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath -UseBasicParsing

        Write-Info "Extracting..."
        Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force

        # Find the binary
        $binary = Get-ChildItem -Path $tmpDir -Recurse -Filter "$BinaryName.exe" | Select-Object -First 1

        if (-not $binary) {
            Write-Err "Binary '$BinaryName.exe' not found in archive."
        }

        Write-Info "Installing to $InstallDir\$BinaryName.exe..."
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        Copy-Item -Path $binary.FullName -Destination "$InstallDir\$BinaryName.exe" -Force

        # Add to PATH if not already there
        $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($currentPath -notlike "*$InstallDir*") {
            [Environment]::SetEnvironmentVariable(
                "Path",
                "$InstallDir;$currentPath",
                "User"
            )
            $env:Path = "$InstallDir;$env:Path"
            Write-Ok "Added $InstallDir to your PATH (restart terminal to take effect)."
        }

        Write-Ok "Installed $BinaryName.exe to $InstallDir"

        # Verify
        try {
            $ver = & "$InstallDir\$BinaryName.exe" --version 2>$null
            Write-Ok "Version: $ver"
        } catch {
            Write-Ok "Installed successfully."
        }

        Write-Host ""
        Write-Host "Get started:"
        Write-Host "  $BinaryName onboard        # first-time setup"
        Write-Host "  $BinaryName agent           # start interactive session"
        Write-Host "  $BinaryName agent -m 'hi'   # single message"

    } finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Main
