# Quick-launch the POS against a bootstrapped development environment.
#
# DEVELOPMENT ONLY (ADR-013: outlet machines have no toolchain).
#
# Reads device identity and the edge encryption key from apps/pos/.env.dev,
# which scripts\dev-bootstrap.ps1 writes at the end of its run. That file is
# gitignored because it carries a database encryption key; .env.dev.example
# documents its shape.
#
# T12: this is also how the KDS LAN server starts in development. The POS
# process embeds it (apps/pos/src-tauri/src/state.rs::AppState::open calls
# holler_edge_device::server::start over the POS's own Arc<Mutex<Db>>) rather
# than this script launching a second OS process for it -- chosen because
# every KOT-notification call site (commands/kitchen.rs) lives in the same
# process that mutates kot state, and the wire protocol has no message for
# "another process changed something, please rebroadcast", so the process
# that writes kot state and the process holding the Hub must be the same one.
# `edge/device`'s standalone `kds-lan-server` bin still exists (for
# connectivity testing without a full POS build) but must never run at the
# same time as the POS against the same edge.db.enc -- see docs/DEV_SETUP.md.
#
# Usage (from anywhere):
#   .\apps\pos\run-dev.ps1
#
# See docs/DEV_SETUP.md.

[CmdletBinding()]
param(
    # Alternate env file, e.g. to run a second till against another outlet.
    [string]$EnvFile = (Join-Path $PSScriptRoot ".env.dev"),

    # Suppress the informational note about an already-running Vite server.
    # No longer skips a gate -- `tauri dev` starts Vite itself now.
    [switch]$SkipViteCheck,

    # Run the RELEASE binary that is already built, instead of `tauri dev`.
    #
    # This is the build a demo and a firewall rule point at: the rule names an
    # exact program path, and target\debug is not that path, so a rehearsal run
    # through `tauri dev` proves nothing about the inbound rule the phone
    # depends on (docs/demo-script.md 0.0b).
    #
    # No Vite, and no 5173 guard: the release binary serves its own embedded
    # frontend, so a dev server running beside it is simply unrelated rather
    # than a conflict.
    [switch]$Release
)

$ErrorActionPreference = "Stop"

# STRUCTURAL GUARD. Starting the POS binds the LAN server (9310), the captain
# listener (9320) and Vite (5173), and it opens the operator's edge database
# with the real key from .env.dev. No scratch equivalent, so an agent shell is
# refused outright. See scripts\agent-guard.ps1.
. (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "scripts\agent-guard.ps1")
Assert-NotAgentShell -ScriptName "apps\pos\run-dev.ps1" -Ports "9310, 9320, 5173"

if (-not (Test-Path $EnvFile)) {
    throw @"
No env file at $EnvFile.

Run the bootstrap first, which seeds the databases and writes it:
    .\scripts\dev-bootstrap.ps1

See docs/DEV_SETUP.md.
"@
}

# Parse KEY=VALUE. Blank lines and # comments are ignored; values are taken
# verbatim (no quote stripping) because the key is hex and the ids are UUIDs.
$required = @("HOLLER_OUTLET_ID", "HOLLER_DEVICE_ID", "HOLLER_DB_KEY_HEX")
$loaded = @{}

foreach ($line in Get-Content $EnvFile) {
    $trimmed = $line.Trim()
    if ($trimmed -eq "" -or $trimmed.StartsWith("#")) { continue }
    if ($trimmed -match '^([A-Za-z_][A-Za-z0-9_]*)=(.*)$') {
        $name = $Matches[1]
        $value = $Matches[2].Trim()
        Set-Item -Path "Env:\$name" -Value $value
        $loaded[$name] = $value
    }
}

$missing = $required | Where-Object { -not $loaded.ContainsKey($_) -or $loaded[$_] -eq "" }
if ($missing) {
    throw "$EnvFile is missing: $($missing -join ', '). Re-run .\scripts\dev-bootstrap.ps1."
}

# The POS panics on a key that is not 32 bytes of hex; catch it here with a
# better message than a Rust panic in a GUI process.
if ($loaded["HOLLER_DB_KEY_HEX"].Length -ne 64) {
    throw "HOLLER_DB_KEY_HEX must be exactly 64 hex characters (32 bytes); got $($loaded['HOLLER_DB_KEY_HEX'].Length)."
}

# THIS SCRIPT IS THE ONLY WAY TO START THE POS, AND IT STARTS VITE ITSELF.
# tauri.conf.json's beforeDevCommand is `pnpm dev`, so `tauri dev` ALWAYS runs
# Vite. There is no second terminal, and starting one by hand first is what
# breaks the launch.
#
# THE OLD NOTE HERE WAS FALSE IN BOTH DIRECTIONS, and cost an evening. It said
# `tauri dev` would "not start its own" Vite when one was already serving 5173
# -- it always starts one -- and it treated the situation as a warning worth
# printing rather than a problem worth stopping for. What actually happens with
# `strictPort: true` (apps/pos/vite.config.ts) is that the second Vite FAILS on
# the taken port. And when a stale Vite does end up serving the window, it
# serves a bundle built in a different environment: on 2026-09-12 that is
# exactly why the UPI QR was missing from the bill screen while .env.dev
# carried the payee -- the serving Vite had no VITE_ variables in it at all.
#
# So: refuse, name the pid, and say what to do. -SkipViteCheck still forces a
# launch, for the case where the operator knows the running server is the right
# one; the warning it prints says what they are accepting.
if (-not $SkipViteCheck -and -not $Release) {
    $viteHolder = $null
    $conn = Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue
    if ($conn) { $viteHolder = $conn.OwningProcess | Select-Object -First 1 }

    if ($viteHolder) {
        $proc = Get-Process -Id $viteHolder -ErrorAction SilentlyContinue
        $desc = if ($proc) { "$($proc.ProcessName) pid $viteHolder, started $($proc.StartTime)" } else { "pid $viteHolder" }
        Write-Host ""
        Write-Host "REFUSED: something is already serving http://localhost:5173 ($desc)." -ForegroundColor Red
        Write-Host "  This script starts Vite itself (tauri.conf.json beforeDevCommand), and" -ForegroundColor Red
        Write-Host "  vite.config.ts sets strictPort, so a second one cannot start." -ForegroundColor Red
        Write-Host "  NOTHING WAS STARTED." -ForegroundColor Red
        Write-Host ""
        Write-Host "  Stop it and re-run:" -ForegroundColor Yellow
        Write-Host "    Stop-Process -Id $viteHolder" -ForegroundColor Yellow
        Write-Host "  A Vite left from an earlier shell serves a bundle built WITHOUT the" -ForegroundColor Yellow
        Write-Host "  current .env.local -- that is how the UPI QR went missing on 2026-09-12." -ForegroundColor Yellow
        Write-Host "  -SkipViteCheck forces a launch against the running server if you are sure." -ForegroundColor DarkGray
        exit 1
    }
}

# --- -Release: the binary must exist, and must be NEWER than the frontend ----
# A release binary embeds apps\pos\dist at COMPILE time (tauri.conf.json's
# frontendDist). So a dist rebuilt after the last link is a window showing the
# previous UI with nothing anywhere saying so -- the exact shape of the stale
# -Vite incident this script already refuses for (see the 5173 block below),
# one build profile over. Refuse rather than warn: a demo rehearsal that
# silently exercises last night's screens is worse than one that does not start.
$releaseExe = Join-Path $PSScriptRoot "src-tauri\target\release\holler-pos.exe"
if ($Release) {
    if (-not (Test-Path $releaseExe)) {
        throw @"
-Release was given but no release binary exists at:
    $releaseExe

Build it with the TAURI CLI, which is not the same as cargo:
    cd $PSScriptRoot\..\captain; pnpm build
    cd $PSScriptRoot; pnpm exec tauri build --no-bundle

NOT ``cargo build --release``. That produces a DEV-MODE binary in the release
profile: the window loads http://localhost:5173 instead of the UI compiled
into it, and shows "can't reach this page" with no dev server running. Only
the Tauri CLI sets the environment that embeds the frontend. (2026-09-15.)
"@
    }

    $exeTime = (Get-Item $releaseExe).LastWriteTime
    $distDir = Join-Path $PSScriptRoot "dist"
    if (Test-Path $distDir) {
        $newestDist = Get-ChildItem $distDir -Recurse -File |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1
        if ($newestDist -and $newestDist.LastWriteTime -gt $exeTime) {
            throw @"
The release binary is OLDER than apps\pos\dist, so it embeds a previous frontend.

    binary : $exeTime  $releaseExe
    dist   : $($newestDist.LastWriteTime)  $($newestDist.FullName)

The POS frontend is embedded at compile time, so rebuilding dist alone changes
nothing in the window. Re-link:
    cd $PSScriptRoot; pnpm exec tauri build --no-bundle

(apps\captain\dist is NOT checked here and does not need to be: the captain
page is served from disk at request time, so a captain rebuild takes effect
without a relink.)
"@
        }
    }

    # AND THE CHECK THAT MATTERS: is the UI actually INSIDE the binary?
    #
    # Everything above this point is a check on the FILE -- that it exists and
    # that it is newer than dist. Both pass on a binary that cannot draw a
    # window, which is exactly what shipped on 2026-09-14: `build : RELEASE`
    # printed, correct path, correct hash, and a window reading "can't reach
    # this page -- localhost refused to connect". Existence and identity
    # checks standing in for a function check.
    $checker = Join-Path $PSScriptRoot "..\..\scripts\check-release-binary.ps1"
    if (Test-Path $checker) {
        & $checker -RepoRoot (Resolve-Path (Join-Path $PSScriptRoot "..\..")) -Quiet
        if ($LASTEXITCODE -ne 0) {
            throw "the release binary does not carry the UI -- see the refusal above. Nothing was launched."
        }
    } else {
        Write-Host "WARNING: scripts\check-release-binary.ps1 is missing -- the embedded-UI check did NOT run." -ForegroundColor Red
    }
}

$lanAddr = if ($loaded.ContainsKey('HOLLER_LAN_BIND_ADDR')) { $loaded['HOLLER_LAN_BIND_ADDR'] } else { "0.0.0.0:9310 (default)" }

Write-Host "outlet : $($loaded['HOLLER_OUTLET_ID'])"
Write-Host "device : $($loaded['HOLLER_DEVICE_ID'])"
Write-Host "env    : $EnvFile"
Write-Host "KDS LAN server will bind $lanAddr on this machine (unauthenticated -- see docs/DEV_SETUP.md)"
if ($loaded.ContainsKey('HOLLER_PRINTER_FILE_SINK_DIR') -and $loaded['HOLLER_PRINTER_FILE_SINK_DIR'] -ne "") {
    Write-Host "printer: FILE SINK ACTIVE -- prints go to $($loaded['HOLLER_PRINTER_FILE_SINK_DIR']), not to any device." -ForegroundColor Yellow
}
if ($Release) {
    Write-Host "build  : RELEASE -- $releaseExe" -ForegroundColor Cyan
} else {
    Write-Host "build  : DEBUG (tauri dev, Vite on 5173)" -ForegroundColor DarkGray
}
Write-Host ""

if ($Release) {
    & $releaseExe
} else {
    Push-Location $PSScriptRoot
    try {
        pnpm exec tauri dev
    } finally {
        Pop-Location
    }
}
