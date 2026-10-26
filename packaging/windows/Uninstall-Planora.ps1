$ErrorActionPreference = 'Stop'

# Prefer Burn's registered maintenance UI. The MSI entry is the fallback for
# administrative deployments that did not use Planora Setup.
$roots = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)
$entries = foreach ($root in $roots) {
    Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue | ForEach-Object {
        Get-ItemProperty -LiteralPath $_.PSPath -ErrorAction SilentlyContinue
    }
}
$entry = $entries |
    Where-Object { $_.DisplayName -eq 'Planora' -and ($_.ModifyPath -or $_.UninstallString) } |
    Sort-Object @{ Expression = { [bool]$_.ModifyPath }; Descending = $true } |
    Select-Object -First 1
if (-not $entry) { throw 'Planora is not registered in Windows Installed Apps.' }
$command = if ($entry.ModifyPath) { $entry.ModifyPath } else { $entry.UninstallString }
if ($command -notmatch '^\s*(?:"([^"]+)"|([^\s]+))(.*)$') {
    throw 'The registered Planora maintenance command is invalid.'
}
$exe = if ($Matches[1]) { $Matches[1] } else { $Matches[2] }
$arguments = $Matches[3].Trim()
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "The registered Planora maintenance program is missing: $exe"
}
Start-Process -FilePath $exe -ArgumentList $arguments
