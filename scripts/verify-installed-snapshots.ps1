param(
    [Parameter(Mandatory = $true)][ValidateRange(1, 100)][int]$ProtocolVersion,
    [ValidateRange(1, 1000)][int]$Count = 20,
    [switch]$IncludeSensorSummary
)

$ErrorActionPreference = 'Stop'
$pipe = [IO.Pipes.NamedPipeClientStream]::new('.', 'planora-v2', [IO.Pipes.PipeDirection]::InOut)

function Read-Exact([IO.Stream]$Stream, [int]$Length) {
    $buffer = [byte[]]::new($Length)
    $offset = 0
    while ($offset -lt $Length) {
        $pending = $Stream.BeginRead($buffer, $offset, $Length - $offset, $null, $null)
        if (-not $pending.AsyncWaitHandle.WaitOne(5000)) { throw 'Helper response timed out' }
        $read = $Stream.EndRead($pending)
        if ($read -le 0) { throw 'Helper closed the pipe during a response' }
        $offset += $read
    }
    return ,$buffer
}

function Exchange([IO.Stream]$Stream, [int]$Id, $Command) {
    $payload = [Text.Encoding]::UTF8.GetBytes((@{ request_id = $Id; command = $Command } | ConvertTo-Json -Compress -Depth 8))
    if ($payload.Length -gt 65536) { throw 'Request exceeds Planora IPC limit' }
    $length = [BitConverter]::GetBytes([uint32]$payload.Length)
    $Stream.Write($length, 0, 4)
    $Stream.Write($payload, 0, $payload.Length)
    $Stream.Flush()
    $header = Read-Exact $Stream 4
    $responseLength = [BitConverter]::ToUInt32($header, 0)
    if ($responseLength -gt 8MB) { throw 'Helper returned an oversized response' }
    $response = [Text.Encoding]::UTF8.GetString((Read-Exact $Stream $responseLength)) | ConvertFrom-Json
    if ($response.request_id -ne $Id) { throw 'Response request ID mismatch' }
    return $response.response
}

try {
    $pipe.Connect(2000)
    $hello = Exchange $pipe 1 @{ Hello = @{ version = $ProtocolVersion } }
    if ($hello.Hello.version -ne $ProtocolVersion -or -not $hello.Hello.helper_authorized) {
        throw 'Helper handshake did not confirm the expected protocol and authorization'
    }
    $sampling = Exchange $pipe 2 @{ SetSamplingInterval = @{ interval_ms = 100 } }
    if ($sampling.SamplingStatus.effective_ms -gt 100) { throw 'Helper did not apply the 100 ms sampling request' }
    $sequences = [Collections.Generic.HashSet[long]]::new()
    $maximumAgeMs = 0L
    $lastFrame = $null
    for ($index = 0; $index -lt $Count; $index++) {
        $frame = (Exchange $pipe ($index + 3) 'Snapshot').Snapshot
        if (-not $frame) { throw "Snapshot $index was not returned" }
        $age = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - [long]$frame.cpu_percent.sampled_at_ms
        if ($age -lt 0 -or $age -gt 2500) { throw "Snapshot $index CPU reading is stale or future-dated: ${age}ms" }
        $maximumAgeMs = [Math]::Max($maximumAgeMs, $age)
        [void]$sequences.Add([long]$frame.sequence)
        $lastFrame = $frame
        Start-Sleep -Milliseconds 180
    }
    if ($sequences.Count -lt $Count) { throw "Only $($sequences.Count) distinct snapshots arrived out of $Count" }
    $result = [ordered]@{ ProtocolVersion = $ProtocolVersion; FreshDistinctSnapshots = $sequences.Count; MaximumCpuAgeMs = $maximumAgeMs }
    if ($IncludeSensorSummary) {
        $sensors = @($lastFrame.sensors)
        $result.SensorEntries = $sensors.Count
        $result.AvailableSensors = @($sensors | Where-Object { $null -ne $_.value.value }).Count
        $result.AvailableCpuPackagePower = @($sensors | Where-Object { $_.sensor_type -eq 'CpuPackagePowerWatts' -and $null -ne $_.value.value }).Count
        $result.AvailableCpuTemperature = @($sensors | Where-Object { $_.sensor_type -eq 'CpuTemperatureCelsius' -and $null -ne $_.value.value }).Count
        $result.SensorProviders = @(($sensors | ForEach-Object { $_.provider } | Sort-Object -Unique))
    }
    [pscustomobject]$result
} finally {
    $pipe.Dispose()
}
