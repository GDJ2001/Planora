param(
    [ValidateRange(30, 14400)][int]$DurationSeconds = 3600,
    [ValidateRange(1, 60)][int]$IntervalSeconds = 10,
    [string]$OutputPath = "dist/process-growth-windows.csv"
)

$ErrorActionPreference = 'Stop'
$targetNames = @('planora', 'planora-helper', 'planora-sensor-broker')
$outputFile = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $OutputPath))
$outputDirectory = Split-Path -Parent $outputFile
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null

$started = [System.Diagnostics.Stopwatch]::StartNew()
$samples = [System.Collections.Generic.List[object]]::new()
while ($started.Elapsed.TotalSeconds -lt $DurationSeconds) {
    $elapsed = [Math]::Round($started.Elapsed.TotalSeconds, 3)
    # PerfProc exposes service and GUI-session counters even when Get-Process
    # cannot open the GUI process from this automation session.
    $processes = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process |
        Where-Object { $_.Name -match '^planora(?:#\d+|-helper|-sensor-broker)?$' })
    foreach ($process in $processes) {
        $name = $process.Name -replace '#\d+$', ''
        if ($name -notin $targetNames) { continue }
        $samples.Add([pscustomobject]@{
            elapsed_seconds = $elapsed
            name = $name
            pid = $process.IDProcess
            cpu_percent = $process.PercentProcessorTime
            working_set_bytes = $process.WorkingSet
            private_bytes = $process.PrivateBytes
            handles = $process.HandleCount
            threads = $process.ThreadCount
        })
    }
    Start-Sleep -Seconds $IntervalSeconds
}

$samples | Export-Csv -LiteralPath $outputFile -NoTypeInformation -Encoding utf8
$summary = foreach ($group in ($samples | Group-Object name, pid)) {
    $ordered = @($group.Group | Sort-Object elapsed_seconds)
    $first = $ordered[0]
    $last = $ordered[-1]
    [pscustomobject]@{
        name = $first.name
        pid = $first.pid
        samples = $ordered.Count
        duration_seconds = [Math]::Round($last.elapsed_seconds - $first.elapsed_seconds, 1)
        cpu_percent_mean = [Math]::Round(($ordered | Measure-Object cpu_percent -Average).Average, 3)
        working_set_bytes_delta = $last.working_set_bytes - $first.working_set_bytes
        private_bytes_delta = $last.private_bytes - $first.private_bytes
        handles_delta = $last.handles - $first.handles
        threads_delta = $last.threads - $first.threads
        peak_working_set_bytes = ($ordered | Measure-Object working_set_bytes -Maximum).Maximum
        peak_handles = ($ordered | Measure-Object handles -Maximum).Maximum
    }
}
$summaryPath = [System.IO.Path]::ChangeExtension($outputFile, '.json')
$summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $summaryPath -Encoding utf8
Write-Output "Samples: $outputFile"
Write-Output "Summary: $summaryPath"
$summary | Format-Table -AutoSize
