param(
    [string]$BinPath = "$PSScriptRoot\..\target\release\planora.exe",
    [string]$OutDir = "$PSScriptRoot\..\dist\overview-review",
    [switch]$AllPages,
    [switch]$ScaleMatrix
)
$ErrorActionPreference = 'Stop'
$BinPath = (Resolve-Path -LiteralPath $BinPath).Path
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path
Add-Type -AssemblyName System.Drawing
$identity = (& $BinPath --build-info | ConvertFrom-Json)
$hash = (Get-FileHash -LiteralPath $BinPath -Algorithm SHA256).Hash
$records = [Collections.Generic.List[object]]::new()
function Capture {
    param([string]$Page, [string]$Theme, [int]$Width, [int]$Height, [double]$Scale=0, [double]$TextScale=1, [int]$Scroll=0)
    $name = "$Page-$Theme-${Width}x$Height-ppp$Scale-text$TextScale-y$Scroll.png"
    $path = Join-Path $OutDir $name
    $arguments = '--native-fixture --view '+$Page+' --theme '+$Theme+' --width '+$Width+' --height '+$Height+' --auto-close-ms 15000 --screenshot-out "'+$path+'" --fixture-text-scale '+$TextScale+' --fixture-scroll '+$Scroll
    if ($Scale -gt 0) { $arguments += ' --fixture-scale '+$Scale }
    $process = Start-Process -FilePath $BinPath -ArgumentList $arguments -PassThru -Wait
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $path)) { throw "Capture failed: $name" }
    $image = [Drawing.Image]::FromFile($path)
    try { $pixels = "$($image.Width)x$($image.Height)" } finally { $image.Dispose() }
    $records.Add([pscustomobject]@{file=$name;page=$Page;theme=$Theme;requestedWindowLogicalSize="${Width}x$Height";pixelSize=$pixels;fixturePixelsPerPoint=$Scale;fixtureTextScale=$TextScale;scrollY=$Scroll;data='synthetic fixture';review='pending'})
    Write-Host $name
}
foreach ($theme in @('dark','light')) {
    foreach ($size in @(@(1024,640),@(1180,790),@(1366,768),@(1536,864),@(1920,1080))) { Capture overview $theme $size[0] $size[1] }
    Capture overview $theme 1024 640 0 2 650
    Capture overview $theme 1024 640 0 2 1100
    if ($ScaleMatrix) {
        foreach ($scale in @(1,1.25,1.5,2)) { Capture overview $theme 1180 790 $scale 1; Capture overview $theme 1180 790 $scale 1.5 450 }
    }
    if ($AllPages) {
        foreach ($page in @('performance','processes','history','hardware','benchmarks','capabilities','recordings','settings')) {
            Capture $page $theme 1180 790
            Capture $page $theme 1024 640
        }
    }
}
[pscustomobject]@{build=$identity;binary=$BinPath;sha256=$hash;scaleNote='Pixel-per-point overrides emulate UI scale in an isolated native fixture. They do not change Windows display settings or validate mixed-monitor transitions.';captures=$records} | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $OutDir 'manifest.json') -Encoding utf8
