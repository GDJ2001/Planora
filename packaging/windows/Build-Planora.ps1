param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
Push-Location $root
try {
    if (git status --porcelain) {
        throw 'Commit source changes before building a release MSI.'
    }

    $metadata = (cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
    $version = ($metadata.packages | Where-Object { $_.name -eq 'planora-ui' } | Select-Object -First 1).version
    if (-not $version) { throw 'Could not determine Planora version' }
    # Windows Installer ignores prerelease text. 0.3.0-beta.1 intentionally
    # upgrades 0.2.0 while leaving room for later betas and 0.3.0 stable.
    $productVersionExpected = if ($version -match '^0\.3\.0-beta\.(\d+)$') {
        "0.2.$(1000 + [int]$Matches[1])"
    } else {
        ($version -split '-', 2)[0]
    }
    if ($productVersionExpected -notmatch '^\d+\.\d+\.\d+$') { throw "Invalid MSI numeric version: $productVersionExpected" }
    $commit = (git rev-parse --short HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine source commit' }

    if (-not $SkipBuild) {
        cargo build --release --bins --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    }
    $sourceDir = Join-Path $root 'target\release'
    $ui = Join-Path $sourceDir 'planora.exe'
    $helper = Join-Path $sourceDir 'planora-helper.exe'
    foreach ($binary in @($ui, $helper)) {
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
            throw "Release binary missing: $binary"
        }
    }

    $identity = (& $ui --build-info | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0 -or $identity.version -ne $version -or $identity.gitCommit -ne $commit) {
        throw "UI identity does not match source: version=$($identity.version), commit=$($identity.gitCommit); expected $version, $commit"
    }
    $helperIdentity = (& $helper --build-info | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0 -or $helperIdentity.version -ne $version -or
        $helperIdentity.gitCommit -ne $commit -or
        $helperIdentity.protocolVersion -ne $identity.protocolVersion -or
        $helperIdentity.target -ne $identity.target -or
        $helperIdentity.profile -ne $identity.profile) {
        throw "Helper identity does not match UI and source: version=$($helperIdentity.version), commit=$($helperIdentity.gitCommit), protocol=$($helperIdentity.protocolVersion)"
    }

    New-Item -ItemType Directory -Force -Path (Join-Path $root 'dist') | Out-Null
    $msi = Join-Path $root "dist\Planora-$version-x64.msi"
    wix build 'packaging/windows/Planora.wxs' -arch x64 `
        -d "PlanoraVersion=$productVersionExpected" -d "ProjectDir=$root" -d "SourceDir=$sourceDir" -o $msi
    if ($LASTEXITCODE -ne 0) { throw 'WiX build failed' }
    wix msi validate $msi
    if ($LASTEXITCODE -ne 0) { throw 'WiX validation failed' }

    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($msi, 0)
    $view = $database.OpenView("SELECT `Value` FROM `Property` WHERE `Property`='ProductVersion'")
    $view.Execute()
    $record = $view.Fetch()
    $productVersion = $record.StringData(1)
    $view.Close()
    if ($productVersion -ne $productVersionExpected) {
        throw "MSI ProductVersion $productVersion differs from expected $productVersionExpected"
    }

    $setup = Join-Path $root "dist\Planora-$version-x64-Setup.exe"
    wix build 'packaging/windows/Planora-Bundle.wxs' -arch x64 `
        -ext WixToolset.BootstrapperApplications.wixext `
        -d "BundleVersion=$productVersion" -d "ProjectDir=$root" -d "MsiPath=$msi" -o $setup
    if ($LASTEXITCODE -ne 0) { throw 'WiX Burn setup build failed' }

    $manifest = [pscustomobject]@{
        version = $version
        sourceCommit = $commit
        target = $identity.target
        profile = $identity.profile
        productVersion = $productVersion
        protocolVersion = $identity.protocolVersion
        binaries = @(
            [pscustomobject]@{ file = 'planora.exe'; sha256 = (Get-FileHash -LiteralPath $ui -Algorithm SHA256).Hash },
            [pscustomobject]@{ file = 'planora-helper.exe'; sha256 = (Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash }
        )
        msi = [pscustomobject]@{ file = [IO.Path]::GetFileName($msi); sha256 = (Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash }
        setup = [pscustomobject]@{ file = [IO.Path]::GetFileName($setup); sha256 = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash }
    }
    $manifestPath = Join-Path $root "dist\Planora-$version-x64.manifest.json"
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8
    Write-Host "Built $msi and $setup from $commit; manifest $manifestPath"
} finally {
    Pop-Location
}
