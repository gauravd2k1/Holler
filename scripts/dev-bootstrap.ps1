# Holler development bootstrap.
#
# DEVELOPMENT ONLY. Nothing here runs at an outlet (ADR-013): outlet machines
# have no Docker, no Postgres and no developer toolchain.
#
# Brings a fresh clone to the point where the POS can log in offline:
#   1. starts the cloud infra containers (Postgres/Redis/NATS)
#   2. applies the frozen Postgres migrations and seeds cloud fixtures
#   3. seeds the encrypted edge SQLite database the POS reads (now including
#      a KDS device row and a kitchen station -- T12)
#   4. writes apps\pos\.env.dev and apps\kds\.env.dev
#
# Re-running is safe: both seeders upsert against fixed development ids.
#
# See docs/DEV_SETUP.md for the full sequence including running the frontend
# and the KDS LAN server, and the item-1 two-machine runbook.

[CmdletBinding()]
param(
    # 32-byte key, hex-encoded, for the edge database's encryption at rest
    # (ADR-011). THERE IS DELIBERATELY NO DEFAULT. A default here is consent by
    # omission: an outlet ships with an encrypted SQLite that anyone who read
    # this repository can decrypt, because nobody changed it -- and nothing in
    # the install path would ever say so. Supply it per-machine instead, either
    # as -DbKeyHex or via the HOLLER_DB_KEY_HEX environment variable; the
    # script refuses to run without one. See docs/DEV_SETUP.md for how to mint
    # one, and keep it out of the repository.
    [string]$DbKeyHex = "",

    # Where the POS keeps its edge database. Must match Tauri's app_data_dir()
    # for identifier com.holler.pos, or the POS will open a different (empty)
    # database than the one this script seeds.
    [string]$EdgeDataDir = (Join-Path $env:APPDATA "com.holler.pos"),

    # Seeded KDS device row (edge/database/src/bin/devseed.rs KDS_DEVICE_ID).
    # Fixed like every other devseed id -- re-stated here rather than parsed
    # out of the Rust seeder's output because it prints nothing today.
    [string]$KdsDeviceId = "0191a000-0000-7000-8000-00000000000d",

    # Bind port for the KDS LAN server, embedded in the POS
    # (apps/pos/src-tauri/src/state.rs::DEFAULT_LAN_BIND_ADDR) or run
    # standalone (edge/device/src/bin/kds_lan_server.rs). Pinned to 9310.
    [int]$LanPort = 9310,

    # Skip "docker compose up" if the containers are already running.
    [switch]$SkipInfra,

    # Seed the billing config a bill needs before one can be issued: tax
    # profile + rules, outlet fiscal profile (GSTIN), an active SALES series,
    # three discount definitions, and two printers with printer_role rows.
    # OFF by default because tests/e2e-scenario seeds its own and would end
    # up with two active SALES series (see devseed.rs's own note).
    #
    # Without this, the POS reaches "Issue Bill" and fails with
    # NO_FISCAL_PROFILE_CONFIGURED -- which is correct behaviour, just not a
    # runnable acceptance path.
    [switch]$WithBilling,

    # Write every print to this directory as a file instead of sending it to
    # a device (HOLLER_PRINTER_FILE_SINK_DIR). This is how a machine with no
    # thermal printer attached can still verify the real ESC/POS byte stream:
    # same renderer, same spool, same transport boundary -- only the final
    # write lands somewhere you can open. See
    # edge/printer/src/transport/file_sink.rs for what that does and does not
    # establish (it establishes nothing about real device I/O).
    [string]$PrinterFileSinkDir = ""
)

# Best-effort LAN IPv4 address for this machine, used to build
# apps/kds/.env.dev's VITE_KDS_LAN_URL -- a second machine must reach this
# over the LAN, so localhost/127.0.0.1 is never right here. Picks the first
# non-loopback, non-link-local (169.254.x.x) IPv4 address; on a machine with
# several NICs this may not be the one a KDS device is actually on, so
# DEV_SETUP.md tells the reader to double-check it against `ipconfig`.
function Get-LanIPv4 {
    $candidates = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object { $_.IPAddress -ne "127.0.0.1" -and $_.IPAddress -notlike "169.254.*" }
    if ($candidates) { return ($candidates | Select-Object -First 1).IPAddress }
    return "127.0.0.1"
}

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

# --- 0. the edge database key ------------------------------------------------
# Fail here, before any container starts, rather than letting a seeder deeper in
# the run produce a database under a key the operator never chose. A default was
# removed from this parameter deliberately; do not reinstate one "just for dev".
# Dev keys become outlet keys the moment nobody has to type one.
if ([string]::IsNullOrWhiteSpace($DbKeyHex)) {
    $DbKeyHex = $env:HOLLER_DB_KEY_HEX
}
if ([string]::IsNullOrWhiteSpace($DbKeyHex)) {
    throw @"
HOLLER_DB_KEY_HEX is not set and -DbKeyHex was not supplied.

This script has no default key on purpose. A hardcoded default means an edge
database can be encrypted with a key published in this repository, and nothing
downstream would report it.

Mint one for this machine and keep it out of the repository:

    `$env:HOLLER_DB_KEY_HEX = -join ((1..32) | ForEach-Object { '{0:x2}' -f (Get-Random -Max 256) })

Then re-run this script. Use the SAME value every time on this machine -- a
different key opens a different (empty) database, not an error.
"@
}
if ($DbKeyHex -notmatch '^[0-9a-fA-F]{64}$') {
    throw "HOLLER_DB_KEY_HEX must be exactly 64 hex characters (32 bytes); got $($DbKeyHex.Length) character(s)."
}

Write-Host "Holler dev bootstrap" -ForegroundColor Cyan
Write-Host "repo: $repoRoot"

# --- 1. infrastructure -------------------------------------------------------
# Only postgres/redis/nats. The `backend` compose service is deliberately NOT
# started: its Dockerfile build is broken (see docs/DEV_SETUP.md, Known gaps),
# and the backend is run natively instead.
if (-not $SkipInfra) {
    Write-Host "`n[1/4] starting infra containers (postgres, redis, nats)..." -ForegroundColor Cyan
    Push-Location $repoRoot
    try {
        docker compose up -d postgres redis nats
        if ($LASTEXITCODE -ne 0) { throw "docker compose up failed" }
    } finally {
        Pop-Location
    }

    # Postgres declares a healthcheck; wait for it rather than racing the seeder.
    Write-Host "waiting for postgres to report healthy..."
    $deadline = (Get-Date).AddSeconds(60)
    do {
        $state = (docker inspect --format '{{.State.Health.Status}}' holler-postgres-1 2>$null)
        if ($state -eq "healthy") { break }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    if ($state -ne "healthy") { throw "postgres did not become healthy within 60s" }
} else {
    Write-Host "`n[1/4] skipping infra startup (-SkipInfra)" -ForegroundColor Yellow
}

# --- 2. cloud migrations + seed ---------------------------------------------
Write-Host "`n[2/4] applying Postgres migrations and seeding cloud fixtures..." -ForegroundColor Cyan
Push-Location (Join-Path $repoRoot "backend")
try {
    $seedOutput = go run ./cmd/devseed
    if ($LASTEXITCODE -ne 0) { throw "backend devseed failed" }
} finally {
    Pop-Location
}

# The Go seeder prints a KEY=VALUE block delimited by ---HOLLER-DEVSEED---.
$values = @{}
$inBlock = $false
foreach ($line in $seedOutput) {
    if ($line -eq "---HOLLER-DEVSEED---") { $inBlock = $true; continue }
    if ($line -eq "---END---") { $inBlock = $false; continue }
    if ($inBlock -and $line -match '^([A-Z_]+)=(.*)$') {
        $values[$Matches[1]] = $Matches[2]
    }
}

foreach ($required in @("HOLLER_OUTLET_ID", "HOLLER_DEVICE_ID", "HOLLER_SEED_EMAIL",
                        "HOLLER_SEED_PASSWORD", "HOLLER_SEED_PASSWORD_HASH")) {
    if (-not $values.ContainsKey($required)) {
        throw "backend devseed did not print $required - cannot seed the edge database"
    }
}

# --- 3. edge seed ------------------------------------------------------------
# The edge SQLite file gets its SCHEMA automatically on first open, but never
# its DATA: the cloud-to-edge config pull exists in edge/sync and is not wired
# into the POS. This step stands in for it.
Write-Host "`n[3/4] seeding the encrypted edge database..." -ForegroundColor Cyan
Push-Location (Join-Path $repoRoot "edge\database")
try {
    $env:HOLLER_DB_KEY_HEX = $DbKeyHex
    $env:HOLLER_EDGE_DATA_DIR = $EdgeDataDir
    $env:HOLLER_SEED_PASSWORD_HASH = $values["HOLLER_SEED_PASSWORD_HASH"]
    # Setting the plaintext password makes the seeder re-open the sealed file
    # and prove the offline-login path works before we claim success.
    $env:HOLLER_SEED_PASSWORD = $values["HOLLER_SEED_PASSWORD"]
    if ($WithBilling) { $env:HOLLER_SEED_BILLING = "1" }

    cargo run --quiet --bin devseed
    if ($LASTEXITCODE -ne 0) { throw "edge devseed failed" }
} finally {
    Pop-Location
    Remove-Item Env:\HOLLER_SEED_PASSWORD -ErrorAction SilentlyContinue
    Remove-Item Env:\HOLLER_SEED_PASSWORD_HASH -ErrorAction SilentlyContinue
    Remove-Item Env:\HOLLER_SEED_BILLING -ErrorAction SilentlyContinue
}

# --- 4. env files for the launchers ------------------------------------------
# apps/pos/run-dev.ps1 reads this instead of hardcoding device identity and the
# encryption key. Gitignored: it carries the edge database key.
$envFile = Join-Path $repoRoot "apps\pos\.env.dev"
$envLines = @(
    "# Generated by scripts/dev-bootstrap.ps1. DO NOT COMMIT.",
    "# Regenerate by re-running the bootstrap; see apps/pos/.env.dev.example.",
    "HOLLER_OUTLET_ID=$($values['HOLLER_OUTLET_ID'])",
    "HOLLER_DEVICE_ID=$($values['HOLLER_DEVICE_ID'])",
    "HOLLER_DB_KEY_HEX=$DbKeyHex",
    "HOLLER_LAN_BIND_ADDR=0.0.0.0:$LanPort"
)
# run-dev.ps1 exports every KEY=VALUE it finds here into the POS process, so
# naming the sink in this file is all it takes to route prints to disk.
if ($PrinterFileSinkDir -ne "") {
    $resolvedSink = [System.IO.Path]::GetFullPath($PrinterFileSinkDir)
    New-Item -ItemType Directory -Force -Path $resolvedSink | Out-Null
    $envLines += "HOLLER_PRINTER_FILE_SINK_DIR=$resolvedSink"
}
# ASCII so Windows PowerShell 5.1 reads it back without a BOM surprise.
$envLines | Out-File -FilePath $envFile -Encoding ascii
Write-Host "`n[4/4] wrote $envFile" -ForegroundColor Cyan
if ($WithBilling) {
    Write-Host "billing config seeded: bills can be issued, discounted and split on this machine." -ForegroundColor Cyan
}
if ($PrinterFileSinkDir -ne "") {
    Write-Host "printer FILE SINK: every print will be written to $resolvedSink" -ForegroundColor Yellow
    Write-Host "  .escpos = the real byte stream sent to the transport; .txt = the same bill with escapes stripped, for reading." -ForegroundColor Yellow
    Write-Host "  This proves the render and the spool. It proves NOTHING about a real 58/80mm printer." -ForegroundColor Yellow
}

# apps/kds/.env.dev (T12). Vite does NOT load this by itself -- it is read
# only with `--mode dev`, which every documented KDS launch command below
# passes; see apps/kds/.env.dev.example for why the name does not change
# Vite's default-mode behaviour.
$lanIp = Get-LanIPv4
$kdsEnvFile = Join-Path $repoRoot "apps\kds\.env.dev"
$kdsEnvLines = @(
    "# Generated by scripts/dev-bootstrap.ps1. DO NOT COMMIT.",
    "# Regenerate by re-running the bootstrap; see apps/kds/.env.dev.example.",
    "# Read only with --mode dev (Vite's default mode is 'development', not 'dev').",
    "VITE_KDS_LAN_URL=ws://${lanIp}:${LanPort}/kds",
    "VITE_KDS_OUTLET_ID=$($values['HOLLER_OUTLET_ID'])",
    "VITE_KDS_DEVICE_ID=$KdsDeviceId"
)
$kdsEnvLines | Out-File -FilePath $kdsEnvFile -Encoding ascii
Write-Host "wrote $kdsEnvFile (LAN URL host detected as $lanIp -- verify with ipconfig if this machine has more than one network adapter)" -ForegroundColor Cyan

Write-Host "`nready." -ForegroundColor Green
Write-Host "`nLaunch the POS with:" -ForegroundColor Cyan
Write-Host "  cd apps\pos; pnpm dev        # terminal 1 (Vite)"
Write-Host "  .\apps\pos\run-dev.ps1       # terminal 2 (reads .env.dev, also starts the KDS LAN server)"
Write-Host "`nLaunch the KDS (on this machine or a second one on the same LAN):" -ForegroundColor Cyan
Write-Host "  cd apps\kds; pnpm install; pnpm dev --host 0.0.0.0 --mode dev"
Write-Host "  then open http://${lanIp}:5174 from the KDS device"
Write-Host "`nLogin:" -ForegroundColor Cyan
Write-Host "  email:    $($values['HOLLER_SEED_EMAIL'])"
Write-Host "  password: $($values['HOLLER_SEED_PASSWORD'])"
Write-Host "`nEdge database: $EdgeDataDir\edge.db.enc"
Write-Host "Details: docs/DEV_SETUP.md"
