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
    [switch]$SkipViteCheck
)

$ErrorActionPreference = "Stop"

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

# tauri.conf.json's beforeDevCommand is now `pnpm dev`, so `tauri dev` starts
# Vite itself and this check no longer gates anything -- it is informational.
#
# It still earns its place: if Vite is ALREADY answering on 5173, `tauri dev`
# will start a second one, Vite will fall back to 5174 (or fail under
# strictPort), and the window then loads whatever the FIRST server is serving --
# possibly a stale build from another branch. That is confusing enough to be
# worth a line of output, but not worth refusing to launch over.
if (-not $SkipViteCheck) {
    $viteAlreadyUp = $false
    try {
        $null = Invoke-WebRequest -Uri "http://localhost:5173" -UseBasicParsing -TimeoutSec 3
        $viteAlreadyUp = $true
    } catch {
        $viteAlreadyUp = $false
    }
    if ($viteAlreadyUp) {
        Write-Host "note   : Vite is already serving http://localhost:5173 -- tauri dev will not start its own." -ForegroundColor Yellow
        Write-Host "         The window loads THAT server. Stop it first if you want a clean one." -ForegroundColor Yellow
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
Write-Host ""

Push-Location $PSScriptRoot
try {
    pnpm exec tauri dev
} finally {
    Pop-Location
}
