# Holler demo build -- stop what scripts\demo-up.ps1 started, AND NOTHING ELSE.
#
#   .\scripts\demo-down.ps1            # stop this machine's recorded demo stack
#   .\scripts\demo-down.ps1 -WhatIf    # say what it would stop, kill nothing
#
# DEVELOPMENT / DEMO ONLY (ADR-013).
#
# ---------------------------------------------------------------------------
# THE RULE THIS SCRIPT EXISTS TO ENFORCE: A PID IS NOT AN IDENTITY.
#
# Windows reuses pids, and "the port answers" says nothing about WHICH process
# answers -- the old one answers identically. A test displaced the operator's
# running backend three times in one day, and on the sweep that followed, three
# of the four listeners found on the demo ports were the operator's own, started
# minutes earlier. Every one of those runs was already aimed at a scratch
# directory and a scratch database, and still took down the live stack.
#
# So this script kills NOTHING it cannot identify:
#
#   - it works from the list demo-up recorded (pid + process name + START TIME),
#   - it re-reads each pid and REFUSES it when the start time has moved, because
#     that is a different process wearing a recycled pid,
#   - it refuses a pid whose name no longer matches what was recorded,
#   - and it never sweeps a port. A listener demo-up did not record is REPORTED,
#     with its start time, and left running.
#
# -Port <n> exists for the one case the record cannot cover: demo-up died before
# it could record something. It still refuses to kill a listener that started
# BEFORE the recorded run began -- that one is provably not this run's.
# ---------------------------------------------------------------------------
#
# GAP A6, AND IT IS PRINTED ON EVERY RUN RATHER THAN FILED HERE. No exit path on
# this build fires RunEvent::Exit -- neither a window close nor Ctrl+C -- so
# stopping the POS leaves a PLAINTEXT edge.db beside the sealed .enc, and the
# .enc is only as current as the last successful seal. That is why no
# trustworthy backup can be taken before a risky migration. The plaintext
# leftover is NOT a recovery route and must never be offered as one: it is the
# data-loss bug itself (operator's ruling). The fix is a reset, not a copy.

[CmdletBinding()]
param(
    # Written by demo-up.ps1. A parameter so a scratch run can be torn down
    # without touching the real record.
    [string]$StateFile = (Join-Path $env:LOCALAPPDATA "Holler\demo-up-state.json"),

    # Extra ports to consider, for the case where demo-up died before recording
    # a process. A listener on one of these is stopped ONLY if it started after
    # the recorded run began; anything older is reported and left alone.
    [int[]]$Port = @(),

    # Report and stop nothing.
    [switch]$WhatIf,

    # Leave the state file in place after a successful teardown. By default it
    # is removed, so a later demo-down cannot act on a record that no longer
    # describes anything running.
    [switch]$KeepState
)

$ErrorActionPreference = "Stop"

# Dot-sourced, so the guard file being missing stops the script rather than
# silently disabling the control.
. (Join-Path $PSScriptRoot "agent-guard.ps1")

# THE OPERATOR'S LIVE PORTS ARE UNREACHABLE FROM AN AGENT SHELL, STRUCTURALLY.
# Not "an agent should not" -- an agent CANNOT. The record is the only thing
# naming what gets stopped, and a record is a file an agent could write, so
# refusing on the state file's path alone would leave a hole you could drive the
# till through. Two conditions, both required:
#
#   - the state file must be named explicitly and sit outside the real one's
#     directory, and
#   - nothing that owns one of the live demo ports may be stopped, whatever the
#     record says about it.
#
# Together these leave the kill path testable against scratch processes on
# scratch ports -- which is the only way to know it works -- while making the
# operator's running stack unreachable. A human shell has no CLAUDECODE and is
# unaffected.
$script:LiveDemoPorts = @(8080, 9310, 9320, 5173, 5174, 5175)
$script:AgentShell = Test-IsAgentShell
if ($script:AgentShell -and (-not $WhatIf)) {
    $realStateDir = Join-Path $env:LOCALAPPDATA "Holler"
    if (-not $PSBoundParameters.ContainsKey("StateFile")) {
        Deny-AgentRun -ScriptName "demo-down.ps1" `
            -Problem "-StateFile was not given, so it would default to the operator's own record at $StateFile." `
            -WhatToDo "Pass -StateFile explicitly, outside $realStateDir, or ask the operator to run this. -WhatIf reports what it would stop and touches nothing."
    }
    if (Test-PathIsInside -Path $StateFile -Root $realStateDir) {
        Deny-AgentRun -ScriptName "demo-down.ps1" `
            -Problem "-StateFile '$StateFile' is inside $realStateDir, which is the operator's own record." `
            -WhatToDo "Point it at a scratch file, e.g. `$env:TEMP\holler-scratch\demo-up-state.json."
    }
    Write-Host "agent-guard: agent shell detected; live-port processes ($($script:LiveDemoPorts -join ', ')) will be refused" -ForegroundColor DarkGray
}

# Pids that own a live demo port right now, resolved ONCE so the refusal below
# cannot be raced by something binding a port mid-teardown.
$script:LivePortOwners = @{}
if ($script:AgentShell) {
    foreach ($lp in $script:LiveDemoPorts) {
        $c = Get-NetTCPConnection -LocalPort $lp -State Listen -ErrorAction SilentlyContinue
        if ($c) { $script:LivePortOwners[[int]($c | Select-Object -First 1 -ExpandProperty OwningProcess)] = $lp }
    }
}

function Write-Note($msg) { Write-Host "  $msg" -ForegroundColor DarkGray }
function Write-Ok($msg)   { Write-Host "  OK      $msg" -ForegroundColor Green }
function Write-Kept($msg) { Write-Host "  KEPT    $msg" -ForegroundColor Yellow }
function Write-Gone($msg) { Write-Host "  GONE    $msg" -ForegroundColor DarkGray }

Write-Host ""
Write-Host "Holler demo stack -- shutting down" -ForegroundColor Green
Write-Host "record: $StateFile"
if ($WhatIf) { Write-Host "-WhatIf: reporting only. Nothing will be stopped." -ForegroundColor Yellow }
Write-Host ""

if (-not (Test-Path $StateFile)) {
    Write-Host "No record at $StateFile." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Nothing was stopped, deliberately: without the record there is no way to tell" -ForegroundColor Yellow
    Write-Host "a process demo-up started from one of yours, and a port that answers proves" -ForegroundColor Yellow
    Write-Host "neither. List what is listening and decide by START TIME:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  foreach (`$p in 8080,9310,9320,5173,5174,5175) {" -ForegroundColor DarkGray
    Write-Host "    Get-NetTCPConnection -LocalPort `$p -State Listen -ErrorAction SilentlyContinue |" -ForegroundColor DarkGray
    Write-Host "      ForEach-Object { Get-Process -Id `$_.OwningProcess } |" -ForegroundColor DarkGray
    Write-Host "      Select-Object Id, ProcessName, StartTime }" -ForegroundColor DarkGray
    Write-Host ""
    exit 1
}

try {
    $state = Get-Content $StateFile -Raw | ConvertFrom-Json
} catch {
    Write-Host "FAILED: $StateFile is not readable JSON ($($_.Exception.Message))." -ForegroundColor Red
    Write-Host "NEXT ACTION: delete it and stop the demo processes by hand, deciding by start time." -ForegroundColor Yellow
    exit 1
}

$runStartedAt = $null
if ($state.started_at) {
    try { $runStartedAt = [DateTime]::Parse($state.started_at) } catch { $runStartedAt = $null }
}
if ($runStartedAt) { Write-Note "the recorded run began $($runStartedAt.ToString('yyyy-MM-dd HH:mm:ss'))" }

# Decides whether one recorded entry may be stopped. Returns the live process
# object when it is provably the same one, and a REASON string when it is not.
function Resolve-RecordedProcess($entry) {
    $proc = Get-Process -Id $entry.pid -ErrorAction SilentlyContinue
    if (-not $proc) { return @{ Proc = $null; Reason = "already gone" } }

    if ($script:LivePortOwners.ContainsKey([int]$entry.pid)) {
        $livePort = $script:LivePortOwners[[int]$entry.pid]
        return @{ Proc = $null; Reason = "pid $($entry.pid) owns LIVE port $livePort and this is an agent shell -- refused whatever the record says" }
    }

    if ($entry.name -and ($proc.ProcessName -ne $entry.name)) {
        return @{ Proc = $null; Reason = "pid $($entry.pid) is now '$($proc.ProcessName)', recorded as '$($entry.name)' -- a recycled pid, NOT this run's process" }
    }

    # THE CHECK THAT MATTERS. A pid that has been reused reports a different
    # start time, and that is the only thing distinguishing "the till demo-up
    # launched" from "whatever took its pid afterwards".
    $liveStart = $null
    try { $liveStart = $proc.StartTime } catch { $liveStart = $null }
    if ($entry.started_at) {
        $recordedStart = $null
        try { $recordedStart = [DateTime]::Parse($entry.started_at) } catch { $recordedStart = $null }
        if ($recordedStart -and $liveStart) {
            # One second of tolerance: the recorded value is round-tripped
            # through JSON and back, and an exact tick compare would refuse
            # every entry over a formatting detail.
            if ([Math]::Abs(($liveStart - $recordedStart).TotalSeconds) -gt 1) {
                return @{ Proc = $null; Reason = "pid $($entry.pid) started $($liveStart.ToString('yyyy-MM-dd HH:mm:ss')), recorded as $($recordedStart.ToString('yyyy-MM-dd HH:mm:ss')) -- a recycled pid, NOT this run's process" }
            }
        } elseif (-not $liveStart) {
            return @{ Proc = $null; Reason = "pid $($entry.pid) will not report its start time (access denied), so it cannot be identified" }
        }
    } elseif ($liveStart -and $runStartedAt -and ($liveStart -lt $runStartedAt)) {
        # No start time was recorded for this entry, but the process predates
        # the run that supposedly started it. It is not this run's.
        return @{ Proc = $null; Reason = "pid $($entry.pid) started $($liveStart.ToString('yyyy-MM-dd HH:mm:ss')), BEFORE the recorded run began -- not this run's process" }
    }

    return @{ Proc = $proc; Reason = $null }
}

# Order matters: the POS first, so the till releases the edge database, 9310 and
# 9320 before anything else is torn down around it; then the web servers; then
# the backend, which is the one thing the till talks to and the last thing worth
# keeping alive. The windows that launched them go last -- killing a wrapper
# first orphans its child.
$roleOrder = @{ "pos" = 0; "pos-vite" = 1; "kds" = 2; "backend" = 3; "pos-window" = 4; "kds-window" = 5; "backend-window" = 6 }
$entries = @($state.processes) | Sort-Object -Property @{ Expression = {
    if ($roleOrder.ContainsKey($_.role)) { $roleOrder[$_.role] } else { 99 }
} }

$stopped = 0
$kept = 0
foreach ($entry in $entries) {
    $label = "$($entry.role) -- $($entry.name) pid $($entry.pid)"
    if ($entry.note) { $label = "$label ($($entry.note))" }

    $resolved = Resolve-RecordedProcess $entry
    if (-not $resolved.Proc) {
        if ($resolved.Reason -eq "already gone") {
            Write-Gone $label
        } else {
            Write-Kept "$label`n          $($resolved.Reason)"
            $kept++
        }
        continue
    }

    if ($WhatIf) {
        Write-Note "would stop $label"
        continue
    }
    try {
        Stop-Process -Id $entry.pid -Force -ErrorAction Stop
        Write-Ok "stopped $label"
        $stopped++
        if ($entry.role -eq "pos") { $script:PosWasStopped = $true }
    } catch {
        Write-Kept "$label`n          could not stop it: $($_.Exception.Message)"
        $kept++
    }
}

# -Port: the escape hatch for a process demo-up never got to record. It still
# refuses anything older than the run.
foreach ($p in ($Port | Sort-Object -Unique)) {
    $conn = Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue
    if (-not $conn) { Write-Gone "port ${p}: nothing listening"; continue }
    $ownerPid = $conn | Select-Object -First 1 -ExpandProperty OwningProcess
    if ($script:LivePortOwners.ContainsKey([int]$ownerPid)) {
        Write-Kept "port ${p}: pid $ownerPid owns LIVE port $($script:LivePortOwners[[int]$ownerPid]) and this is an agent shell -- refused."
        $kept++
        continue
    }
    $proc = Get-Process -Id $ownerPid -ErrorAction SilentlyContinue
    if (-not $proc) { Write-Gone "port ${p}: pid $ownerPid is gone"; continue }
    $liveStart = $null
    try { $liveStart = $proc.StartTime } catch { $liveStart = $null }
    $when = if ($liveStart) { $liveStart.ToString('yyyy-MM-dd HH:mm:ss') } else { "unknown start time" }
    if ((-not $liveStart) -or (-not $runStartedAt) -or ($liveStart -lt $runStartedAt)) {
        Write-Kept "port ${p}: $($proc.ProcessName) pid $ownerPid started $when`n          it predates the recorded run (or will not say), so it is NOT this run's. Left alone."
        $kept++
        continue
    }
    if ($WhatIf) { Write-Note "would stop port ${p}: $($proc.ProcessName) pid $ownerPid started $when"; continue }
    try {
        Stop-Process -Id $ownerPid -Force -ErrorAction Stop
        Write-Ok "stopped port ${p}: $($proc.ProcessName) pid $ownerPid"
        $stopped++
    } catch {
        Write-Kept "port ${p}: could not stop pid $ownerPid -- $($_.Exception.Message)"
        $kept++
    }
}

Write-Host ""
if ($WhatIf) {
    Write-Host "-WhatIf: nothing was stopped." -ForegroundColor Yellow
} else {
    Write-Host "stopped $stopped process(es); $kept left running." -ForegroundColor Green
    if (-not $KeepState) {
        Remove-Item $StateFile -ErrorAction SilentlyContinue
        Write-Note "removed $StateFile so a later run cannot act on a stale record"
    }
}

Write-Host ""
Write-Host "Docker containers are left running on purpose -- 'make down' stops them." -ForegroundColor DarkGray

# Printed only when a till was ACTUALLY stopped. A warning that fires on every
# run, including runs that stopped nothing, is a warning that stops being read --
# and this one has to be read.
if ($script:PosWasStopped) {
    Write-Host ""
    Write-Host "GAP A6: stopping the POS did NOT seal the edge database. No exit path on this" -ForegroundColor Yellow
    Write-Host "build fires RunEvent::Exit, so a plaintext edge.db is now sitting beside the" -ForegroundColor Yellow
    Write-Host "sealed .enc, and the .enc is only as current as the last successful seal." -ForegroundColor Yellow
    Write-Host "That leftover is NOT a recovery route -- it is the data-loss bug itself. The next" -ForegroundColor Yellow
    Write-Host "'demo-up -Fresh' clears both and reseeds." -ForegroundColor Yellow
}
exit 0
