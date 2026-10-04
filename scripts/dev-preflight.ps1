#Requires -Version 5.1
<#
.SYNOPSIS
    Read-only diagnosis of the local Holler dev stack. Prints what is wrong and
    the exact command that fixes it. Changes nothing.

.DESCRIPTION
    WHY THIS EXISTS

    Bringing the stack up by hand costs a surprising amount of time, and the
    cost is never in the interesting part. It is spent on the same small set of
    states, each of which LOOKS like something else:

      * A foreign container holds 5432, so Holler's postgres cannot bind and
        the backend connects to a stranger's database.
      * The backend answers on 8080 while the process you started is gone --
        a different one is serving. The port answering is not evidence.
      * A `holler-pos` process outlives its own window (gap A6). It keeps
        9310 and 9320 and keeps the edge database open, so the next launch
        fails in a way that reads as a code fault. This is the "stale window"
        case: the process is alive, the window is not, and nothing says so.
      * HOLLER_DB_KEY_HEX lives in the process environment, so it dies with
        the terminal. The bootstrap then refuses -- and its advice is to mint
        a new key, which is wrong on a machine that already has a sealed
        database.
      * `demo-reset.ps1` drops the schema, so every device_credential goes
        with it. The KDS then reconnects forever with no visible cause,
        because a rejected auth frame just closes the socket.
      * `pnpm dev` does not read `.env.dev`; only `pnpm dev --mode dev` does.
        The KDS reports "not configured" and the file it names is fine.

    None of these is hard once named. All of them are invisible until named.
    This script names them.

    IT IS READ-ONLY, DELIBERATELY. It starts nothing, stops nothing and binds
    nothing. Every remediation is printed for a human to run, because the
    things that would fix these states are exactly the things that have
    displaced the operator's own stack before -- see CLAUDE.md on ports and on
    scratch databases. A diagnosis that acts is a diagnosis you cannot run
    while something important is up.

.PARAMETER Json
    Emit the findings as JSON instead of a table, for a caller that wants to
    branch on them.

.EXAMPLE
    .\scripts\dev-preflight.ps1

.EXAMPLE
    .\scripts\dev-preflight.ps1 -Json
#>
[CmdletBinding()]
param(
    # Where the POS keeps its edge database. Same default as dev-bootstrap.ps1
    # -- must match Tauri's app_data_dir() for identifier com.holler.pos.
    [string]$EdgeDataDir = (Join-Path $env:APPDATA "com.holler.pos"),

    # The cloud API's base URL, for the health probe.
    [string]$CloudBaseUrl = "http://localhost:8080",

    [switch]$Json
)

$ErrorActionPreference = 'Continue'
$repoRoot = Split-Path -Parent $PSScriptRoot

$findings = New-Object System.Collections.ArrayList

# status: OK | WARN | FAIL | INFO
function Add-Finding {
    param(
        [Parameter(Mandatory)][string]$Area,
        [Parameter(Mandatory)][ValidateSet('OK', 'WARN', 'FAIL', 'INFO')][string]$Status,
        [Parameter(Mandatory)][string]$Detail,
        [string]$Fix = ''
    )
    $null = $findings.Add([pscustomobject]@{
        Area   = $Area
        Status = $Status
        Detail = $Detail
        Fix    = $Fix
    })
}

# Port owner, with START TIME. The start time is the point: CLAUDE.md records a
# sweep where three of four listeners were the operator's, started minutes
# earlier, and killing by port alone would have taken them out.
function Get-PortOwner {
    param([int]$Port)
    $conn = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue |
            Select-Object -First 1
    if (-not $conn) { return $null }
    $proc = Get-Process -Id $conn.OwningProcess -ErrorAction SilentlyContinue
    if (-not $proc) {
        return [pscustomobject]@{ Port = $Port; Pid = $conn.OwningProcess; Name = '<exited>'; StartTime = $null }
    }
    $start = $null
    try { $start = $proc.StartTime } catch { }
    [pscustomobject]@{ Port = $Port; Pid = $proc.Id; Name = $proc.ProcessName; StartTime = $start }
}

# ---------------------------------------------------------------- 1. Docker --

$dockerOk = $false
$psOut = & docker ps --format '{{.Names}}|{{.Status}}|{{.Ports}}' 2>&1
if ($LASTEXITCODE -ne 0) {
    Add-Finding -Area 'docker' -Status 'FAIL' `
        -Detail 'Docker engine is not reachable. Docker Desktop does not autostart on this box.' `
        -Fix 'Start Docker Desktop, then: docker compose up -d postgres redis nats'
} else {
    $dockerOk = $true
    $running = @{}
    foreach ($line in @($psOut)) {
        if ($line -isnot [string] -or $line -notmatch '\|') { continue }
        $parts = $line -split '\|'
        $running[$parts[0]] = $parts[1]
    }

    foreach ($svc in 'holler-postgres-1', 'holler-redis-1', 'holler-nats-1') {
        if ($running.ContainsKey($svc)) {
            Add-Finding -Area 'docker' -Status 'OK' -Detail "$svc : $($running[$svc])"
        } else {
            Add-Finding -Area 'docker' -Status 'FAIL' -Detail "$svc is NOT running." `
                -Fix 'docker compose up -d postgres redis nats    # not `make dev`, and not a bare `docker compose up` -- the backend service fails to build'
        }
    }

    # A FOREIGN container on 5432 is the one that wastes an hour: Holler's
    # postgres silently fails to bind and the backend reaches a stranger.
    foreach ($name in $running.Keys) {
        if ($name -notlike 'holler-*' -and $running[$name] -match 'Up') {
            $portsLine = ($psOut | Where-Object { $_ -like "$name|*" }) -join ''
            if ($portsLine -match '5432') {
                Add-Finding -Area 'docker' -Status 'FAIL' `
                    -Detail "Container '$name' (not Holler's) is holding port 5432. Holler's postgres cannot bind, and the backend would connect to that database instead." `
                    -Fix "docker stop $name    # then: docker compose up -d postgres redis nats"
            }
        }
    }
}

# ------------------------------------------------- 2. stale POS / orphan WS --
#
# THE STALE-WINDOW CASE. A6-1 is a confirmed FAIL: closing the window does not
# end the process. The process keeps 9310/9320 and keeps edge.db open, and the
# next launch then fails for reasons that read as a code fault.
#
# A window is judged gone by MainWindowHandle/MainWindowTitle, never by whether
# the port answers -- the orphan answers identically to a healthy one.

$posProcs = @(Get-Process holler-pos -ErrorAction SilentlyContinue)
if ($posProcs.Count -eq 0) {
    Add-Finding -Area 'pos' -Status 'INFO' -Detail 'No holler-pos process is running.'
} else {
    foreach ($p in $posProcs) {
        $start = $null
        try { $start = $p.StartTime } catch { }
        $hasWindow = ($p.MainWindowHandle -ne 0) -and (-not [string]::IsNullOrEmpty($p.MainWindowTitle))
        if ($hasWindow) {
            Add-Finding -Area 'pos' -Status 'OK' `
                -Detail "holler-pos pid $($p.Id) running with a window ('$($p.MainWindowTitle)'), started $start"
        } else {
            Add-Finding -Area 'pos' -Status 'FAIL' `
                -Detail ("holler-pos pid $($p.Id) is ALIVE BUT HAS NO WINDOW (handle=$($p.MainWindowHandle), title='$($p.MainWindowTitle)'), started $start. " +
                         'This is gap A6: the window closed, the Tauri event loop never ended, so RunEvent::Exit never fired and the exit hook never sealed the database. It still holds 9310/9320 and edge.db.') `
                -Fix "Stop-Process -Id $($p.Id) -Force"
        }
    }
}

# ----------------------------------------------------------------- 3. ports --
#
# Every port the operator's own stack uses. The rule in CLAUDE.md is about the
# ports, not about intent, so this lists owners rather than judging them.

$portMap = [ordered]@{
    5432 = 'postgres'
    6379 = 'redis'
    4222 = 'nats'
    8080 = 'backend API'
    9310 = 'POS LAN server (KDS)'
    9320 = 'POS captain listener'
    5173 = 'POS dev server'
    5174 = 'KDS dev server'
    5175 = 'admin dev server'
}
# NOTE: iterate with GetEnumerator(). Indexing an OrderedDictionary with an
# INTEGER is POSITIONAL in PowerShell, not by key, so $portMap[5432] asks for
# the 5432nd entry and returns $null -- every label printed as "()" until this
# was fixed.
foreach ($entry in $portMap.GetEnumerator()) {
    $port  = [int]$entry.Key
    $label = $entry.Value
    $owner = Get-PortOwner -Port $port
    if ($owner) {
        Add-Finding -Area 'ports' -Status 'INFO' `
            -Detail "$port ($label) <- $($owner.Name) pid=$($owner.Pid) started=$($owner.StartTime)"
    } else {
        Add-Finding -Area 'ports' -Status 'INFO' -Detail "$port ($label) free"
    }
}

# --------------------------------------------------------------- 4. backend --
#
# Identity AND health, together. Either one alone is the trap: a health check
# passes against a process you did not start, and a live pid proves nothing
# about whether it serves.

$apiOwner = Get-PortOwner -Port 8080
$healthOk = $false
try {
    $resp = Invoke-WebRequest -Uri "$CloudBaseUrl/health" -UseBasicParsing -TimeoutSec 5
    $healthOk = ($resp.StatusCode -eq 200)
} catch {
    $healthOk = $false
}

if ($healthOk -and $apiOwner) {
    Add-Finding -Area 'backend' -Status 'OK' `
        -Detail "healthy on 8080, served by $($apiOwner.Name) pid=$($apiOwner.Pid) started=$($apiOwner.StartTime). Record THIS pid -- a restart is verified by a new pid, never by the port answering."
} elseif ($healthOk -and -not $apiOwner) {
    Add-Finding -Area 'backend' -Status 'WARN' `
        -Detail '/health returns 200 but nothing local is listening on 8080. Something is proxying, or the URL points elsewhere.'
} else {
    Add-Finding -Area 'backend' -Status 'FAIL' -Detail "No healthy backend at $CloudBaseUrl." `
        -Fix '.\scripts\dev-up.ps1 -SkipInfra -SkipSeed -NoKds -NoPos    # own window; then record the NEW api pid, not the powershell wrapper pid it prints'
}

# ----------------------------------------------------- 5. edge database key --
#
# The single most common stall. It is per-process, so a new terminal is a new
# failure, every time.

if ([string]::IsNullOrWhiteSpace($env:HOLLER_DB_KEY_HEX)) {
    $posEnv = Join-Path $repoRoot 'apps\pos\.env.dev'
    $sealed = Join-Path $EdgeDataDir 'edge.db.enc'
    if (Test-Path -LiteralPath $sealed) {
        # WARN, not FAIL. This reads the environment of the shell THIS SCRIPT
        # runs in, which is not necessarily the shell that will launch the POS
        # -- an agent running the preflight always sees it unset, and reporting
        # that as a blocker makes every clean stack look broken. The variable
        # is per-process and only has to be set where run-dev.ps1 is invoked.
        Add-Finding -Area 'edge-key' -Status 'WARN' `
            -Detail 'HOLLER_DB_KEY_HEX is not set IN THIS TERMINAL, and a sealed edge.db.enc EXISTS. It only needs to be set in the terminal you launch the POS from -- if that is a different window, this is not a problem. Do not mint a new key: a different key cannot open that file and never falls back to an empty one.' `
            -Fix "`$env:HOLLER_DB_KEY_HEX = (Select-String -Path '$posEnv' -Pattern '^HOLLER_DB_KEY_HEX=(.+)`$').Matches[0].Groups[1].Value"
    } else {
        Add-Finding -Area 'edge-key' -Status 'WARN' `
            -Detail 'HOLLER_DB_KEY_HEX is not set and no sealed edge.db.enc exists. This machine has no edge database yet, so minting a key is correct here.' `
            -Fix 'See scripts\dev-bootstrap.ps1''s refusal message for the mint snippet.'
    }
} elseif ($env:HOLLER_DB_KEY_HEX -notmatch '^[0-9a-fA-F]{64}$') {
    Add-Finding -Area 'edge-key' -Status 'FAIL' `
        -Detail "HOLLER_DB_KEY_HEX is set but is $($env:HOLLER_DB_KEY_HEX.Length) character(s); it must be exactly 64 hex characters." `
        -Fix 'Re-read it from apps\pos\.env.dev rather than editing it by hand.'
} else {
    Add-Finding -Area 'edge-key' -Status 'OK' -Detail 'HOLLER_DB_KEY_HEX is set in this terminal and is 64 hex characters. (Value never printed.)'
}

# ------------------------------------------------------- 6. edge db on disk --

if (-not (Test-Path -LiteralPath $EdgeDataDir)) {
    Add-Finding -Area 'edge-db' -Status 'INFO' -Detail "$EdgeDataDir does not exist yet."
} else {
    $plain  = Get-Item (Join-Path $EdgeDataDir 'edge.db') -ErrorAction SilentlyContinue
    $sealed = Get-Item (Join-Path $EdgeDataDir 'edge.db.enc') -ErrorAction SilentlyContinue
    $marker = Test-Path -LiteralPath (Join-Path $EdgeDataDir 'edge.db.open-marker')
    $posAlive = $posProcs.Count -gt 0

    if ($sealed) {
        Add-Finding -Area 'edge-db' -Status 'OK' -Detail "edge.db.enc present, $($sealed.Length) bytes, written $($sealed.LastWriteTime)"
    } else {
        Add-Finding -Area 'edge-db' -Status 'WARN' -Detail 'No edge.db.enc. The edge database has never been sealed here.'
    }

    if ($plain -and -not $posAlive) {
        $msg = "Plaintext edge.db is present ($($plain.Length) bytes, $($plain.LastWriteTime)) with NO holler-pos running. The last exit did not seal."
        if ($sealed -and $plain.LastWriteTime -gt $sealed.LastWriteTime) {
            $msg += " It is NEWER than edge.db.enc ($($sealed.LastWriteTime)), so a backup that copies only the .enc loses the last session."
        }
        $msg += ' Cached Argon2id credential hashes are on disk in the clear (ADR-011). No data is lost: recover_crash_leftovers folds it back in on the next open.'
        Add-Finding -Area 'edge-db' -Status 'WARN' -Detail $msg `
            -Fix 'Launch the POS once and let it recover, or back up BOTH files. Tracked as gap A6.'
    } elseif ($plain -and $posAlive) {
        Add-Finding -Area 'edge-db' -Status 'OK' -Detail 'Plaintext edge.db present while the POS runs -- normal, that is the working file.'
    }

    if ($marker -and -not $posAlive) {
        Add-Finding -Area 'edge-db' -Status 'WARN' -Detail 'edge.db.open-marker is present with no POS running -- another sign the last exit was abnormal.'
    }

    $quarantined = @(Get-ChildItem $EdgeDataDir -Filter '*.unreadable-*' -ErrorAction SilentlyContinue)
    if ($quarantined.Count -gt 0) {
        $when = ($quarantined | Sort-Object LastWriteTime | Select-Object -First 1).LastWriteTime
        Add-Finding -Area 'edge-db' -Status 'INFO' `
            -Detail "$($quarantined.Count) quarantined leftover file(s), oldest $when. These are EVIDENCE, kept with their bytes intact. Do not delete them -- that is the operator's call."
    }
}

# ------------------------------------------------- 7. devices / credentials --
#
# demo-reset.ps1 drops the schema, so every credential goes with it while the
# tokens written into .env files survive. The KDS then reconnects for ever and
# says only "Disconnected" -- a rejected auth frame just closes the socket.

if ($dockerOk) {
    $devq = & docker exec holler-postgres-1 psql -U holler -d holler -tAc `
        "SELECT (SELECT count(*) FROM device), (SELECT count(*) FROM device_credential WHERE revoked_at IS NULL);" 2>&1
    if ($LASTEXITCODE -eq 0 -and "$devq" -match '(\d+)\|(\d+)') {
        $deviceCount = [int]$Matches[1]
        $credCount   = [int]$Matches[2]
        if ($credCount -eq 0 -and $deviceCount -gt 0) {
            Add-Finding -Area 'devices' -Status 'FAIL' `
                -Detail "$deviceCount device row(s) but ZERO live device_credential rows. Any token already written into apps\kds\.env.dev or the captain's storage points at a credential that no longer exists, so the KDS will reconnect silently for ever." `
                -Fix '.\scripts\dev-bootstrap.ps1    # step [3c/4] re-enrols and rewrites apps\kds\.env.dev'
        } else {
            Add-Finding -Area 'devices' -Status 'OK' -Detail "$deviceCount device row(s), $credCount live credential(s)."
        }
    } else {
        Add-Finding -Area 'devices' -Status 'WARN' -Detail 'Could not query device/device_credential (is the schema migrated?).'
    }
}

# ------------------------------------------------------------- 8. KDS env ----

$kdsEnv = Join-Path $repoRoot 'apps\kds\.env.dev'
if (Test-Path -LiteralPath $kdsEnv) {
    $needed = 'VITE_KDS_OUTLET_ID', 'VITE_KDS_DEVICE_ID', 'VITE_KDS_DEVICE_TOKEN'
    $missing = @($needed | Where-Object {
        $null -eq (Select-String -Path $kdsEnv -Pattern "^$_=." -ErrorAction SilentlyContinue)
    })
    if ($missing.Count -gt 0) {
        Add-Finding -Area 'kds' -Status 'FAIL' -Detail "apps\kds\.env.dev is missing: $($missing -join ', ')" `
            -Fix '.\scripts\dev-bootstrap.ps1'
    } else {
        Add-Finding -Area 'kds' -Status 'OK' `
            -Detail 'apps\kds\.env.dev carries outlet id, device id and device token. REMEMBER: `pnpm dev` does NOT read this file -- only `pnpm dev --mode dev` does.'
    }
} else {
    Add-Finding -Area 'kds' -Status 'WARN' -Detail 'apps\kds\.env.dev does not exist.' -Fix '.\scripts\dev-bootstrap.ps1'
}

# ------------------------------------------------------ 8b. printer sink ----
#
# Without HOLLER_PRINTER_FILE_SINK_DIR the seeded printer points at a
# deliberately non-existent device path (edge/database/src/bin/devseed.rs:3088),
# so "Print Bill" writes NOTHING and -- observed 2026-10-04 -- shows no banner
# either. On a machine with no printer attached, which is every machine here,
# that silently removes the receipt from the demo's step 2.
#
# Note it is read at transport construction, i.e. at PROCESS START, so it has to
# be in the environment of the shell that launches the POS -- putting it in
# .env.dev only helps because run-dev.ps1 reads that file.

$posEnvFile = Join-Path $repoRoot 'apps\pos\.env.dev'
if (Test-Path -LiteralPath $posEnvFile) {
    $sinkLine = Select-String -Path $posEnvFile -Pattern '^HOLLER_PRINTER_FILE_SINK_DIR=(.+)$' -ErrorAction SilentlyContinue
    if ($sinkLine) {
        $sinkDir = $sinkLine.Matches[0].Groups[1].Value.Trim()
        if (Test-Path -LiteralPath $sinkDir) {
            $recent = @(Get-ChildItem $sinkDir -Filter '*.pdf' -ErrorAction SilentlyContinue |
                        Sort-Object LastWriteTime -Descending | Select-Object -First 1)
            $last = if ($recent.Count -gt 0) { " Last receipt: $($recent[0].LastWriteTime)." } else { ' No receipts written yet.' }
            Add-Finding -Area 'printer' -Status 'OK' -Detail "File sink -> $sinkDir.$last"
        } else {
            Add-Finding -Area 'printer' -Status 'WARN' -Detail "HOLLER_PRINTER_FILE_SINK_DIR names '$sinkDir', which does not exist." `
                -Fix "New-Item -ItemType Directory -Path '$sinkDir' -Force"
        }
    } else {
        Add-Finding -Area 'printer' -Status 'FAIL' `
            -Detail 'HOLLER_PRINTER_FILE_SINK_DIR is NOT in apps\pos\.env.dev. With no printer attached the seeded printer points at a non-existent device path, so Print Bill writes nothing AND shows no banner -- the receipt silently disappears from the demo.' `
            -Fix '.\scripts\dev-bootstrap.ps1 -PrinterFileSinkDir C:\Code\Holler\.dev-prints   # or set $env:HOLLER_PRINTER_FILE_SINK_DIR in the shell that launches the POS'
    }
}

# ------------------------------------------------------- 9. release binary ---
#
# Delegated, never reimplemented: check-release-binary.ps1 asserts the binary
# CONTAINS the current dist entry chunk. The obvious test -- looking for a
# localhost:5173 string -- passes the broken binary, because a correct
# production binary still embeds the whole tauri.conf.json.

$exe = Join-Path $repoRoot 'apps\pos\src-tauri\target\release\holler-pos.exe'
if (Test-Path -LiteralPath $exe) {
    $checker = Join-Path $PSScriptRoot 'check-release-binary.ps1'
    if (Test-Path -LiteralPath $checker) {
        $out = & $checker -RepoRoot $repoRoot 2>&1 6>&1
        if ($LASTEXITCODE -eq 0) {
            Add-Finding -Area 'release-binary' -Status 'OK' -Detail 'Content-verified: it carries the current dist entry chunk.'
        } else {
            Add-Finding -Area 'release-binary' -Status 'FAIL' `
                -Detail "check-release-binary.ps1 refused it: $(($out | Select-Object -Last 3) -join ' / ')" `
                -Fix 'cd apps\pos; pnpm exec tauri build    # cargo build --release produces a DEV-MODE app that fetches its UI from localhost:5173'
        }
    }
} else {
    Add-Finding -Area 'release-binary' -Status 'WARN' -Detail 'No release binary built yet.' `
        -Fix 'cd apps\pos; pnpm exec tauri build'
}

# ------------------------------------------------------------------ output ---

if ($Json) {
    $findings | ConvertTo-Json -Depth 4
    return
}

$colour = @{ OK = 'Green'; WARN = 'Yellow'; FAIL = 'Red'; INFO = 'Gray' }

Write-Host ''
Write-Host 'Holler dev preflight' -ForegroundColor Cyan
Write-Host "repo: $repoRoot"
Write-Host "edge: $EdgeDataDir"
Write-Host ''

foreach ($area in ($findings | Select-Object -ExpandProperty Area -Unique)) {
    Write-Host "[$area]" -ForegroundColor Cyan
    foreach ($f in ($findings | Where-Object { $_.Area -eq $area })) {
        Write-Host ("  {0,-5} {1}" -f $f.Status, $f.Detail) -ForegroundColor $colour[$f.Status]
        if ($f.Fix) { Write-Host "        fix: $($f.Fix)" -ForegroundColor DarkCyan }
    }
    Write-Host ''
}

$fails = @($findings | Where-Object { $_.Status -eq 'FAIL' })
$warns = @($findings | Where-Object { $_.Status -eq 'WARN' })

if ($fails.Count -eq 0) {
    Write-Host "No blockers. $($warns.Count) warning(s)." -ForegroundColor Green
} else {
    Write-Host "$($fails.Count) blocker(s), $($warns.Count) warning(s). Run the fixes above in order." -ForegroundColor Red
}
Write-Host ''
Write-Host 'This script changed nothing. It starts, stops and binds nothing, by design.' -ForegroundColor DarkGray

exit 0
