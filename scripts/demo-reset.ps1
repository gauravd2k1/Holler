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

    # Must match Tauri's app_data_dir() for com.holler.pos -- same default
    # dev-bootstrap.ps1 uses, and it must agree with whatever apps\pos\.env.dev
    # was provisioned against.
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
$repoRoot = Split-Path -Parent $PSScriptRoot
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
Write-Host "  - $edgeSealedPath (the encrypted edge database)" -ForegroundColor Yellow
Write-Host "  - $edgePlaintextPath, if present (gap A6's leftover plaintext copy)" -ForegroundColor Yellow
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
    try {
        $env:DATABASE_URL = $DatabaseUrl
        $seedOutput = go run ./cmd/devseed
        $devseedExit = $LASTEXITCODE
    } finally {
        Pop-Location
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
    Write-Note "-WhatIf: would delete $edgeSealedPath and $edgePlaintextPath (and any -wal/-shm siblings), then run edge devseed"
} else {
    foreach ($f in @($edgeSealedPath, $edgePlaintextPath, "$edgePlaintextPath-wal", "$edgePlaintextPath-shm")) {
        if (Test-Path $f) {
            Write-Note "deleting $f"
            Remove-Item -Path $f -Force
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
        Remove-Item Env:\HOLLER_SEED_PASSWORD -ErrorAction SilentlyContinue
        Remove-Item Env:\HOLLER_SEED_PASSWORD_HASH -ErrorAction SilentlyContinue
        Remove-Item Env:\HOLLER_SEED_BILLING -ErrorAction SilentlyContinue
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
    Write-Note "edge database seeded and sealed at $edgeSealedPath"
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
    try {
        $env:HOLLER_DB_KEY_HEX = $DbKeyHex
        $assertOutput = cargo run --quiet --bin demo-assert -- "$EdgeDataDir" 2>&1
        $assertExit = $LASTEXITCODE
    } finally {
        Pop-Location
        Remove-Item Env:\HOLLER_DB_KEY_HEX -ErrorAction SilentlyContinue
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
