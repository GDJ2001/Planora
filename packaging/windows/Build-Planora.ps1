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
    # Windows Installer ignores prerelease text. Beta builds use an ordered
    # numeric version below 0.3.0 while leaving room for later betas.
    $productVersionExpected = if ($version -match '^0\.3\.0-beta\.(\d+)$') {
        "0.2.$(1000 + [int]$Matches[1])"
    } else {
        ($version -split '-', 2)[0]
    }
    if ($productVersionExpected -notmatch '^\d+\.\d+\.\d+$') { throw "Invalid MSI numeric version: $productVersionExpected" }
    $commit = (git rev-parse --short HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine source commit' }

    $sourceDir = Join-Path $root 'target\release'
    $ui = Join-Path $sourceDir 'planora.exe'
    $helper = Join-Path $sourceDir 'planora-helper.exe'
    $broker = Join-Path $sourceDir 'planora-sensor-broker.exe'

    if (-not $SkipBuild) {
        cargo build --release --bins --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }

        dotnet publish (Join-Path $root 'crates\planora-sensor-broker\PlanoraSensorBroker.csproj') -c Release -r win-x64 --self-contained -p:PlanoraGitCommit=$commit -o $sourceDir
        if ($LASTEXITCODE -ne 0) { throw 'Sensor broker publish failed' }
    }

    foreach ($binary in @($ui, $helper, $broker)) {
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
            throw "Release binary missing: $binary"
        }
    }
    $brokerIdentity = (& $broker --build-info | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0 -or $brokerIdentity.version -ne $version -or $brokerIdentity.build_commit -ne $commit) {
        throw "Sensor broker identity does not match source: version=$($brokerIdentity.version), commit=$($brokerIdentity.build_commit); expected $version, $commit"
    }

    $pawnIoInstaller = Join-Path $root 'packaging\windows\deps\PawnIO_setup.exe'
    if (-not (Test-Path -LiteralPath $pawnIoInstaller -PathType Leaf)) {
        throw "PawnIO installer missing: $pawnIoInstaller"
    }
    $pawnIoHash = (Get-FileHash -LiteralPath $pawnIoInstaller -Algorithm SHA256).Hash.ToLowerInvariant()
    $expectedPawnIoHash = '1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032'
    if ($pawnIoHash -ne $expectedPawnIoHash) {
        throw "PawnIO installer hash mismatch: expected $expectedPawnIoHash, got $pawnIoHash"
    }
    $pawnIoSignature = Get-AuthenticodeSignature -LiteralPath $pawnIoInstaller
    if ($pawnIoSignature.Status -ne 'Valid' -or $pawnIoSignature.SignerCertificate.Subject -notmatch 'namazso') {
        throw "PawnIO installer signature is not valid and issued to namazso: $($pawnIoSignature.Status)"
    }
    $uiBytes = [IO.File]::ReadAllBytes($ui)
    $peOffset = [BitConverter]::ToInt32($uiBytes, 0x3c)
    if ($peOffset -lt 0 -or $peOffset + 94 -ge $uiBytes.Length -or
        [BitConverter]::ToUInt16($uiBytes, $peOffset + 24 + 68) -ne 2) {
        throw 'Planora UI must be a Windows GUI executable (PE subsystem 2).'
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
        -ext WixToolset.BootstrapperApplications.wixext -ext WixToolset.Util.wixext `
        -d "BundleVersion=$productVersion" -d "AppVersion=$version" -d "ProjectDir=$root" -d "MsiPath=$msi" -o $setup
    if ($LASTEXITCODE -ne 0) { throw 'WiX Burn setup build failed' }

    $manifest = [pscustomobject]@{
        version = $version
        sourceCommit = $commit
        target = $identity.target
        profile = $identity.profile
        productVersion = $productVersion
        protocolVersion = $identity.protocolVersion
        uiSubsystem = 'Windows GUI'
        binaries = @(
            [pscustomobject]@{ file = 'planora.exe'; sha256 = (Get-FileHash -LiteralPath $ui -Algorithm SHA256).Hash },
            [pscustomobject]@{ file = 'planora-helper.exe'; sha256 = (Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash },
            [pscustomobject]@{ file = 'planora-sensor-broker.exe'; sha256 = (Get-FileHash -LiteralPath $broker -Algorithm SHA256).Hash },
            [pscustomobject]@{ file = 'PawnIO_setup.exe'; sha256 = (Get-FileHash -LiteralPath $pawnIoInstaller -Algorithm SHA256).Hash }
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
