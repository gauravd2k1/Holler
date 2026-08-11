# Holler development bootstrap.
#
# DEVELOPMENT ONLY. Nothing here runs at an outlet (ADR-013): outlet machines
# have no Docker, no Postgres and no developer toolchain.
#
# Brings a fresh clone to the point where the POS can log in offline:
#   1. starts the cloud infra containers (Postgres/Redis/NATS)
#   2. applies the frozen Postgres migrations and seeds cloud fixtures
#   3. seeds the encrypted edge SQLite database the POS reads (now including
#      a KDS device row and a kitchen station — T12)
#   4. writes apps\pos\.env.dev and apps\kds\.env.dev
#
# Re-running is safe: both seeders upsert against fixed development ids.
#
# See docs/DEV_SETUP.md for the full sequence including running the frontend
# and the KDS LAN server, and the item-1 two-machine runbook.

[CmdletBinding()]
param(
    # 32-byte key, hex-encoded, for the edge database's encryption at rest.
    # A fixed default keeps re-runs reproducible. It is a DEVELOPMENT key and
    # must never be used for anything holding real data (ADR-011).
    [string]$DbKeyHex = "5ff0c2a1b93d4e6f8a7c1d2e3f405162738495a6b7c8d9eafb0c1d2e3f405162",

    # Where the POS keeps its edge database. Must match Tauri's app_data_dir()
    # for identifier com.holler.pos, or the POS will open a different (empty)
    # database than the one this script seeds.
    [string]$EdgeDataDir = (Join-Path $env:APPDATA "com.holler.pos"),

    # Seeded KDS device row (edge/database/src/bin/devseed.rs KDS_DEVICE_ID).
    # Fixed like every other devseed id — re-stated here rather than parsed
    # out of the Rust seeder's output because it prints nothing today.
    [string]$KdsDeviceId = "0191a000-0000-7000-8000-00000000000d",

    # Bind port for the KDS LAN server, embedded in the POS
    # (apps/pos/src-tauri/src/state.rs::DEFAULT_LAN_BIND_ADDR) or run
    # standalone (edge/device/src/bin/kds_lan_server.rs). Pinned to 9310.
    [int]$LanPort = 9310,

    # Skip "docker compose up" if the containers are already running.
    [switch]$SkipInfra
)

# Best-effort LAN IPv4 address for this machine, used to build
# apps/kds/.env.dev's VITE_KDS_LAN_URL — a second machine must reach this
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

    cargo run --quiet --bin devseed
    if ($LASTEXITCODE -ne 0) { throw "edge devseed failed" }
} finally {
    Pop-Location
    Remove-Item Env:\HOLLER_SEED_PASSWORD -ErrorAction SilentlyContinue
    Remove-Item Env:\HOLLER_SEED_PASSWORD_HASH -ErrorAction SilentlyContinue
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
# ASCII so Windows PowerShell 5.1 reads it back without a BOM surprise.
$envLines | Out-File -FilePath $envFile -Encoding ascii
Write-Host "`n[4/4] wrote $envFile" -ForegroundColor Cyan

# apps/kds/.env.dev (T12). Vite does NOT load this by itself — it is read
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
