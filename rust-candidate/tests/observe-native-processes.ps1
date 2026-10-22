param(
    [Parameter(Mandatory = $true)][ValidateRange(1, 2147483647)][int]$RootPid,
    [Parameter(Mandatory = $true)][ValidateRange(1, 2147483647)][int]$OwnerPid,
    [Parameter(Mandatory = $true)][string]$OutputFile,
    [Parameter(Mandatory = $true)][string]$StopFile
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$encoding = [System.Text.UTF8Encoding]::new($false)
$written = 0
$limit = 2MB
$started = [DateTime]::UtcNow
$tracked = @{}
function Test-SameBirth([DateTime]$Opened, [DateTime]$Snapshot) {
    # CIM_DATETIME carries six fractional digits (microseconds), unlike .NET's 100 ns ticks.
    $openedTicks = $Opened.ToUniversalTime().Ticks
    $snapshotTicks = $Snapshot.ToUniversalTime().Ticks
    return ($openedTicks - ($openedTicks % 10)) -eq ($snapshotTicks - ($snapshotTicks % 10))
}
[System.IO.File]::WriteAllText($OutputFile, '', $encoding)
$owner = Get-Process -Id $OwnerPid
$null = $owner.Handle
$root = Get-Process -Id $RootPid
$null = $root.Handle
$rootIdentity = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $RootPid" -Property ProcessId, ParentProcessId, CreationDate -OperationTimeoutSec 2
if ($owner.HasExited -or $rootIdentity.ParentProcessId -ne $OwnerPid -or
    $root.StartTime.ToUniversalTime() -lt $owner.StartTime.ToUniversalTime() -or
    -not (Test-SameBirth $root.StartTime $rootIdentity.CreationDate)) {
    $root.Dispose(); $owner.Dispose(); throw 'Driver identity does not belong to the live observer'
}
$tracked["root:$RootPid"] = @{ process = $root; parent = 0; name = $root.ProcessName; version = $null; ended = $false }

function Write-Observation([string]$Kind, [hashtable]$Fields) {
    $entry = @{ at = [DateTime]::UtcNow.ToString('o'); kind = $Kind }
    foreach ($key in $Fields.Keys) { $entry[$key] = $Fields[$key] }
    $line = ($entry | ConvertTo-Json -Compress -Depth 4) + "`n"
    $size = $encoding.GetByteCount($line)
    if ($script:written + $size -le $limit) {
        [System.IO.File]::AppendAllText($OutputFile, $line, $encoding)
        $script:written += $size
    }
}

function Sample-OwnedProcesses {
    # Query only the known live descendant frontier; never retain unrelated PIDs.
    $parents = [System.Collections.Generic.HashSet[int]]::new()
    $parentProcesses = @{}
    if (-not $root.HasExited) { $null = $parents.Add($RootPid) }
    foreach ($item in @($tracked.Values)) {
        try { if (-not $item.ended -and -not $item.process.HasExited) {
            $null = $parents.Add($item.process.Id)
            $parentProcesses[$item.process.Id] = $item.process
        } } catch { }
    }
    $queried = [System.Collections.Generic.HashSet[int]]::new()
    for ($depth = 0; $depth -lt 6; $depth++) {
        $frontier = @($parents | Where-Object { -not $queried.Contains($_) })
        if ($frontier.Count -eq 0) { break }
        if ($frontier.Count -gt 64) { throw 'Owned process frontier exceeds bound' }
        $filter = ($frontier | ForEach-Object { "ParentProcessId = $([int]$_)" }) -join ' OR '
        foreach ($parentId in $frontier) { $null = $queried.Add($parentId) }
        foreach ($child in @(Get-CimInstance -ClassName Win32_Process -Filter $filter -Property ProcessId, ParentProcessId, Name, CreationDate -OperationTimeoutSec 2)) {
            if (-not $parents.Contains([int]$child.ParentProcessId)) { continue }
            $parentProcess = $parentProcesses[[int]$child.ParentProcessId]
            if ($null -eq $parentProcess -or $parentProcess.HasExited -or
                $child.CreationDate.ToUniversalTime() -lt $parentProcess.StartTime.ToUniversalTime()) { continue }
            $identity = "$($child.ProcessId):$($child.CreationDate.ToUniversalTime().Ticks)"
            if ($tracked.ContainsKey($identity)) { continue }
            if ($tracked.Count -ge 128) { throw 'Owned process count exceeds bound' }
            try {
                $process = Get-Process -Id $child.ProcessId
                $null = $process.Handle # Retain this exact process object through exit/PID reuse.
                if (-not (Test-SameBirth $process.StartTime $child.CreationDate)) {
                    $process.Dispose()
                    Write-Observation 'process-identity-changed' @{ pid = [int]$child.ProcessId }
                    continue
                }
                $version = $null
                try { $version = (Get-Item -LiteralPath $process.Path).VersionInfo.FileVersion } catch { }
                $tracked[$identity] = @{ process = $process; parent = [int]$child.ParentProcessId;
                    name = [string]$child.Name; version = $version; ended = $false }
                $null = $parents.Add([int]$child.ProcessId)
                $parentProcesses[[int]$child.ProcessId] = $process
                Write-Observation 'owned-process-start' @{ pid = [int]$child.ProcessId;
                    parentPid = [int]$child.ParentProcessId; name = [string]$child.Name; fileVersion = $version }
            } catch { Write-Observation 'process-open-unavailable' @{ pid = [int]$child.ProcessId; errorType = $_.Exception.GetType().Name } }
        }
    }
    foreach ($identity in @($tracked.Keys)) {
        $item = $tracked[$identity]
        if ($item.ended) { continue }
        try {
            if ($item.process.HasExited) {
                Write-Observation 'owned-process-exit' @{ pid = $item.process.Id; name = $item.name; exitCode = $item.process.ExitCode }
                $item.ended = $true
                if ($item.process.Id -ne $RootPid) { $item.process.Dispose() }
            } else {
                $item.process.Refresh()
                Write-Observation 'owned-process-sample' @{ pid = $item.process.Id; parentPid = $item.parent; name = $item.name;
                    cpuMs = $item.process.TotalProcessorTime.TotalMilliseconds; workingBytes = $item.process.WorkingSet64;
                    privateBytes = $item.process.PrivateMemorySize64; threads = $item.process.Threads.Count }
            }
        } catch { Write-Observation 'process-sample-unavailable' @{ name = $item.name; errorType = $_.Exception.GetType().Name } }
    }
}

try {
    Write-Observation 'sampler-start' @{ rootPid = $RootPid; intervalSeconds = 10; maximumBytes = $limit }
    do {
        Sample-OwnedProcesses
        if ($written -ge ($limit - 1024)) { Write-Observation 'sampler-bound-reached' @{}; break }
        $next = [DateTime]::UtcNow.AddSeconds(10)
        while ([DateTime]::UtcNow -lt $next -and -not (Test-Path -LiteralPath $StopFile) -and -not $root.HasExited) {
            Start-Sleep -Milliseconds 250
        }
    } while (-not (Test-Path -LiteralPath $StopFile) -and -not $root.HasExited -and ([DateTime]::UtcNow - $started).TotalMinutes -lt 45)
    $rootExitCode = $null
    if ($root.HasExited) { $rootExitCode = $root.ExitCode }
    Write-Observation 'sampler-stop' @{ rootExited = $root.HasExited; rootExitCode = $rootExitCode; writtenBytes = $written }
} catch {
    Write-Observation 'sampler-error' @{ errorType = $_.Exception.GetType().Name }
    exit 1
} finally {
    foreach ($item in @($tracked.Values)) { try { $item.process.Dispose() } catch { } }
    $root.Dispose()
    $owner.Dispose()
}
