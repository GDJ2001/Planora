param(
    [Parameter(Mandatory = $true)][ValidateRange(1, 100)][int]$ProtocolVersion,
    [ValidateRange(1, 15)][int]$DurationSeconds = 3,
    [switch]$Cancel
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
    $jobId = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $start = Exchange $pipe 2 @{ StartProfile = @{ job_id = $jobId; target = @{ DiskIo = @{ target_process = $null; duration_seconds = $DurationSeconds } } } }
    if (-not $start.ProfileStatus.Running) { throw "Profile did not start: $($start | ConvertTo-Json -Compress -Depth 5)" }

    $requestId = 3
    if ($Cancel) {
        Start-Sleep -Milliseconds 500
        $cancelResponse = Exchange $pipe $requestId @{ CancelProfile = @{ job_id = $jobId } }
        if (-not $cancelResponse.ProfileStatus.Cancelling) {
            throw "Profile cancellation was not acknowledged: $($cancelResponse | ConvertTo-Json -Compress -Depth 5)"
        }
        $requestId++
    }

    $deadline = [DateTime]::UtcNow.AddSeconds($DurationSeconds + 10)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
        $status = Exchange $pipe $requestId 'ProfileStatus'
        $requestId++
        if ($status.ProfileStatus.Completed) {
            if ($Cancel) { throw 'Profile completed despite acknowledged cancellation' }
            $result = $status.ProfileStatus.Completed.result
            [pscustomobject]@{
                DurationSeconds = $result.duration_seconds
                ElapsedMs = $result.elapsed_ms
                EventsTraced = $result.events_traced
                EventsLost = $result.events_lost
                AttributionCoveragePercent = $result.attribution_coverage_percent
                ProcessRecords = @($result.disk_records).Count
                UnattributedOperations = $result.unattributed_operations
            }
            return
        }
        if ($status.ProfileStatus.Failed) {
            throw "Profile failed: $($status.ProfileStatus.Failed | ConvertTo-Json -Compress -Depth 5)"
        }
        if ($status.ProfileStatus.Cancelled) {
            if ($Cancel) {
                [pscustomobject]@{ Cancelled = $true; JobId = $jobId }
                return
            }
            throw 'Profile was cancelled unexpectedly'
        }
    }
    [void](Exchange $pipe $requestId @{ CancelProfile = @{ job_id = $jobId } })
    throw 'Profile did not complete within its bounded deadline'
} finally {
    $pipe.Dispose()
}
