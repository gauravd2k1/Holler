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
    [string]$PrinterFileSinkDir = "",

    # Where the POS sync worker sends replays (ADR-020). Also the API this
    # script enrols this till against. Never fatal if nothing is listening.
    [string]$CloudBaseUrl = "http://localhost:8080",

    # The principal used to enrol. POST /devices/enroll is gated on
    # outlet.manage, which the seeded cashier and buyer deliberately do NOT
    # hold: outlet.manage also gates the compliance config writes, so granting
    # it to a till operator would hand them the GSTIN printed on every invoice.
    [string]$SyncEnrollEmail = "owner@holler.test",
    [string]$SyncEnrollPassword = "holler123"
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

# --- device enrollment helpers (T15) -----------------------------------------
# Shared by the POS's own sync credential (3b) and the KDS credential (3c).
#
# Device lookup for the rotate-vs-enroll decision. `POST /devices/enroll`
# matches an existing device by (tenant, outlet, name) and 409s if one already
# exists, and there is deliberately no device LIST route (device_http.go),
# so "does a device by this name already exist" can only be answered two
# ways: a direct Postgres lookup (only possible when the cloud IS the local
# Docker stack this script itself started), or remembering an id this script
# minted on a previous run.
#
# T15 defect: the previous version always tried the Postgres lookup via
# `docker exec holler-postgres-1`, unconditionally -- correct only when
# -CloudBaseUrl is the local stack. Pointed at a second machine (the
# documented two-machine demo setup, docs/lan-setup.md section 7), that
# `docker exec` finds a container that was never asked about THIS cloud, so
# the lookup silently returns nothing and the script takes the enroll branch
# every time, hitting 409 on every re-run instead of rotating. Detect which
# case this is instead of guessing.
function Test-CloudBaseUrlIsLocal {
    param([string]$CloudBaseUrl)
    try {
        $targetHost = ([Uri]$CloudBaseUrl).Host
    } catch {
        return $false
    }
    return ($targetHost -eq "localhost" -or $targetHost -eq "127.0.0.1" -or $targetHost -eq "::1")
}

# Local-machine memory of device ids this script has minted, keyed by
# (cloud url, outlet, kind, name) so a rotate on a later run against a
# NON-local cloud (no Postgres lookup available) still finds the right
# device instead of guessing. Deliberately outside the repository --
# machine-local bookkeeping, not something to ever commit -- and separate
# from apps\pos\.env.dev / apps\kds\.env.dev, which carry only what the
# running apps read.
$script:BootstrapStateFile = Join-Path $env:LOCALAPPDATA "Holler\dev-bootstrap-state.json"

function Get-BootstrapStateMap {
    $map = @{}
    if (Test-Path $script:BootstrapStateFile) {
        try {
            $raw = Get-Content $script:BootstrapStateFile -Raw -ErrorAction Stop
            if ($raw) {
                $obj = $raw | ConvertFrom-Json -ErrorAction Stop
                foreach ($prop in $obj.PSObject.Properties) { $map[$prop.Name] = [string]$prop.Value }
            }
        } catch {
            # A corrupt/partial state file is machine-local cache, not a
            # source of truth -- treat it as empty rather than failing the
            # bootstrap over it.
            $map = @{}
        }
    }
    return $map
}

function Set-BootstrapStateEntry {
    param([string]$Key, [string]$Value)
    $map = Get-BootstrapStateMap
    $map[$Key] = $Value
    $dir = Split-Path -Parent $script:BootstrapStateFile
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    ($map | ConvertTo-Json) | Out-File -FilePath $script:BootstrapStateFile -Encoding ascii
}

# Resolves an enrolled device credential for (OutletId, Kind, Name) against
# $CloudBaseUrl: rotate if a device is already known (by local Postgres
# lookup when the cloud is local, or by this script's own state file
# otherwise), enroll if not. Returns @{ Token; DeviceId; Status }. Throws on
# an unrecoverable case -- callers decide whether that is fatal to the
# bootstrap (it never is here; see the "NOTHING HERE MAY FAIL THE BOOTSTRAP"
# note below).
function Resolve-DeviceEnrollment {
    param(
        [string]$CloudBaseUrl,
        [hashtable]$Headers,
        [string]$OutletId,
        [string]$Kind,
        [string]$Name,
        [bool]$IsLocalCloud
    )

    $stateKey = "$CloudBaseUrl|$OutletId|$Kind|$Name"
    $existingId = $null

    if ($IsLocalCloud) {
        $existingId = (docker exec holler-postgres-1 psql -U holler -d holler -t -A -c `
            "SELECT d.id FROM device d JOIN device_credential c ON c.device_id = d.id AND c.revoked_at IS NULL WHERE d.outlet_id = '$OutletId' AND d.name = '$Name' LIMIT 1;" 2>$null)
        if ($existingId) { $existingId = $existingId.Trim() }
    }
    if (-not $existingId) {
        $map = Get-BootstrapStateMap
        if ($map.ContainsKey($stateKey)) { $existingId = $map[$stateKey] }
    }

    if ($existingId) {
        $rotateBody = @{ label = "dev-bootstrap" } | ConvertTo-Json
        $enrolled = Invoke-RestMethod -Uri "$CloudBaseUrl/devices/$existingId/credentials/rotate" `
            -Method Post -Body $rotateBody -ContentType 'application/json' -Headers $Headers
        Set-BootstrapStateEntry -Key $stateKey -Value $existingId
        return @{ Token = $enrolled.token; DeviceId = $existingId; Status = "rotated" }
    }

    try {
        $enrollBody = @{ outlet_id = $OutletId; kind = $Kind; name = $Name; label = "dev-bootstrap" } | ConvertTo-Json
        $enrolled = Invoke-RestMethod -Uri "$CloudBaseUrl/devices/enroll" -Method Post `
            -Body $enrollBody -ContentType 'application/json' -Headers $Headers
        Set-BootstrapStateEntry -Key $stateKey -Value $enrolled.device_id
        return @{ Token = $enrolled.token; DeviceId = $enrolled.device_id; Status = "enrolled" }
    } catch {
        $detail = $_.ErrorDetails.Message
        if (-not $detail) { $detail = $_.Exception.Message }
        if ((-not $IsLocalCloud) -and ($detail -match "already enrolled")) {
            throw "a device named '$Name' already exists on $CloudBaseUrl, but this script has no local record of its id: the Postgres lookup only runs against a LOCAL cloud ($CloudBaseUrl is not localhost) and $script:BootstrapStateFile has no entry for it -- most likely because a previous enrollment for this cloud/outlet/name ran from a different machine, or this machine's state file was cleared. There is no device LIST route (device_http.go) to recover the id automatically. Fix: log in as owner@holler.test against $CloudBaseUrl and POST /devices/{deviceId}/credentials/rotate by hand with the id from wherever it was first enrolled, then paste the resulting token into the env file yourself; or enroll under a different -SyncEnrollEmail/name."
        }
        throw $detail
    }
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

# --- 3b/3c. sync + KDS device credentials (ADR-020, T15) ---------------------
# One cloud login, two enrollments: 3b is the POS's own sync credential; 3c
# (below, once logged in) is the KDS screen's credential -- both use
# Resolve-DeviceEnrollment. The POS process HOSTS the sync worker, and it
# needs three variables together:
# HOLLER_CLOUD_BASE_URL, HOLLER_TENANT_ID and HOLLER_DEVICE_TOKEN. All three or
# none -- a worker with a URL and no credential 401s every request and burns
# retry budget doing it.
#
# NOTHING HERE MAY FAIL THE BOOTSTRAP. Sync disabled is a legitimate development
# state: the outlet works offline by design (ADR-013), the POS logs which
# variables were missing, and every other seeded surface is unaffected. The
# common case is simply that the API is not running yet -- dev-up.ps1 starts the
# backend AFTER this script, so a first run on a cold machine reaches this point
# with nothing listening. Re-run the bootstrap once the API is up.
$syncEnvLines = @()
$kdsEnvExtraLines = @()
$deviceToken = $null
$isLocalCloud = Test-CloudBaseUrlIsLocal -CloudBaseUrl $CloudBaseUrl

# Fixed names, because POST /devices/enroll matches an existing device by
# (tenant, outlet, name). That is what makes this step re-runnable.
$syncDeviceName = "Holler Dev Till"
$kdsSyncDeviceName = "Holler Dev KDS"

$apiUp = $false
try {
    Invoke-RestMethod -Uri "$CloudBaseUrl/health" -TimeoutSec 3 | Out-Null
    $apiUp = $true
} catch {
    $apiUp = $false
}

if (-not $apiUp) {
    Write-Host "`n[3b/4] sync credential SKIPPED: no API at $CloudBaseUrl" -ForegroundColor Yellow
    Write-Host "  The POS will start with sync disabled and say so. Start the backend and re-run" -ForegroundColor Yellow
    Write-Host "  this bootstrap to enroll -- re-running is safe." -ForegroundColor Yellow
    Write-Host "[3b/4] KDS credential SKIPPED for the same reason." -ForegroundColor Red
    Write-Host "  apps\kds\.env.dev will be written WITHOUT VITE_KDS_DEVICE_TOKEN. The KDS" -ForegroundColor Red
    Write-Host "  throws a config error at startup until this bootstrap is re-run with the" -ForegroundColor Red
    Write-Host "  backend reachable -- see docs/lan-setup.md section 5." -ForegroundColor Red
} else {
    $tenantId = $values['HOLLER_TENANT_ID']
    $authHeaders = @{ 'X-Tenant-ID' = $tenantId }
    $headers = $null
    try {
        $loginBody = @{
            email     = $SyncEnrollEmail
            password  = $SyncEnrollPassword
            outlet_id = $values['HOLLER_OUTLET_ID']
        } | ConvertTo-Json
        $session = Invoke-RestMethod -Uri "$CloudBaseUrl/auth/login" -Method Post `
            -Body $loginBody -ContentType 'application/json' -Headers $authHeaders
        $headers = @{
            'X-Tenant-ID'   = $tenantId
            'Authorization' = "Bearer $($session.access_token)"
        }
    } catch {
        $detail = $_.ErrorDetails.Message
        if (-not $detail) { $detail = $_.Exception.Message }
        Write-Host "`n[3b/4] sync credential SKIPPED: could not log in as $SyncEnrollEmail : $detail" -ForegroundColor Yellow
        Write-Host "  Not fatal. The POS and KDS start with sync/credential disabled." -ForegroundColor Yellow
        if ($detail -match "authentication required") {
            # ADR-012 deliberately returns the SAME response for a bad password
            # and for a throttled one, so this cannot be distinguished from the
            # outside. Five attempts per fifteen-minute fixed window, held in
            # process memory -- so several bootstrap runs in a row can trip it,
            # and restarting the backend clears it.
            Write-Host "  A 401 here can ALSO be the login rate limiter, which is intentionally" -ForegroundColor Yellow
            Write-Host "  indistinguishable from a wrong password (ADR-012). Repeated bootstrap" -ForegroundColor Yellow
            Write-Host "  runs can trip it; restart the backend or wait out the 15-minute window." -ForegroundColor Yellow
        }
    }

    if ($headers) {
        Write-Host "`n[3b/4] enrolling this till's sync credential..." -ForegroundColor Cyan
        try {
            $pos = Resolve-DeviceEnrollment -CloudBaseUrl $CloudBaseUrl -Headers $headers `
                -OutletId $values['HOLLER_OUTLET_ID'] -Kind 'POS' -Name $syncDeviceName -IsLocalCloud $isLocalCloud
            $deviceToken = $pos.Token
            if ([string]::IsNullOrWhiteSpace($deviceToken)) { throw "the response carried no token" }
            Write-Host "$($pos.Status) device $($pos.DeviceId)"

            # NOTE the device id: the POS stamps this on what it sends, so the
            # credential and the identity must agree.
            $syncEnvLines = @(
                "HOLLER_CLOUD_BASE_URL=$CloudBaseUrl",
                "HOLLER_TENANT_ID=$tenantId",
                "HOLLER_DEVICE_TOKEN=$deviceToken"
            )
            Write-Host "sync ENABLED for this till (token written to .env.dev only)" -ForegroundColor Cyan
        } catch {
            $detail = $_.Exception.Message
            Write-Host "[3b/4] sync credential SKIPPED: $detail" -ForegroundColor Yellow
            Write-Host "  Not fatal. The POS starts with sync disabled and logs which variables" -ForegroundColor Yellow
            Write-Host "  were missing. Enrollment needs a principal holding outlet.manage" -ForegroundColor Yellow
            Write-Host "  (owner@holler.test is seeded with it); override with -SyncEnrollEmail." -ForegroundColor Yellow
            $syncEnvLines = @()
        }

        # --- 3c. KDS device credential (T15) -------------------------------------
        # apps/kds/src/lib/lanConfig.ts throws without VITE_KDS_DEVICE_TOKEN, and
        # devseed.rs seeds a `device` row for the KDS but no `device_credential_cache`
        # row -- there is no offline path to a working credential, a real cloud
        # enrollment is mandatory (docs/lan-setup.md section 0.3). This enrolls a
        # KDS device SEPARATE from the seeded local `device` row referenced by
        # $KdsDeviceId: verification (edge/device/src/auth.rs CachedCredentialVerifier)
        # checks the credential's outlet_id and device_kind, never device_id, so the
        # credential's own device_id need not equal $KdsDeviceId -- only $KdsDeviceId
        # needs to exist locally, which devseed already guarantees (it is also the
        # value kot_status_history.changed_by_device_id's FK is checked against).
        Write-Host "`n[3c/4] enrolling this till's KDS credential..." -ForegroundColor Cyan
        try {
            $kds = Resolve-DeviceEnrollment -CloudBaseUrl $CloudBaseUrl -Headers $headers `
                -OutletId $values['HOLLER_OUTLET_ID'] -Kind 'KDS' -Name $kdsSyncDeviceName -IsLocalCloud $isLocalCloud
            if ([string]::IsNullOrWhiteSpace($kds.Token)) { throw "the response carried no token" }
            Write-Host "$($kds.Status) device $($kds.DeviceId)"
            $kdsEnvExtraLines = @("VITE_KDS_DEVICE_TOKEN=$($kds.Token)")
            Write-Host "KDS credential ENABLED (token written to apps\kds\.env.dev only)" -ForegroundColor Cyan
        } catch {
            $detail = $_.Exception.Message
            Write-Host "[3c/4] KDS credential SKIPPED: $detail" -ForegroundColor Red
            Write-Host "  apps\kds\.env.dev will be written WITHOUT VITE_KDS_DEVICE_TOKEN. The KDS" -ForegroundColor Red
            Write-Host "  throws a config error at startup until this is fixed -- re-run this" -ForegroundColor Red
            Write-Host "  bootstrap once the cause above is resolved. Not fatal to this bootstrap" -ForegroundColor Red
            Write-Host "  run: the outlet still works offline (ADR-013), just without a KDS." -ForegroundColor Red
            $kdsEnvExtraLines = @()
        }
    } else {
        Write-Host "[3c/4] KDS credential SKIPPED: no login session (see above)." -ForegroundColor Red
        Write-Host "  apps\kds\.env.dev will be written WITHOUT VITE_KDS_DEVICE_TOKEN." -ForegroundColor Red
    }
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
# ADR-020. Appended only when all three were obtained; the POS treats a partial
# set as no set. THE TOKEN LIVES HERE AND NOWHERE ELSE -- .env.dev is gitignored,
# and no committed file in this repository carries a device token.
$envLines += $syncEnvLines
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
# T15. VITE_KDS_DEVICE_TOKEN, from step 3c -- the credential this token
# belongs to is a SEPARATE cloud-enrolled device from $KdsDeviceId above;
# see the 3c comment for why that is correct rather than a mismatch.
# Appended only when 3c actually obtained one -- a stale/empty token line
# would be worse than the file omitting it, since lanConfig.ts's error for
# "unset" ("no enrolled credential") is a clearer signal than whatever an
# empty string would produce.
$kdsEnvLines += $kdsEnvExtraLines
$kdsEnvLines | Out-File -FilePath $kdsEnvFile -Encoding ascii
Write-Host "wrote $kdsEnvFile (LAN URL host detected as $lanIp -- verify with ipconfig if this machine has more than one network adapter)" -ForegroundColor Cyan
if ($kdsEnvExtraLines.Count -eq 0) {
    Write-Host "  WARNING: VITE_KDS_DEVICE_TOKEN was NOT written -- the KDS will throw a" -ForegroundColor Red
    Write-Host "  config error at startup. See the [3c/4] message above and docs/lan-setup.md" -ForegroundColor Red
    Write-Host "  section 5. Re-run this bootstrap once that is resolved." -ForegroundColor Red
}

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
