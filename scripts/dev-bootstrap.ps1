# Holler development bootstrap.
#
# DEVELOPMENT ONLY. Nothing here runs at an outlet (ADR-013): outlet machines
# have no Docker, no Postgres and no developer toolchain.
#
# Brings a fresh clone to the point where the POS can log in offline:
#   1. starts the cloud infra containers (Postgres/Redis/NATS)
#   2. applies the frozen Postgres migrations and seeds cloud fixtures
#   3. seeds the encrypted edge SQLite database the POS reads
#   4. prints the environment variables the POS needs at startup
#
# Re-running is safe: both seeders upsert against fixed development ids.
#
# See docs/DEV_SETUP.md for the full sequence including running the frontend.

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

    # Skip "docker compose up" if the containers are already running.
    [switch]$SkipInfra
)

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

# --- 4. what to do next ------------------------------------------------------
Write-Host "`n[4/4] ready." -ForegroundColor Green
Write-Host "`nSet these in the shell that runs 'pnpm exec tauri dev':`n" -ForegroundColor Cyan
Write-Host "`$env:HOLLER_OUTLET_ID   = `"$($values['HOLLER_OUTLET_ID'])`""
Write-Host "`$env:HOLLER_DEVICE_ID   = `"$($values['HOLLER_DEVICE_ID'])`""
Write-Host "`$env:HOLLER_DB_KEY_HEX  = `"$DbKeyHex`""
Write-Host "`nLogin:" -ForegroundColor Cyan
Write-Host "  email:    $($values['HOLLER_SEED_EMAIL'])"
Write-Host "  password: $($values['HOLLER_SEED_PASSWORD'])"
Write-Host "`nEdge database: $EdgeDataDir\edge.db.enc"
Write-Host "Next: see docs/DEV_SETUP.md step 4 (run vite, then tauri dev)."
