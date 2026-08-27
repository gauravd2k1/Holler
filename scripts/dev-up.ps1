# Holler: bring up the whole local development stack from one command.
#
# DEVELOPMENT ONLY. Nothing here runs at an outlet (ADR-013): outlet machines
# have no Docker, no Postgres, no Node and no developer toolchain. They run one
# native executable over one SQLite file.
#
# What this does, in order:
#   1. makes sure the Docker engine is up (starting Docker Desktop if needed)
#   2. runs scripts\dev-bootstrap.ps1 -- infra containers, Postgres migrations,
#      cloud fixtures, and the encrypted edge SQLite seed (menu, inventory,
#      recipes, billing config)
#   3. starts the backend API in its own window        (skip: -NoBackend)
#   4. starts the KDS Vite server in its own window    (skip: -NoKds)
#   5. runs the POS in THIS window, in the foreground  (skip: -NoPos)
#
# Step 5 is foreground ON PURPOSE and must stay that way. The POS is a Tauri
# GUI app: launched from a process whose stdio is captured (a background job,
# an agent tool, a CI step) it starts, binds its LAN port, logs happily -- and
# no window ever appears. `Get-Process holler-pos` shows it running with an
# empty MainWindowTitle and there is nothing on screen.
#
# Observed 2026-08-27. Note what it is NOT: the window station is still
# WinSta0 and [Environment]::UserInteractive is still True in that case, so
# neither of those detects it. The signal that actually differs is redirected
# stdio, which is what the guard below tests -- a heuristic for "no terminal
# is attached", not a direct read of window visibility.
#
# Run this script from a terminal you own.
#
# Ctrl+C stops the POS, and this script then tears down the windows it opened.
# The Docker containers are deliberately LEFT RUNNING -- they are shared with
# `go test` and cost nothing idle. Stop them with `make down`.
#
# See docs/DEV_SETUP.md for the manual sequence this automates.

[CmdletBinding()]
param(
    # Skip "docker compose up" (passed through to the bootstrap). The engine
    # check in step 1 is skipped too.
    [switch]$SkipInfra,

    # Skip the bootstrap entirely -- containers, migrations, and both seeders.
    # Use when the databases are already seeded and you just want the apps.
    [switch]$SkipSeed,

    # Seed WITHOUT the billing config. Off by default, i.e. billing IS seeded,
    # because without it the POS reaches "Issue Bill" and fails with
    # NO_FISCAL_PROFILE_CONFIGURED -- correct behaviour, but not a runnable
    # path. Pass this when running tests/e2e-scenario, which seeds its own and
    # would otherwise end up with two active SALES series.
    [switch]$NoBilling,

    # Route every print to files in this directory instead of a device
    # (HOLLER_PRINTER_FILE_SINK_DIR). The only way to see the real ESC/POS byte
    # stream on a machine with no thermal printer attached. Establishes nothing
    # about real device I/O -- see edge/printer/src/transport/file_sink.rs.
    [string]$PrinterFileSinkDir = "",

    [switch]$NoBackend,
    [switch]$NoKds,
    [switch]$NoPos,

    # Start the POS even though stdio is redirected. Only correct when you are
    # at a real terminal and piping output on purpose -- see the guard below.
    [switch]$AllowRedirectedOutput,

    # Bind the KDS dev server to all interfaces so a second machine on the LAN
    # can open it. Off by default: localhost only.
    [switch]$KdsOnLan,

    # Postgres connection for the backend API. Matches docker-compose.yml.
    [string]$DatabaseUrl = "postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable",

    # DEVELOPMENT signing key. Has no default in backend config by design --
    # a missing secret is a startup error there, never a generated fallback --
    # so one is supplied here and must never be used for real data.
    [string]$TokenSigningKey = "holler-dev-signing-key-not-for-prod"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$spawned = @()

function Write-Step($n, $msg) { Write-Host ""; Write-Host "[$n] $msg" -ForegroundColor Cyan }
function Write-Note($msg)     { Write-Host "       $msg" -ForegroundColor DarkGray }

# Opens a service in its own titled window and remembers it for cleanup. Kept
# as separate windows rather than merged into this one on purpose: three
# interleaved log streams with no prefixes are unreadable, and when the backend
# dies you want its stack trace still on screen.
function Start-ServiceWindow($title, $workDir, $command) {
    $inner = "`$Host.UI.RawUI.WindowTitle = '$title'; Set-Location '$workDir'; $command"
    $p = Start-Process powershell -PassThru -ArgumentList @(
        "-NoExit", "-NoProfile", "-Command", $inner
    )
    $script:spawned += $p
    Write-Note "$title -> pid $($p.Id)"
    return $p
}

Write-Host "Holler dev stack" -ForegroundColor Green
Write-Host "repo: $repo"

# Terminal guard. This is the exact failure this script exists to prevent, so
# it is checked rather than merely documented. IsOutputRedirected is True
# whenever stdio is captured -- a background job or an agent tool, but also a
# deliberate `.\dev-up.ps1 > log.txt` from a real terminal, where the window
# WOULD appear. Hence the named override rather than a flat refusal.
if (-not $NoPos -and [Console]::IsOutputRedirected -and -not $AllowRedirectedOutput) {
    Write-Host ""
    Write-Host "refusing to start the POS: this session's output is redirected," -ForegroundColor Red
    Write-Host "which usually means no terminal is attached. The POS would run" -ForegroundColor Red
    Write-Host "with no window ever appearing on screen." -ForegroundColor Red
    Write-Host ""
    Write-Host "Run it from a terminal you own, or pass -NoPos to start only the" -ForegroundColor Yellow
    Write-Host "services. If you are at a real terminal and piping output on" -ForegroundColor Yellow
    Write-Host "purpose, pass -AllowRedirectedOutput." -ForegroundColor Yellow
    exit 1
}

# --------------------------------------------------------------- 1. engine --
if (-not $SkipInfra -and -not $SkipSeed) {
    Write-Step 1 "checking the Docker engine..."
    $engineUp = $false
    try {
        docker info --format '{{.ServerVersion}}' *> $null
        $engineUp = ($LASTEXITCODE -eq 0)
    } catch { $engineUp = $false }

    if (-not $engineUp) {
        # Docker Desktop does not autostart on this box (docs/RESUME.md 7).
        $dd = Join-Path $env:ProgramFiles "Docker\Docker\Docker Desktop.exe"
        if (-not (Test-Path $dd)) { throw "Docker engine is down and Docker Desktop was not found at $dd" }
        Write-Note "engine down -- launching Docker Desktop and waiting (up to 180s)"
        Start-Process $dd | Out-Null
        $deadline = (Get-Date).AddSeconds(180)
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Seconds 5
            try {
                docker info *> $null
                if ($LASTEXITCODE -eq 0) { $engineUp = $true; break }
            } catch {}
        }
        if (-not $engineUp) { throw "Docker engine did not come up within 180s" }
    }
    Write-Note "engine ready"
} else {
    Write-Step 1 "skipping the Docker engine check"
}

# ----------------------------------------------------------- 2. bootstrap --
if ($SkipSeed) {
    Write-Step 2 "skipping bootstrap (-SkipSeed)"
    Write-Note "assuming Postgres and the edge database are already seeded"
} else {
    Write-Step 2 "bootstrapping databases (migrations + cloud fixtures + edge seed)..."
    # A HASHTABLE, not an array. Array splatting passes its elements
    # POSITIONALLY: @("-SkipInfra") binds the literal string "-SkipInfra" to
    # dev-bootstrap's first positional parameter, which is $DbKeyHex -- so the
    # seeder dies with "HOLLER_DB_KEY_HEX must be exactly 64 hex characters"
    # and every switch is silently dropped. Only hashtable splatting binds by
    # name. Cost one debugging session on 2026-08-27.
    $bootstrapArgs = @{}
    if ($SkipInfra)                 { $bootstrapArgs["SkipInfra"] = $true }
    if (-not $NoBilling)            { $bootstrapArgs["WithBilling"] = $true }
    if ($PrinterFileSinkDir -ne "") { $bootstrapArgs["PrinterFileSinkDir"] = $PrinterFileSinkDir }
    & (Join-Path $PSScriptRoot "dev-bootstrap.ps1") @bootstrapArgs
}

# ------------------------------------------------------------- 3. backend --
if ($NoBackend) {
    Write-Step 3 "skipping the backend API (-NoBackend)"
    Write-Note "nothing in the M1-M4 offline acceptance path needs it"
} else {
    Write-Step 3 "starting the backend API on :8080..."
    $cmd = "`$env:DATABASE_URL='$DatabaseUrl'; `$env:TOKEN_SIGNING_KEY='$TokenSigningKey'; go run ./cmd/api"
    Start-ServiceWindow "holler-backend" (Join-Path $repo "backend") $cmd | Out-Null
}

# ----------------------------------------------------------------- 4. KDS --
if ($NoKds) {
    Write-Step 4 "skipping the KDS (-NoKds)"
} else {
    Write-Step 4 "starting the KDS dev server on :5174..."
    $kdsDir = Join-Path $repo "apps\kds"
    if (-not (Test-Path (Join-Path $kdsDir "node_modules"))) {
        Write-Note "installing KDS dependencies (first run)"
        Push-Location $kdsDir
        pnpm install
        Pop-Location
    }
    $hostArg = ""
    if ($KdsOnLan) { $hostArg = " --host 0.0.0.0" }
    Start-ServiceWindow "holler-kds" $kdsDir "pnpm dev --mode dev$hostArg" | Out-Null
    if ($KdsOnLan) { Write-Note "reachable across this LAN -- the KDS bridge is unauthenticated in dev" }
}

# ----------------------------------------------------------------- 5. POS --
try {
    if ($NoPos) {
        Write-Step 5 "skipping the POS (-NoPos)"
        Write-Host ""
        Write-Host "Services are up in their own windows. Press Ctrl+C to stop them." -ForegroundColor Green
        while ($true) { Start-Sleep -Seconds 3600 }
    } else {
        $posDir = Join-Path $repo "apps\pos"
        if (-not (Test-Path (Join-Path $posDir "node_modules"))) {
            Write-Note "installing POS dependencies (first run)"
            Push-Location $posDir
            pnpm install
            Pop-Location
        }

        Write-Step 5 "starting the POS in this window..."
        Write-Note "login: cashier@holler.test / holler123"
        Write-Note "blank window? delete apps\pos\node_modules\.vite, and check the Network tab --"
        Write-Note "not only the console. optimizeDeps is dev-server-only; vite build never reads it."
        Write-Note "LNK1104 on the Rust build is McAfee holding the fresh binary, not a code error: re-run."
        Write-Host ""

        # run-dev.ps1 is the ONE way to start the POS: it reads device identity
        # and the encryption key from .env.dev and validates them. Do not start
        # `pnpm dev` for the POS alongside it -- tauri.conf.json's
        # beforeDevCommand starts Vite itself, and a second one falls back to
        # 5174 (colliding with the KDS) or fails under strictPort.
        & (Join-Path $posDir "run-dev.ps1")
    }
}
finally {
    Write-Host ""
    Write-Host "shutting down the windows this script opened..." -ForegroundColor Yellow
    foreach ($p in $spawned) {
        if ($p -and -not $p.HasExited) {
            try {
                Stop-Process -Id $p.Id -Force -ErrorAction Stop
                Write-Note "stopped pid $($p.Id)"
            } catch {}
        }
    }
    Write-Host "Docker containers left running on purpose -- 'make down' stops them." -ForegroundColor DarkGray
}
