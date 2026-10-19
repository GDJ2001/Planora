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
}
'@
$systemDpi = [PlanoraDpi]::GetDpiForSystem()
$captures = [System.Collections.Generic.List[object]]::new()

Write-Host "Capturing native Planora $($buildInfo.version) ($($buildInfo.gitCommit))"
Write-Host "Binary: $BinPath"
Write-Host "SHA-256: $binHash"
Write-Host "System DPI: $systemDpi ($([math]::Round($systemDpi / 96 * 100))%)"

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

    $script:captures.Add([pscustomobject]@{
        file = $fileName
        view = $View
        theme = $Theme
        requestedLogicalSize = "${Width}x${Height}"
        actualPixelSize = "${actualWidth}x${actualHeight}"
        sha256 = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash
        automaticCheck = 'passed: new PNG, zero exit, decoded dimensions'
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
    Capture-NativeWindow -View 'overview' -Theme 'dark' -Width $res.W -Height $res.H
    Capture-NativeWindow -View 'overview' -Theme 'light' -Width $res.W -Height $res.H
}

foreach ($view in @('processes', 'hardware', 'benchmarks', 'capabilities', 'settings')) {
    Capture-NativeWindow -View $view -Theme 'dark' -Width 1180 -Height 790
    Capture-NativeWindow -View $view -Theme 'light' -Width 1180 -Height 790
}
Capture-NativeWindow -View 'processes' -Theme 'dark' -Width 1536 -Height 864
Capture-NativeWindow -View 'processes' -Theme 'light' -Width 1536 -Height 864

$manifest = [pscustomobject]@{
    capturedAt = (Get-Date).ToString('o')
    source = 'installed native eframe GPU framebuffer'
    binaryPath = $BinPath
    binarySha256 = $binHash
    build = $buildInfo
    windowsVersion = [System.Environment]::OSVersion.VersionString
    systemDpi = $systemDpi
    systemScalePercent = [math]::Round($systemDpi / 96 * 100)
    captures = $captures.ToArray()
}
$manifestPath = Join-Path $OutDir 'manifest.json'
$manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifestPath -Encoding utf8
Write-Host "Captured $($captures.Count) native images. Review each for clipping and overlap. Manifest: $manifestPath"
