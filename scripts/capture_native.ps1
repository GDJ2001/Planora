# Capture the installed native Windows app. Run after installing the final MSI.
param(
    [string]$OutDir = "$PSScriptRoot\..\docs\screenshots\native",
    [string]$BinPath = 'C:\Program Files\Planora\planora.exe'
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $BinPath -PathType Leaf)) {
    throw "Planora executable not found: $BinPath"
}
$BinPath = (Resolve-Path -LiteralPath $BinPath).Path
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path

$binHash = (Get-FileHash -LiteralPath $BinPath -Algorithm SHA256).Hash
$buildInfo = (& $BinPath --build-info | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0 -or -not $buildInfo.version -or -not $buildInfo.gitCommit) {
    throw "Could not read build identity from $BinPath"
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class PlanoraDpi {
    [DllImport("user32.dll")]
    public static extern uint GetDpiForSystem();
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);
}
'@
$systemDpi = [PlanoraDpi]::GetDpiForSystem()
$captures = [System.Collections.Generic.List[object]]::new()
$referenceScale = $null

Write-Host "Capturing native Planora $($buildInfo.version) ($($buildInfo.gitCommit))"
Write-Host "Binary: $BinPath"
Write-Host "SHA-256: $binHash"
Write-Host "System-reported DPI: $systemDpi; effective capture scaling is measured from each PNG"

function Capture-NativeWindow {
    param(
        [string]$View,
        [string]$Theme,
        [int]$Width,
        [int]$Height
    )

    $fileName = "native_${View}_${Theme}_${Width}x${Height}.png"
    $filePath = Join-Path $OutDir $fileName
    if (Test-Path -LiteralPath $filePath) {
        Remove-Item -LiteralPath $filePath -Force
    }

    $arguments = "--view $View --theme $Theme --width $Width --height $Height --auto-close-ms 30000 --screenshot-out `"$filePath`""
    [PlanoraDpi]::SetCursorPos(0, 0) | Out-Null
    $proc = Start-Process -FilePath $BinPath -ArgumentList $arguments -PassThru -Wait -WindowStyle Normal
    if ($proc.ExitCode -ne 0) {
        throw "Native capture failed ($fileName): process exit code $($proc.ExitCode)"
    }
    if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
        throw "Native capture failed ($fileName): no new PNG was written"
    }
    $file = Get-Item -LiteralPath $filePath
    if ($file.Length -lt 10000) {
        throw "Native capture failed ($fileName): PNG is only $($file.Length) bytes"
    }

    $image = [System.Drawing.Image]::FromFile($filePath)
    try {
        $actualWidth = $image.Width
        $actualHeight = $image.Height
    } finally {
        $image.Dispose()
    }
    if ($actualWidth -lt 100 -or $actualHeight -lt 100) {
        throw "Native capture failed ($fileName): implausible dimensions ${actualWidth}x${actualHeight}"
    }
    $scaleX = $actualWidth / $Width
    $scaleY = $actualHeight / $Height
    if ([math]::Abs($scaleX - $scaleY) -gt 0.02 * (($scaleX + $scaleY) / 2)) {
        throw "Native capture failed ($fileName): requested ${Width}x${Height}, captured ${actualWidth}x${actualHeight}; width/height scales disagree"
    }
    if ($null -eq $script:referenceScale) {
        $script:referenceScale = ($scaleX + $scaleY) / 2
    } elseif ([math]::Abs($scaleX - $script:referenceScale) -gt 0.03 * $script:referenceScale) {
        throw "Native capture failed ($fileName): capture scale changed from $($script:referenceScale) to $scaleX; requested size may have been overridden"
    }

    $script:captures.Add([pscustomobject]@{
        file = $fileName
        view = $View
        theme = $Theme
        requestedLogicalSize = "${Width}x${Height}"
        actualPixelSize = "${actualWidth}x${actualHeight}"
        estimatedPixelScale = [math]::Round(($scaleX + $scaleY) / 2, 3)
        sha256 = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash
        automaticCheck = 'passed: new PNG, zero exit, decoded dimensions, requested-size scaling'
        visualReview = 'pending'
    })
    Write-Host "  ${fileName}: ${actualWidth}x${actualHeight} pixels"
}

$resolutions = @(
    @{ W = 1024; H = 640 },
    @{ W = 1180; H = 790 },
    @{ W = 1366; H = 768 },
    @{ W = 1536; H = 864 },
    @{ W = 1920; H = 1080 }
)
foreach ($res in $resolutions) {
    foreach ($view in @('overview', 'processes', 'hardware', 'benchmarks', 'capabilities', 'recordings', 'settings')) {
        Capture-NativeWindow -View $view -Theme 'dark' -Width $res.W -Height $res.H
        Capture-NativeWindow -View $view -Theme 'light' -Width $res.W -Height $res.H
    }
}

$manifest = [pscustomobject]@{
    capturedAt = (Get-Date).ToString('o')
    source = 'installed native eframe GPU framebuffer'
    binaryPath = $BinPath
    binarySha256 = $binHash
    build = $buildInfo
    windowsVersion = [System.Environment]::OSVersion.VersionString
    systemReportedDpi = $systemDpi
    systemReportedScalePercent = [math]::Round($systemDpi / 96 * 100)
    effectiveCaptureScalePercent = [math]::Round($referenceScale * 100)
    captures = $captures.ToArray()
}
$manifestPath = Join-Path $OutDir 'manifest.json'
$manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifestPath -Encoding utf8
Write-Host "Captured $($captures.Count) native images. Review each for clipping and overlap. Manifest: $manifestPath"
