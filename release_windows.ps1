# LibreCard Build and Package Script
# I fucking hate Microsoft. I didn't, am not, and won't bother to learn Powershell
# This script is generated by Claude. Thanks.

# 1. Set the correct target triple for Windows x64 MSVC
$targetTriple = "x86_64-pc-windows-msvc"

# Get the short git hash
$gitHash = git rev-parse --short HEAD
$packageName = "librecard-win_amd64-$gitHash"
$packageDir = "target/$packageName"

# Create the package directory if it doesn't exist
if (!(Test-Path $packageDir)) {
    New-Item -ItemType Directory -Path $packageDir -Force
}

# 1. Build the release binary
Write-Host "Building release binary for $targetTriple..." -ForegroundColor Cyan
cargo build --release --target $targetTriple

# 2. Copy the executable to the package directory
$exePath = "target/$targetTriple/release/librecard.exe"
if (Test-Path $exePath) {
    Write-Host "Copying executable to package directory..." -ForegroundColor Cyan
    Copy-Item $exePath -Destination $packageDir
} else {
    Write-Host "Error: Executable not found at $exePath" -ForegroundColor Red
    exit 1
}

# 3. Copy LICENSE and README.md with .txt extensions
Write-Host "Copying documentation files..." -ForegroundColor Cyan
Copy-Item "LICENSE" -Destination "$packageDir/LICENSE.txt"
Copy-Item "README.md" -Destination "$packageDir/README.txt"

# 4. Create zip archive
Write-Host "Creating zip archive..." -ForegroundColor Cyan
$zipPath = "target/$packageName.zip"
Compress-Archive -Path $packageDir/* -DestinationPath $zipPath -Force

# Completion message
Write-Host "Package created successfully at $zipPath" -ForegroundColor Green

