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
$key = 'HKLM:\SYSTEM\CurrentControlSet\Services\PlanoraHelper'
New-ItemProperty -LiteralPath $key -Name Environment -PropertyType MultiString -Value @("PLANORA_ALLOWED_SID=$UserSid") -Force | Out-Null
if ($service.Status -eq 'Running') { Stop-Service -Name 'PlanoraHelper' -ErrorAction Stop }
Start-Service -Name 'PlanoraHelper' -ErrorAction Stop
Write-Host "Planora helper authorized for $UserSid."
