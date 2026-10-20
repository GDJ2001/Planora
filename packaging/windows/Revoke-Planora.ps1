$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  $process = Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"')) -Verb RunAs -Wait -PassThru
  exit $process.ExitCode
}
$service = Get-Service -Name 'PlanoraHelper' -ErrorAction Stop
if ($service.Status -eq 'Running') { Stop-Service -Name 'PlanoraHelper' -ErrorAction Stop }
$key = 'HKLM:\SOFTWARE\Planora'
Remove-ItemProperty -LiteralPath $key -Name AuthorizedSid -ErrorAction SilentlyContinue
Remove-ItemProperty -LiteralPath $key -Name AuthorizedEnvironment -ErrorAction SilentlyContinue
Remove-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Services\PlanoraHelper' -Name Environment -ErrorAction SilentlyContinue
Write-Host 'Planora helper authorization revoked. Reauthorize from the application to monitor again.'
