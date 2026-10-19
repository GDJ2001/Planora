# scripts/capture_native.ps1
# Native Windows screenshot capture runner for Planora using built-in GPU framebuffer capture

param(
    [string]$OutDir = "$PSScriptRoot\..\docs\screenshots\native"
)

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force $OutDir | Out-Null

$binPath = "$PSScriptRoot\..\target\release\planora.exe"
if (-not (Test-Path $binPath)) {
    Write-Host "Building release binary..."
    cargo build --release --bin planora --locked
}

$binHash = (Get-FileHash $binPath -Algorithm SHA256).Hash

Write-Host "=========================================================="
Write-Host "Native Windows Smoke Test & GPU Screenshot Capture"
Write-Host "Binary: $binPath"
Write-Host "SHA-256: $binHash"
Write-Host "Output Directory: $OutDir"
Write-Host "=========================================================="

function Capture-NativeWindow {
    param(
        [string]$View,
        [string]$Theme,
        [int]$Width,
        [int]$Height
    )

    $outFileName = "native_${View}_${Theme}_${Width}x${Height}.png"
    $outFilePath = Join-Path $OutDir $outFileName

    Write-Host "Capturing Native Window: View=$View, Theme=$Theme, Size=${Width}x${Height}..."
    
    $proc = Start-Process $binPath -ArgumentList "--view $View --theme $Theme --width $Width --height $Height --screenshot-out `"$outFilePath`"" -PassThru -Wait

    if ((Test-Path $outFilePath) -and ((Get-Item $outFilePath).Length -gt 10000)) {
        $len = (Get-Item $outFilePath).Length
        Write-Host "  [PASS] Captured $outFileName ($len bytes, exit code $($proc.ExitCode))" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] Failed to capture $outFileName" -ForegroundColor Red
        throw "Failed to capture $outFileName"
    }
}

# 1. Resolutions matrix for Overview (both Dark and Light)
$resolutions = @(
    @{ W = 1024; H = 640 },
    @{ W = 1180; H = 790 },
    @{ W = 1366; H = 768 },
    @{ W = 1536; H = 864 },
    @{ W = 1920; H = 1080 }
)

foreach ($res in $resolutions) {
    Capture-NativeWindow -View "overview" -Theme "dark" -Width $res.W -Height $res.H
    Capture-NativeWindow -View "overview" -Theme "light" -Width $res.W -Height $res.H
}

# 2. View-specific matrix at 1180x790 (both Dark and Light)
$views = @("processes", "hardware", "benchmarks", "capabilities", "settings")

foreach ($v in $views) {
    Capture-NativeWindow -View $v -Theme "dark" -Width 1180 -Height 790
    Capture-NativeWindow -View $v -Theme "light" -Width 1180 -Height 790
}

# 3. Native 1536x864 verification for Processes (both Dark and Light)
Capture-NativeWindow -View "processes" -Theme "dark" -Width 1536 -Height 864
Capture-NativeWindow -View "processes" -Theme "light" -Width 1536 -Height 864

Write-Host "=========================================================="
Write-Host "All native screenshots captured and verified successfully!"
Write-Host "=========================================================="
