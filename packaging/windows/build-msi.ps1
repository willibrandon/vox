# build-msi.ps1 — Build a Windows MSI installer for Vox.
#
# Prerequisites:
#   - Rust toolchain with cargo
#   - WiX Toolset v4: dotnet tool install --global wix
#   - CUDA 12.8+ (for vox_core/cuda feature)
#
# Usage:
#   .\packaging\windows\build-msi.ps1
#
# Output:
#   packaging\windows\output\vox.msi

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
$OutputDir = "$PSScriptRoot\output"
$WixDir = "$PSScriptRoot\wix"

Write-Host "=== Vox MSI Builder ===" -ForegroundColor Cyan

# Step 1: Build release binary
Write-Host "`n[1/4] Building release binary..." -ForegroundColor Yellow
Push-Location $RepoRoot
try {
    cargo build --release -p vox --features vox_core/cuda
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$BinaryPath = "$RepoRoot\target\release\vox.exe"
if (-not (Test-Path $BinaryPath)) {
    throw "Release binary not found at $BinaryPath"
}

$BinarySize = (Get-Item $BinaryPath).Length
$BinarySizeMB = [math]::Round($BinarySize / 1MB, 2)
Write-Host "  Binary size: $BinarySizeMB MB" -ForegroundColor Green

if ($BinarySizeMB -gt 15) {
    Write-Warning "Binary size $BinarySizeMB MB exceeds 15 MB budget (SC-007)"
}

# Step 2: Prepare output directory
Write-Host "`n[2/4] Preparing output directory..." -ForegroundColor Yellow
if (Test-Path $OutputDir) {
    Remove-Item -Recurse -Force $OutputDir
}
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

# Step 3: Compile WiX source to MSI
Write-Host "`n[3/4] Compiling WiX installer..." -ForegroundColor Yellow
$WxsPath = "$WixDir\main.wxs"
$IconPath = "$RepoRoot\assets\icons\app-icon.ico"

wix build $WxsPath `
    -d "BinaryPath=$BinaryPath" `
    -d "IconPath=$IconPath" `
    -o "$OutputDir\vox.msi"

if ($LASTEXITCODE -ne 0) {
    throw "WiX build failed with exit code $LASTEXITCODE"
}

# Step 4: Verify output
Write-Host "`n[4/4] Verifying output..." -ForegroundColor Yellow
$MsiPath = "$OutputDir\vox.msi"
if (-not (Test-Path $MsiPath)) {
    throw "MSI not found at $MsiPath"
}

$MsiSize = (Get-Item $MsiPath).Length
$MsiSizeMB = [math]::Round($MsiSize / 1MB, 2)

Write-Host "`n=== Build Complete ===" -ForegroundColor Green
Write-Host "  Binary: $BinarySizeMB MB"
Write-Host "  MSI:    $MsiSizeMB MB"
Write-Host "  Output: $MsiPath"
