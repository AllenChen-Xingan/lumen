#!/usr/bin/env pwsh
# Lumen installer for Windows
# Usage: irm https://raw.githubusercontent.com/AllenChen-Xingan/lumen/master/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"
$repo = "AllenChen-Xingan/lumen"

Write-Host ""
Write-Host "  Lumen — structured feed intelligence" -ForegroundColor Cyan
Write-Host ""

# Install directory
$installDir = Join-Path $env:LOCALAPPDATA "lumen" "bin"
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

# Download latest release
$downloaded = $false
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
    $asset = $release.assets | Where-Object { $_.name -like "*windows*x86_64*" } | Select-Object -First 1

    if ($asset) {
        $tarPath = Join-Path $env:TEMP "lumen-download.tar.gz"
        Write-Host "  Downloading $($release.tag_name)..." -ForegroundColor Gray
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tarPath
        tar -xzf $tarPath -C $installDir
        Remove-Item $tarPath
        $downloaded = $true
    }
} catch {
    Write-Host "  Could not download release." -ForegroundColor Yellow
}

# Fallback: build from source
if (-not $downloaded) {
    Write-Host "  Building from source (requires Rust + Node.js)..." -ForegroundColor Yellow

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "  Error: Rust is required. Install from https://rustup.rs" -ForegroundColor Red
        exit 1
    }

    $tempDir = Join-Path $env:TEMP "lumen-build-$(Get-Random)"
    git clone --depth 1 "https://github.com/$repo.git" $tempDir
    Push-Location $tempDir
    pnpm install
    cargo build --release -p lumen
    cargo tauri build
    Copy-Item "target\release\lumen.exe" $installDir
    Copy-Item "target\release\lumen-app.exe" $installDir
    Pop-Location
    Remove-Item $tempDir -Recurse -Force
}

# Add to PATH (for CLI / agent use)
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
    $env:Path = "$env:Path;$installDir"
}

# Create desktop shortcut
$appPath = Join-Path $installDir "lumen-app.exe"
if (Test-Path $appPath) {
    $desktopPath = [Environment]::GetFolderPath("Desktop")
    $shortcutPath = Join-Path $desktopPath "Lumen.lnk"
    $WshShell = New-Object -ComObject WScript.Shell
    $shortcut = $WshShell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $appPath
    $shortcut.WorkingDirectory = $installDir
    $shortcut.Description = "Lumen — structured feed intelligence"
    $shortcut.Save()
}

# Verify
$lumenPath = Join-Path $installDir "lumen.exe"
if ((Test-Path $lumenPath) -and (Test-Path $appPath)) {
    Write-Host ""
    Write-Host "  Lumen installed!" -ForegroundColor Green
    Write-Host ""
    Write-Host "  Desktop shortcut created — double-click Lumen to open." -ForegroundColor White
    Write-Host ""
} else {
    Write-Host "  Installation failed." -ForegroundColor Red
    exit 1
}
