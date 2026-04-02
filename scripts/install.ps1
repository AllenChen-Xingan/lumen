#!/usr/bin/env pwsh
# Lumen installer for Windows
# Usage: irm https://raw.githubusercontent.com/xingan-chen/lumen/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"
$repo = "xingan-chen/lumen"
$binName = "lumen.exe"

Write-Host "Installing Lumen..." -ForegroundColor Cyan

# Determine install directory
$installDir = Join-Path $env:LOCALAPPDATA "lumen" "bin"
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

# Get latest release
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
    $asset = $release.assets | Where-Object { $_.name -like "*windows*x86_64*" -or $_.name -eq "lumen-windows.zip" } | Select-Object -First 1
} catch {
    Write-Host "No release found. Building from source..." -ForegroundColor Yellow

    # Check dependencies
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "Error: Rust is required. Install from https://rustup.rs" -ForegroundColor Red
        exit 1
    }

    $tempDir = Join-Path $env:TEMP "lumen-build-$(Get-Random)"
    git clone --depth 1 "https://github.com/$repo.git" $tempDir
    Push-Location $tempDir
    cargo build --release -p lumen
    Copy-Item "target\release\$binName" $installDir
    Pop-Location
    Remove-Item $tempDir -Recurse -Force
    Write-Host "Built from source." -ForegroundColor Green
    $asset = $null
}

if ($asset) {
    $zipPath = Join-Path $env:TEMP "lumen-download.zip"
    Write-Host "Downloading $($asset.name)..."
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath
    Expand-Archive -Path $zipPath -DestinationPath $installDir -Force
    Remove-Item $zipPath
    Write-Host "Downloaded release." -ForegroundColor Green
}

# Add to PATH if not already there
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
    $env:Path = "$env:Path;$installDir"
    Write-Host "Added $installDir to PATH." -ForegroundColor Green
}

# Verify
$lumenPath = Join-Path $installDir $binName
if (Test-Path $lumenPath) {
    Write-Host ""
    Write-Host "Lumen installed successfully!" -ForegroundColor Green
    Write-Host "  Binary: $lumenPath" -ForegroundColor Gray
    Write-Host ""
    Write-Host "Get started:" -ForegroundColor Cyan
    Write-Host "  lumen add https://example.com/feed.xml"
    Write-Host "  lumen articles --compact"
    Write-Host "  lumen search 'topic' --compact"
    Write-Host ""
    Write-Host "Restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
} else {
    Write-Host "Installation failed. Binary not found at $lumenPath" -ForegroundColor Red
    exit 1
}
