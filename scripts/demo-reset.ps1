# Holler demo build -- the one command that resets cloud + edge to the known
# demo-seed state (T10, demo-kickoff work item 2). Implements, IN ORDER, the
# four steps seed/README.md's "The one command" and the demo-kickoff brief
# specify:
#
#   1. verify the backend by PID (never by the port answering)
#   2. drop and re-apply the Postgres schema, then run backend devseed
#   3. delete the edge data directory (edge.db.enc AND the plaintext edge.db
#      gap A6 leaves beside it), then run the edge devseed
#   4. assert: zero rows in sync_replay_block, zero in stock_deduction_gap,
#      zero blocked rows in local_outbox, and the sync-banner PROXY is empty
#
# DEVELOPMENT / DEMO-REHEARSAL ONLY. Nothing here runs at an outlet (ADR-013).
#
# THIS SCRIPT IS DESTRUCTIVE. It drops the entire "public" Postgres schema and
# deletes the encrypted edge SQLite file. Both are irreversible. It refuses to
# do either without -Force (or -WhatIf, which does neither and only reports
# what it would do).
#
# NON-INTERACTIVE BY DESIGN: apps\pos\.env.dev carries the edge database key
# and is deny-ruled to agents, so an agent cannot run this end to end -- an
# operator does, watching the output. Every step says what it is doing before
# it does it, and every failure names the next action rather than a bare
# stack trace.
#
# Reuses scripts\dev-bootstrap.ps1's own patterns (the KEY=VALUE devseed
# output parsing, the hex-key validation, the "own window" backend launch
# scripts\dev-up.ps1 uses) rather than writing a second, weaker version of
# that logic. Does NOT modify or call either script directly: dev-bootstrap
# also WRITES apps\pos\.env.dev, which this script must not touch (the
# demo operator's already-provisioned file, holding settings -- printer sink,
# sync credential -- this script has no business overwriting).
#
# Usage:
#   .\scripts\demo-reset.ps1 -WhatIf                     # see what would happen, nothing destroyed
#   .\scripts\demo-reset.ps1 -Force                       # the real reset
#   .\scripts\demo-reset.ps1 -Force -DbKeyHex <64 hex>    # explicit key instead of $env:HOLLER_DB_KEY_HEX
#
# See docs/demo-reset.md for the runbook this backs.

[CmdletBinding()]
param(
    # 32-byte key, hex-encoded, for the edge database's encryption at rest
    # (ADR-011). Same rule as dev-bootstrap.ps1: NO DEFAULT. Falls back to
    # $env:HOLLER_DB_KEY_HEX; refuses to run without one. Must be the SAME
    # key the till's apps\pos\.env.dev already carries -- a different key
    # fails to open the sealed database this script just seeded; it does not
    # open or create a different, empty one. If a crash-recovery leftover is
    # present, a wrong key used to be able to silently OVERWRITE the real
    # sealed database with that leftover (T25); the edge crate now verifies
    # the key against the sealed file first and refuses instead.
    [string]$DbKeyHex = "",

    # The repository this run operates on. A parameter because
    # scripts\agent-guard.ps1 requires an agent shell to name a scratch tree
    # EXPLICITLY rather than inherit the real one.
    [string]$RepoRoot = "",

    # Must match Tauri's app_data_dir() for com.holler.pos -- same default
    # dev-bootstrap.ps1 uses, and it must agree with whatever apps\pos\.env.dev
    # was provisioned against.
    [Alias('DataDir')]
    [string]$EdgeDataDir = (Join-Path $env:APPDATA "com.holler.pos"),

    [string]$CloudBaseUrl = "http://localhost:8080",
    [int]$BackendPort = 8080,

    # Matches docker-compose.yml. Compose's default container naming is
    # <project>-<service>-<index>; the project name is this repo directory's
    # name lowercased ("holler"), the same value scripts\dev-bootstrap.ps1
    # already hardcodes for its device-lookup psql call.
    [string]$DatabaseUrl = "postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable",
    [string]$PostgresContainer = "holler-postgres-1",
    [string]$PostgresUser = "holler",
    [string]$PostgresDb = "holler",

    # DEVELOPMENT signing key, matching dev-up.ps1's default. Backend config
    # has no default by design (a missing secret is a startup error there);
    # this is a dev-only substitute, never used for real data.
    [string]$TokenSigningKey = "holler-dev-signing-key-not-for-prod",
    [string]$AdminOrigin = "http://localhost:5175",

    # Required (or -WhatIf) before anything is destroyed. See the
    # destructive-action banner below for exactly what.
    [switch]$Force,

    # Reports what would be destroyed and what commands would run, executes
    # nothing destructive: no process is killed, no schema is dropped, no
    # file is deleted, no seed is run. Lets a caller exercise the argument
    # handling and the preflight checks without the key or a live stack.
    [switch]$WhatIf
)

$ErrorActionPreference = "Stop"

# STRUCTURAL GUARD, FIRST THING -- before the destructive banner, before the
# preflight, before anything. Dot-sourced so a missing guard file stops the
# script rather than silently disabling the control.
. (Join-Path $PSScriptRoot "agent-guard.ps1")

$repoRoot = if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    Split-Path -Parent $PSScriptRoot
} else {
    $RepoRoot
}

Assert-AgentSafePaths -ScriptName "demo-reset.ps1" `
    -BoundParameters $PSBoundParameters `
    -RepoRootValue $repoRoot `
    -DataDirValue $EdgeDataDir `
    -DataDirParameterName "EdgeDataDir"

$startedAt = Get-Date

function Write-Step($n, $msg) { Write-Host ""; Write-Host "[$n/4] $msg" -ForegroundColor Cyan }
function Write-Note($msg)     { Write-Host "       $msg" -ForegroundColor DarkGray }
function Write-Assert($ok, $line) {
    if ($ok) { Write-Host "       $line" -ForegroundColor Green }
    else     { Write-Host "       $line" -ForegroundColor Red }
}

# Every fatal path goes through this so the message a human reads always
# names the next action, never a bare exception. Mirrors the rule this
# script itself exists to satisfy: "an assertion whose output has to be read
# by a human to know whether it passed is not an assertion" -- applied here to
# every failure, not only the final four.
function Fail-WithAction($problem, $nextAction) {
    Write-Host ""
    Write-Host "FAILED: $problem" -ForegroundColor Red
    Write-Host "NEXT ACTION: $nextAction" -ForegroundColor Yellow
    exit 1
}

# --- T24: key-quality helpers -----------------------------------------------
# Identical copy of scripts\dev-bootstrap.ps1's two functions -- no shared
# module between the two owned scripts, kept in sync deliberately.

# HEURISTIC, stated as such wherever it is used. It cannot prove a key is
# cryptographically random -- it exists to catch the ONE failure that
# actually happened: a human or an agent typing a pattern to satisfy the
# hex/length regex. Returns $null when the key looks fine, or a short reason
# string when it looks like a placeholder.
function Get-KeyWeaknessReason {
    param([string]$HexKey)

    $bytes = @(for ($i = 0; $i -lt $HexKey.Length; $i += 2) {
        [Convert]::ToByte($HexKey.Substring($i, 2), 16)
    })

    # A real random 32-byte key has ~26-30 distinct byte values with
    # overwhelming probability (expected distinct count for 32 draws from
    # 256 values is ~27.5, by the birthday-paradox calculation). 16 is a
    # generous floor: it catches a single repeated byte (1 distinct), a
    # short repeated sequence, and every other hand-typed placeholder this
    # check has been tried against, while leaving wide margin before it
    # could ever flag a genuinely random key.
    $distinct = ($bytes | Select-Object -Unique).Count
    if ($distinct -lt 16) {
        return "only $distinct distinct byte value(s) across 32 bytes (need at least 16)"
    }

    # Constant-step run: 00 01 02 03 ... or ff fe fd fc ... -- every distinct
    # byte value can still be 32/32 while the key is trivially guessable.
    $diffs = @(for ($i = 1; $i -lt $bytes.Count; $i++) {
        (($bytes[$i] - $bytes[$i - 1]) + 256) % 256
    })
    if (($diffs | Select-Object -Unique).Count -eq 1) {
        return "bytes form a constant-step sequence (step $($diffs[0]))"
    }

    # Short repeating period: a short pattern typed or pasted repeatedly to
    # fill 32 bytes (e.g. a 4- or 8-byte phrase repeated 8x/4x).
    foreach ($period in @(1, 2, 4, 8)) {
        if ($bytes.Count % $period -ne 0) { continue }
        $isPeriodic = $true
        for ($i = $period; $i -lt $bytes.Count; $i++) {
            if ($bytes[$i] -ne $bytes[$i % $period]) { $isPeriodic = $false; break }
        }
        if ($isPeriodic) {
            return "bytes repeat with a $period-byte period"
        }
    }

    return $null
}

# A short, non-reversible fingerprint for comparing two keys in terminal
# output without ever printing either one. First 12 hex characters (6 bytes)
# of SHA-256 over the raw key bytes.
function Get-KeyFingerprint {
    param([string]$HexKey)
    $bytes = @(for ($i = 0; $i -lt $HexKey.Length; $i += 2) {
        [Convert]::ToByte($HexKey.Substring($i, 2), 16)
    })
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha.ComputeHash([byte[]]$bytes)
    } finally {
        $sha.Dispose()
    }
    return (($hash | ForEach-Object { '{0:x2}' -f $_ }) -join '').Substring(0, 12)
}

# --- T26: caller-environment save/restore -----------------------------------
# `$env:X = ...` inside this script mutates the CALLING SHELL's process
# environment, not a scoped copy -- there is no child-process boundary
# between this script and whatever invoked it with `.\demo-reset.ps1`. A
# `Remove-Item Env:\X` in a `finally` therefore does not "clean up a local",
# it deletes whatever the operator's shell had, including a value the
# operator set before running this script. Every site that needs a variable
# in the current process (to be inherited by a child like `go run` or `cargo
# run`) must save the caller's prior value first and restore EXACTLY that in
# `finally` -- including restoring absence when the caller had not set it,
# which a bare `Remove-Item -ErrorAction SilentlyContinue` gets right only by
# accident (it also fires when the caller DID have a value, discarding it).
# Identical copy of scripts\dev-bootstrap.ps1's two functions -- no shared
# module between the two owned scripts, kept in sync deliberately.
function Save-CallerEnv {
    param([string[]]$Names)
    $saved = @{}
    foreach ($n in $Names) {
        $item = Get-Item -Path "Env:\$n" -ErrorAction SilentlyContinue
        if ($item) { $saved[$n] = $item.Value } else { $saved[$n] = $null }
    }
    return $saved
}

function Restore-CallerEnv {
    param([hashtable]$Saved)
    foreach ($n in $Saved.Keys) {
        if ($null -eq $Saved[$n]) {
            # Caller did not have this set. Verified on PowerShell 5.1: both
            # `$env:X = $null` and `$env:X = ''` remove the variable outright
            # (Windows process environment has no concept of an empty-string
            # value distinct from absent), so Remove-Item is not a weaker
            # substitute here -- it is the same operation, chosen for the
            # explicit -ErrorAction rather than relying on that equivalence.
            Remove-Item -Path "Env:\$n" -ErrorAction SilentlyContinue
        } else {
            Set-Item -Path "Env:\$n" -Value $Saved[$n]
        }
    }
}

# --- key -----------------------------------------------------------------
if ([string]::IsNullOrWhiteSpace($DbKeyHex)) { $DbKeyHex = $env:HOLLER_DB_KEY_HEX }
if ([string]::IsNullOrWhiteSpace($DbKeyHex)) {
    Fail-WithAction `
        "HOLLER_DB_KEY_HEX is not set and -DbKeyHex was not supplied." `
        "Set `$env:HOLLER_DB_KEY_HEX to the SAME key apps\pos\.env.dev already carries (see that file's HOLLER_DB_KEY_HEX line), then re-run."
}
if ($DbKeyHex -notmatch '^[0-9a-fA-F]{64}$') {
    Fail-WithAction `
        "HOLLER_DB_KEY_HEX must be exactly 64 hex characters (32 bytes); got $($DbKeyHex.Length)." `
        "Copy the exact value from apps\pos\.env.dev and re-run."
}
$weakReason = Get-KeyWeaknessReason -HexKey $DbKeyHex
if ($weakReason) {
    Fail-WithAction `
        "HOLLER_DB_KEY_HEX looks like a placeholder, not a random key: $weakReason" `
        "This is a HEURISTIC (see Get-KeyWeaknessReason above) -- it cannot prove randomness, only catch a hand-typed pattern. NO AGENT MAY SUPPLY A LITERAL KEY. Copy the exact value from apps\pos\.env.dev (it was validated by dev-bootstrap.ps1 when written) and re-run, or mint a fresh one with: `$b = New-Object byte[] 32; [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes(`$b); `$env:HOLLER_DB_KEY_HEX = -join (`$b | ForEach-Object { '{0:x2}' -f `$_ })"
}

$edgeSealedPath    = Join-Path $EdgeDataDir "edge.db.enc"
$edgePlaintextPath = Join-Path $EdgeDataDir "edge.db"

Write-Host "Holler demo reset" -ForegroundColor Green
Write-Host "repo: $repoRoot"
Write-Host "started: $startedAt"

# --- destructive-action banner ---------------------------------------------
# Printed unconditionally, before anything is touched, whether or not -Force
# was given -- a caller who forgot -Force still sees exactly what refusing to
# run just avoided.
Write-Host ""
Write-Host "THIS RUN WILL DESTROY, IRREVERSIBLY:" -ForegroundColor Yellow
Write-Host "  - the ENTIRE 'public' schema in Postgres database '$PostgresDb'" -ForegroundColor Yellow
Write-Host "    (container '$PostgresContainer', user '$PostgresUser') -- every table, every row" -ForegroundColor Yellow
Write-Host "  - $edgePlaintextPath, if present (gap A6's leftover plaintext copy) -- DELETED FIRST" -ForegroundColor Yellow
Write-Host "  - $edgeSealedPath (the encrypted edge database) -- DELETED LAST, so a failure" -ForegroundColor Yellow
Write-Host "    part-way through leaves the sealed database intact rather than a bare leftover" -ForegroundColor Yellow
Write-Host "  - the current backend API process listening on port $BackendPort (killed and restarted)" -ForegroundColor Yellow
Write-Host ""
Write-Host "NO BACKUP IS TAKEN. The edge database is encrypted at rest and is never" -ForegroundColor Yellow
Write-Host "copied anywhere unencrypted, including 'for safety' -- ADR-011 is absolute." -ForegroundColor Yellow

if ($WhatIf) {
    Write-Host ""
    Write-Host "-WhatIf: reporting only. Nothing above will actually be touched." -ForegroundColor Cyan
} elseif (-not $Force) {
    Fail-WithAction `
        "-Force was not supplied, and this run is destructive (see above)." `
        "Re-run with -Force to proceed, or -WhatIf to see the plan without destroying anything."
}

# --- preflight: Postgres container must already be up -----------------------
# This script does not start infrastructure itself (scripts\dev-bootstrap.ps1
# / dev-up.ps1 own that); it resets what is already running, per the brief's
# explicit prerequisite ("docker compose up -d postgres redis nats").
$pgUp = (docker inspect --format '{{.State.Running}}' $PostgresContainer 2>$null)
if ($pgUp -ne "true") {
    Fail-WithAction `
        "Postgres container '$PostgresContainer' is not running." `
        "Run 'docker compose up -d postgres redis nats' from $repoRoot, then re-run this script."
}
Write-Note "preflight: Postgres container '$PostgresContainer' is running"

# --- preflight: NOTHING MAY HOLD THE EDGE DATABASE --------------------------
# This exists because of a real reset that half-ran: it destroyed
# edge.db.enc, then FAILED to delete the plaintext edge.db because a running
# POS held the handle. That leaves the worst of the three possible states --
# no sealed file and a stale plaintext leftover -- and the edge's own
# crash-recovery path would then have RESEALED that leftover into a brand new
# edge.db.enc at the next POS start, silently promoting a pre-reset database
# to the current one.
#
# WHY THE CRASH-RECOVERY PATH CANNOT BE THE GUARD HERE:
# `recover_crash_leftovers` deliberately reseals a leftover when NO sealed
# file exists -- that is a genuine first-run crash and the committed rows in
# it must not be thrown away (docs/spec/sync.md: local transactions are never
# deleted). The T25 key check is a no-op in exactly that case, because there
# is no sealed file to verify the key against. So the edge is right to
# reseal, and the only place that can tell "first-run crash" from "a reset
# was interrupted" is HERE, before anything is destroyed.
#
# Two independent checks, because they fail in different situations:
#   - a POS process existing at all, whether or not it currently holds a
#     handle (it will take one the moment it opens the database), and
#   - the files actually being locked, which catches every other holder:
#     a sqlite3 shell, an editor, a backup agent, a previous cargo run.
function Get-HollerPosProcess {
    # Name-based, plus a path check for anything running out of this repo's
    # POS build directory -- a `cargo run`-launched binary and an installed
    # holler-pos.exe are the same risk under different process names.
    $byName = @(Get-Process -Name "holler-pos", "holler_pos" -ErrorAction SilentlyContinue)
    # The TAURI BUILD OUTPUT directory specifically, not all of apps\pos: the
    # first version of this check used apps\pos and swept in esbuild running
    # out of the POS's node_modules, so the refusal named a bundler's pid
    # ahead of the actual till. Vite, esbuild and their friends cannot hold
    # the edge database; the compiled POS binary is the only thing under this
    # tree that opens it.
    $posBuildDir = (Join-Path $repoRoot "apps\pos\src-tauri	arget")
    $byPath = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $path = $null
        try { $path = $_.Path } catch { $path = $null }   # Access denied on system processes
        $path -and $path.StartsWith($posBuildDir, [System.StringComparison]::OrdinalIgnoreCase)
    })
    # Name-matched processes first, so the message leads with the till rather
    # than with whatever else happens to sort lower by pid.
    return @(@($byName) + @($byPath) | Sort-Object -Property Id -Unique |
             Sort-Object -Property @{ Expression = { $_.ProcessName -notlike "holler*" } })
}

# Opening for WRITE with NO sharing is the check that matters: it is exactly
# what Remove-Item needs and will fail on, so this cannot report "free" for a
# file that then refuses to delete.
function Test-FileIsLocked($path) {
    if (-not (Test-Path $path)) { return $false }
    try {
        $stream = [System.IO.File]::Open($path, 'Open', 'ReadWrite', 'None')
        $stream.Close()
        $stream.Dispose()
        return $false
    } catch [System.IO.IOException] {
        return $true
    } catch [System.UnauthorizedAccessException] {
        # A read-only or ACL-denied file will not delete either, so it is a
        # refusal for this script's purposes.
        return $true
    }
}

$posProcesses = Get-HollerPosProcess
if ($posProcesses.Count -gt 0) {
    $named = ($posProcesses | ForEach-Object { "$($_.ProcessName) pid $($_.Id)" }) -join ", "
    Fail-WithAction `
        "a Holler POS process is running ($named). NOTHING HAS BEEN DESTROYED." `
        "Close the POS window (or Stop-Process -Id $($posProcesses[0].Id)) and re-run. A running POS holds the edge database open, and a reset that deletes the sealed file and then cannot delete the plaintext leaves NO sealed database and a stale leftover -- which the next POS start would reseal as the live database."
}
Write-Note "preflight: no Holler POS process is running"

$lockedFiles = @(
    $edgePlaintextPath, "$edgePlaintextPath-wal", "$edgePlaintextPath-shm", $edgeSealedPath
) | Where-Object { Test-FileIsLocked $_ }

if ($lockedFiles.Count -gt 0) {
    # No POS process explains it, so name what CAN be named: the files, and
    # every process this script can see holding a handle. Windows exposes no
    # handle-to-pid mapping without an external tool, so this reports the
    # candidates it can enumerate rather than inventing a pid.
    # NAMES, not paths. A path match on 'holler' hits every process running
    # from this repository -- the first version of this named esbuild as a
    # candidate holder of the edge database, which is a guess dressed as a
    # finding. Naming nothing is better than naming the wrong process.
    $candidates = @(Get-Process -Name "holler-pos", "holler_pos", "sqlite3", "devseed" -ErrorAction SilentlyContinue |
        ForEach-Object { "$($_.ProcessName) pid $($_.Id)" })
    $who = if ($candidates.Count -gt 0) { " Processes that could plausibly hold it: $($candidates -join ', ')." } else { " No process this script can name accounts for it -- the holder is something it cannot see." }
    Fail-WithAction `
        "another process is holding $($lockedFiles -join ', '). NOTHING HAS BEEN DESTROYED.$who" `
        "Close whatever has the edge database open -- a POS, a sqlite shell, an editor previewing the file, or a backup agent -- and re-run. If you cannot find it, 'handle64.exe $edgePlaintextPath' (Sysinternals) names the owner."
}
Write-Note "preflight: nothing holds $edgeSealedPath or $edgePlaintextPath"

# =====================================================================
# 1/4 -- verify the backend by PID, never by the port answering
# =====================================================================
Write-Step 1 "verifying the backend by PID (kill by port, confirm free, start, confirm a NEW pid)..."

function Get-ListenerOwningPid($port) {
    $c = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue
    if ($c) { return ($c | Select-Object -First 1 -ExpandProperty OwningProcess) }
    return $null
}

$oldBackendPid = Get-ListenerOwningPid $BackendPort
if ($oldBackendPid) {
    Write-Note "port $BackendPort is currently owned by pid $oldBackendPid -- killing it"
    if (-not $WhatIf) {
        Stop-Process -Id $oldBackendPid -Force -ErrorAction SilentlyContinue
    }
} else {
    Write-Note "nothing is listening on port $BackendPort"
}

if (-not $WhatIf) {
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-ListenerOwningPid $BackendPort) -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
    }
    if (Get-ListenerOwningPid $BackendPort) {
        Fail-WithAction `
            "port $BackendPort still has a listener 30s after killing pid $oldBackendPid." `
            "Find and stop whatever is re-binding port $BackendPort (Get-NetTCPConnection -LocalPort $BackendPort), then re-run."
    }
    Write-Note "port $BackendPort confirmed free"

    Write-Note "starting the backend in its own window..."
    $backendDir = Join-Path $repoRoot "backend"
    $inner = "`$Host.UI.RawUI.WindowTitle = 'holler-backend (demo-reset)'; Set-Location '$backendDir'; " +
             "`$env:DATABASE_URL='$DatabaseUrl'; `$env:TOKEN_SIGNING_KEY='$TokenSigningKey'; " +
             "`$env:HOLLER_CORS_ALLOWED_ORIGINS='$AdminOrigin'; go run ./cmd/api"
    $backendProc = Start-Process powershell -PassThru -ArgumentList @("-NoExit", "-NoProfile", "-Command", $inner)
    Write-Note "launched powershell wrapper pid $($backendProc.Id) (go run spawns its own child; the port's owning pid is checked below, not this one)"

    # Wait for the health endpoint, then read the port's OWNING pid directly --
    # `go run` execs a compiled child with a DIFFERENT pid than the wrapper
    # above, so the wrapper's pid is not the fact to check. What matters is
    # verified above the port-free wait: nothing could have re-bound
    # $BackendPort during the gap, so whatever owns it now is provably new,
    # never the pid we just killed.
    $deadline = (Get-Date).AddSeconds(60)
    $healthy = $false
    do {
        try {
            Invoke-RestMethod -Uri "$CloudBaseUrl/health" -TimeoutSec 3 | Out-Null
            $healthy = $true
        } catch { Start-Sleep -Seconds 2 }
    } while (-not $healthy -and (Get-Date) -lt $deadline)
    if (-not $healthy) {
        Fail-WithAction `
            "the backend did not answer $CloudBaseUrl/health within 60s of starting." `
            "Check the 'holler-backend (demo-reset)' window for a Go build/runtime error."
    }
    $newBackendPid = Get-ListenerOwningPid $BackendPort
    if (-not $newBackendPid) {
        Fail-WithAction `
            "the backend answered /health but no process owns port $BackendPort." `
            "This should not happen; inspect the 'holler-backend (demo-reset)' window."
    }
    if ($oldBackendPid -and ($newBackendPid -eq $oldBackendPid)) {
        Fail-WithAction `
            "port $BackendPort is owned by the SAME pid ($oldBackendPid) that was supposedly killed." `
            "The restart did not happen. Do not trust this backend's in-memory state (rate limiter etc); investigate before continuing."
    }
    Write-Note "backend verified: port $BackendPort now owned by pid $newBackendPid (was $oldBackendPid, or nothing)"
} else {
    Write-Note "-WhatIf: would start the backend and confirm a new pid on port $BackendPort"
}

# =====================================================================
# 2/4 -- drop and re-apply the Postgres schema, then run backend devseed
# =====================================================================
Write-Step 2 "dropping and re-applying the Postgres schema, then seeding cloud fixtures..."

$dropSql = "DROP SCHEMA public CASCADE; CREATE SCHEMA public; " +
           "GRANT ALL ON SCHEMA public TO $PostgresUser; GRANT ALL ON SCHEMA public TO public;"
if ($WhatIf) {
    Write-Note "-WhatIf: would run inside container '$PostgresContainer': $dropSql"
} else {
    Write-Note "dropping schema 'public' in database '$PostgresDb'..."
    docker exec $PostgresContainer psql -U $PostgresUser -d $PostgresDb -v ON_ERROR_STOP=1 -c $dropSql
    if ($LASTEXITCODE -ne 0) {
        Fail-WithAction `
            "dropping/recreating the Postgres schema failed (exit $LASTEXITCODE)." `
            "Check the psql output above; confirm '$PostgresContainer' is the right container and '$PostgresUser' can DROP SCHEMA."
    }
    Write-Note "schema dropped and recreated"

    # THE DEVICE IDS IN THE BOOTSTRAP STATE FILE JUST STOPPED EXISTING.
    # scripts\dev-bootstrap.ps1 remembers (cloud, outlet, kind, name) -> device
    # id so it can ROTATE a credential instead of enrolling a second device.
    # Dropping the schema deletes every one of those rows, so each remembered
    # id now names nothing -- and the next bootstrap rotated one and got a 404
    # from the backend, which reads as a missing route rather than a missing
    # row. The script that destroyed the rows is the one that knows, so it
    # prunes them here.
    $bootstrapState = Join-Path $env:LOCALAPPDATA "Holler\dev-bootstrap-state.json"
    if (Test-Path $bootstrapState) {
        try {
            $raw = Get-Content $bootstrapState -Raw -ErrorAction Stop
            $parsed = $raw | ConvertFrom-Json
            $kept = @{}
            $dropped = 0
            foreach ($entry in $parsed.PSObject.Properties) {
                # Keys are "cloud|outlet|kind|name"; only this cloud's entries
                # were invalidated by this drop.
                if ($entry.Name -like "$CloudBaseUrl|*") { $dropped++ } else { $kept[$entry.Name] = $entry.Value }
            }
            if ($dropped -gt 0) {
                ($kept | ConvertTo-Json) | Out-File -FilePath $bootstrapState -Encoding ascii
                Write-Note "pruned $dropped stale device id(s) for $CloudBaseUrl from $bootstrapState"
            } else {
                Write-Note "bootstrap state file holds no entries for $CloudBaseUrl -- nothing to prune"
            }
        } catch {
            # Never fatal: a malformed state file is dev-bootstrap's problem to
            # report, and this reset has already done its destructive work.
            Write-Note "could not prune $bootstrapState ($($_.Exception.Message)) -- dev-bootstrap will enroll fresh anyway"
        }
    }
}

# `go run ./cmd/devseed` applies every contract migration (postgres.Migrate)
# against the now-empty schema, then seeds cloud fixtures -- the same call
# scripts\dev-bootstrap.ps1 makes, reusing its KEY=VALUE parsing convention
# rather than a second one.
$cloudValues = @{}
if ($WhatIf) {
    Write-Note "-WhatIf: would run 'go run ./cmd/devseed' in $repoRoot\backend"
} else {
    Write-Note "running backend devseed (applies migrations, then seeds)..."
    Push-Location (Join-Path $repoRoot "backend")
    $savedEnv2 = Save-CallerEnv -Names @("DATABASE_URL")
    try {
        $env:DATABASE_URL = $DatabaseUrl
        $seedOutput = go run ./cmd/devseed
        $devseedExit = $LASTEXITCODE
    } finally {
        Pop-Location
        Restore-CallerEnv -Saved $savedEnv2
    }
    if ($devseedExit -ne 0) {
        Fail-WithAction `
            "backend devseed failed (exit $devseedExit)." `
            "Check the Go output above. KNOWN DEFECT: a malformed tax_rule.id ('<uuid>-CGST') is rejected by Postgres as SQLSTATE 22P02 -- a parallel track owns that fix; if this is what failed, note it and stop rather than re-running blindly."
    }
    $inBlock = $false
    foreach ($line in $seedOutput) {
        Write-Host "       $line" -ForegroundColor DarkGray
        if ($line -eq "---HOLLER-DEVSEED---") { $inBlock = $true; continue }
        if ($line -eq "---END---") { $inBlock = $false; continue }
        if ($inBlock -and $line -match '^([A-Z_]+)=(.*)$') { $cloudValues[$Matches[1]] = $Matches[2] }
    }
    foreach ($required in @("HOLLER_OUTLET_ID", "HOLLER_SEED_EMAIL", "HOLLER_SEED_PASSWORD", "HOLLER_SEED_PASSWORD_HASH")) {
        if (-not $cloudValues.ContainsKey($required)) {
            Fail-WithAction `
                "backend devseed did not print $required." `
                "The edge devseed step below needs it; inspect the Go devseed output above for what changed."
        }
    }
    Write-Note "cloud fixtures seeded (outlet $($cloudValues['HOLLER_OUTLET_ID']))"
}

# =====================================================================
# 3/4 -- delete the edge data directory (both files, gap A6), re-seed
# =====================================================================
Write-Step 3 "resetting the edge database..."

if ($WhatIf) {
    Write-Note "-WhatIf: would delete $edgePlaintextPath and its -wal/-shm siblings FIRST, then $edgeSealedPath, then run edge devseed and assert no plaintext leftover remains"
} else {
    # ORDER IS LOAD-BEARING: THE PLAINTEXT LEFTOVER GOES FIRST, THE SEALED
    # FILE LAST. A reset that deleted the .enc first and then failed on the
    # plaintext left no sealed database and a stale leftover behind -- and the
    # edge's crash-recovery path reseals a leftover when no sealed file exists,
    # so the next POS start would have promoted a pre-reset database to the
    # live one. Deleting in this order means a failure at ANY point leaves the
    # sealed file intact and the outlet still openable.
    foreach ($f in @("$edgePlaintextPath-wal", "$edgePlaintextPath-shm", $edgePlaintextPath, $edgeSealedPath)) {
        if (Test-Path $f) {
            Write-Note "deleting $f"
            try {
                Remove-Item -Path $f -Force
            } catch {
                $stillSealed = if (Test-Path $edgeSealedPath) { "$edgeSealedPath is STILL PRESENT and the outlet can still be opened." } else { "$edgeSealedPath is already gone." }
                Fail-WithAction `
                    "could not delete $f -- $($_.Exception.Message). $stillSealed" `
                    "Something took a handle on the edge database after this script's preflight passed (a POS started mid-run is the likely one). Close it and re-run the whole script from the start."
            }
        }
    }
    if ((Test-Path $edgeSealedPath) -or (Test-Path $edgePlaintextPath)) {
        Fail-WithAction `
            "the edge data files still exist after deletion." `
            "Check file permissions on $EdgeDataDir; a locked handle (POS still running?) would explain this."
    }
    Write-Note "edge data directory clean"

    Write-Note "running edge devseed (sqlite migration 0035 applies here, at this clean bootstrap)..."
    Push-Location (Join-Path $repoRoot "edge\database")
    $savedEnv3 = Save-CallerEnv -Names @(
        "HOLLER_DB_KEY_HEX", "HOLLER_EDGE_DATA_DIR",
        "HOLLER_SEED_PASSWORD_HASH", "HOLLER_SEED_PASSWORD", "HOLLER_SEED_BILLING")
    try {
        $env:HOLLER_DB_KEY_HEX = $DbKeyHex
        $env:HOLLER_EDGE_DATA_DIR = $EdgeDataDir
        $env:HOLLER_SEED_PASSWORD_HASH = $cloudValues["HOLLER_SEED_PASSWORD_HASH"]
        $env:HOLLER_SEED_PASSWORD = $cloudValues["HOLLER_SEED_PASSWORD"]
        $env:HOLLER_SEED_BILLING = "1"
        cargo run --quiet --bin devseed
        $edgeSeedExit = $LASTEXITCODE
    } finally {
        Pop-Location
        Restore-CallerEnv -Saved $savedEnv3
    }
    if ($edgeSeedExit -ne 0) {
        Fail-WithAction `
            "edge devseed failed (exit $edgeSeedExit)." `
            "Check the cargo output above. If it names seed\demo-outlet.json, note that a parallel track owns that file's known tax_rule.id defect."
    }
    if (-not (Test-Path $edgeSealedPath)) {
        Fail-WithAction `
            "edge devseed reported success but $edgeSealedPath does not exist." `
            "HOLLER_EDGE_DATA_DIR may not match this script's -EdgeDataDir; inspect the cargo output above for the path it actually wrote."
    }
    # AFTER A RESET, A PLAINTEXT LEFTOVER MUST NOT EXIST -- and nothing else
    # in the system will ever say so. `recover_crash_leftovers` reseals a
    # leftover UNCONDITIONALLY when no sealed file is present (correctly: that
    # is a first-run crash whose committed rows must not be discarded, and the
    # T25 key check has no sealed file to verify against). That is exactly the
    # shape a half-finished reset leaves behind, so the reset is the only
    # place that can tell the two apart, and the only honest moment to check
    # is here -- immediately after a seed that is supposed to have sealed and
    # wiped.
    $strayLeftovers = @($edgePlaintextPath, "$edgePlaintextPath-wal", "$edgePlaintextPath-shm") |
        Where-Object { Test-Path $_ }
    if ($strayLeftovers.Count -gt 0) {
        Fail-WithAction `
            "the edge devseed sealed $edgeSealedPath but left a PLAINTEXT leftover behind: $($strayLeftovers -join ', ')." `
            "Do not start the POS until this is resolved: the crash-recovery path reseals a leftover when it finds one, so starting the till would reseal THAT file as the live database. Delete the leftover(s) by hand, confirm $edgeSealedPath is still present, and re-run this script."
    }
    Write-Note "edge database seeded and sealed at $edgeSealedPath, with no plaintext leftover"
}

# =====================================================================
# 4/4 -- assert the known-clean state, fail loudly on the first failure
# =====================================================================
Write-Step 4 "asserting the known-clean-state invariants..."

if ($WhatIf) {
    Write-Note "-WhatIf: would build and run scripts\demo-assert (cargo run --bin demo-assert -- `"$EdgeDataDir`") and check:"
    Write-Note "  - sync_replay_block: 0 rows"
    Write-Note "  - stock_deduction_gap: 0 rows (NOT grn_gap -- the seed's one NO_PURCHASE_ORDER row there is expected, ADR-019)"
    Write-Note "  - sync_outbox_block blocked rows: 0 (this is what 'zero blocked rows in local_outbox' queries)"
    Write-Note "  - sync_outbox_block persistently-failing rows: 0 (PROXY for the sync banner -- not a screenshot of it)"
} else {
    $assertDir = Join-Path $repoRoot "scripts\demo-assert"
    Push-Location $assertDir
    $savedEnv4 = Save-CallerEnv -Names @("HOLLER_DB_KEY_HEX")
    try {
        $env:HOLLER_DB_KEY_HEX = $DbKeyHex
        $assertOutput = cargo run --quiet --bin demo-assert -- "$EdgeDataDir" 2>&1
        $assertExit = $LASTEXITCODE
    } finally {
        Pop-Location
        Restore-CallerEnv -Saved $savedEnv4
    }

    # Report each assertion BY NAME with its actual count, pass or fail --
    # "all checks passed" alone is not a result. demo-assert already prints
    # this format ("<name>: <n> rows -- OK/FAIL"); relayed verbatim here so
    # this script's own transcript carries it, not only stdout from a
    # subprocess a reader might not scroll back to.
    foreach ($line in $assertOutput) {
        $ok = $line -match "-- OK$" -or $line -notmatch "-- FAIL$"
        Write-Assert $ok $line
    }

    if ($assertExit -ne 0) {
        $nextAction = if ($assertExit -eq 1) {
            "demo-assert could not even open the sealed database (see the line above) -- this is a setup failure, not a failed assertion. Confirm HOLLER_DB_KEY_HEX and that step 3 actually wrote $edgeSealedPath."
        } else {
            "One or more of the four invariants is non-zero (see the FAIL lines above). Do not proceed with the demo on this state -- investigate the specific row(s) named, then re-run this whole script rather than patching around it."
        }
        Fail-WithAction "the known-clean-state assertions did not all pass (demo-assert exit $assertExit)." $nextAction
    }
    Write-Note "REMINDER: assertion 4 (sync banner) is a DATABASE PROXY, not an observation of the POS screen. It has not been looked at in a browser by this script."
}

Write-Host ""
Write-Host "demo reset complete." -ForegroundColor Green
Write-Host "elapsed: $((Get-Date) - $startedAt)"
if (-not $WhatIf) {
    Write-Host "login: $($cloudValues['HOLLER_SEED_EMAIL']) / $($cloudValues['HOLLER_SEED_PASSWORD'])"
    Write-Host "edge database: $edgeSealedPath"
}
Write-Host "Launch the stack per docs\DEV_SETUP.md / scripts\dev-up.ps1 -SkipSeed (seeding already done above)." -ForegroundColor Cyan
