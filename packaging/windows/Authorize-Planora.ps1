param([string]$UserSid)
$ErrorActionPreference = 'Stop'
if (-not $UserSid) { $UserSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value }
if ($UserSid -notmatch '^S-1-(\d+-)+\d+$') { throw 'Invalid user SID.' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"'), '-UserSid', $UserSid)
  $process = Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -Verb RunAs -Wait -PassThru
  exit $process.ExitCode
}
$service = Get-Service -Name 'PlanoraHelper' -ErrorAction Stop
$key = 'HKLM:\SOFTWARE\Planora'
# The MSI creates this key. New-Item -Force on an existing key also opens the
# HKLM\SOFTWARE parent for writing, which can fail even for an elevated caller.
if (-not (Test-Path -LiteralPath $key)) { New-Item -Path $key -Force | Out-Null }
New-ItemProperty -LiteralPath $key -Name AuthorizedSid -PropertyType String -Value $UserSid -Force | Out-Null
Remove-ItemProperty -LiteralPath $key -Name AuthorizedEnvironment -ErrorAction SilentlyContinue
Remove-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Services\PlanoraHelper' -Name Environment -ErrorAction SilentlyContinue
if ($service.Status -eq 'Running') { Stop-Service -Name 'PlanoraHelper' -ErrorAction Stop }
Start-Service -Name 'PlanoraHelper' -ErrorAction Stop
Write-Host "Planora helper authorized for $UserSid."
