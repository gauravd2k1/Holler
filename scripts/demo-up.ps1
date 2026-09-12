# Holler demo build -- THE ONE COMMAND THAT BRINGS THE DEMO STACK UP.
#
#   .\scripts\demo-up.ps1 -DbKeyHex <64-hex> -LanHost <hotspot-ip> [-Fresh]
#
# Day-of checklist becomes three lines: hotspot -> this -> the phone.
#
# DEVELOPMENT / DEMO ONLY. Nothing here runs at an outlet (ADR-013): outlet
# machines have no Docker, no Postgres, no Node and no developer toolchain.
#
# WHAT IT DOES, one checkpoint per step, STOPPING AT THE FIRST FAILURE WITH THE
# REASON AND THE NEXT ACTION:
#
#   [1/10] preflight  -- no POS process, the demo ports free, nothing holding
#                        the edge database (every refusal NAMES the pid)
#   [2/10] infra      -- postgres, redis, nats (skip: -SkipInfra)
#   [3/10] reset      -- scripts\demo-reset.ps1 -Force   (only with -Fresh)
#   [4/10] backend    -- own window, PORT SET EXPLICITLY, verified by NEW pid
#   [5/10] bootstrap  -- scripts\dev-bootstrap.ps1, remembered UPI payee, -LanHost
#   [6/10] captain    -- rebuild apps\captain\dist (the POS serves it from disk)
#   [7/10] waiter     -- enrol/rotate the WAITER device, capture its token
#   [8/10] POS        -- apps\pos\run-dev.ps1 in its own window, THE ONLY VITE
#   [9/10] KDS        -- own window, and the browser opened on it
#   [10/10] the on-screen checks, the captain URL and the pair token
#
# scripts\demo-down.ps1 stops, BY PID AND START TIME, what this script started.
#
# ---------------------------------------------------------------------------
# FOUR ORDERING DECISIONS, EACH MADE AGAINST A KNOWN FAILURE. Do not reorder
# without reading these.
#
# 1. THE WAITER IS ENROLLED BEFORE THE POS STARTS, NOT AFTER.
#    The captain listener verifies a token against the edge's LOCAL
#    device_credential_cache (captain.rs module header) and that cache is
#    written in exactly one place: config::apply_bundle, reached only from
#    AppState::drain_outbox's config pull (edge/sync/src/config.rs:814 is the
#    only non-test caller of repo::replace_device_credential_cache). The POS
#    runs that pull at startup and then every 60s
#    (DEFAULT_PERIODIC_DRAIN_INTERVAL). Enrolling first means the credential is
#    already in the cloud when the till's STARTUP pull runs, so it is cached by
#    the time the phone pairs. Enrolling afterwards works too -- in up to a
#    minute, silently, with the phone showing "That device token was rejected"
#    the whole time, which is the same message a mistyped token gives
#    (docs\lan-setup.md section 6.2).
#
# 2. THE WAITER CREDENTIAL IS ROTATED ON EVERY RUN, EVEN WHEN THE DEVICE
#    ALREADY EXISTS. This is scenario board S-CAP-20 and it is a demo-killer:
#    outlet.Repository.ListEdgeCredentials only returns credentials whose OWN
#    config_version exceeds the edge's cursor (backend/internal/outlet/device.go),
#    so a credential the edge has already pulled is NEVER re-sent -- and
#    config::apply_bundle's insert_device_if_absent loop, which mints the
#    `device` row the order-create foreign key needs, only ever sees
#    credentials that ARRIVE in a bundle. A device paired before that fix
#    therefore stays permanently broken, and re-pairing does not help.
#    RotateCredential bumps the outlet config_version and issues the new
#    credential AT that version (backend/internal/outlet/device_service.go:139-167),
#    so a rotated credential is always above the edge's cursor and its device
#    row is always minted. Rotating unconditionally makes S-CAP-20 structurally
#    impossible rather than something the operator has to remember.
#    THE COST, stated: the phone gets a NEW token on every run and must be
#    paired again. That is why pairing the phone is a step of the day-of
#    checklist rather than a thing done once.
#    (docs\demo-status.md says the device must be "re-ENROLLED". The repo is the
#    authority and it disagrees: what is required is a credential above the
#    edge's cursor, which BOTH enrol and rotate produce. Rotate is enough.)
#
# 3. THIS SCRIPT STARTS THE BACKEND ITSELF, EVEN AFTER -Fresh HAS ALREADY
#    STARTED ONE. demo-reset.ps1 launches a backend in its step 1/4 and that one
#    carries neither PORT nor HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS. The second is
#    not cosmetic: the demo signs into the till, the admin console and the
#    captain page from one laptop, the default budget is five attempts per
#    fifteen minutes, and a throttled login is INDISTINGUISHABLE from a wrong
#    password by design (ADR-012, scenario board S-BE-09). One mistyped
#    password would end the demo at its first screen. So the backend is always
#    (re)started here, with both variables set, and verified by a NEW pid.
#
# 4. apps\captain\dist IS REBUILT BEFORE THE POS STARTS. captain.rs serves that
#    directory from disk (dist_dir()), and docs\lan-setup.md section 6.3 says to
#    restart the POS if you build it afterwards. Building first removes the
#    ordering trap entirely. It is the same class of defect as a stale Vite
#    serving a bundle built in a different environment -- which is how the UPI
#    QR went missing from the bill screen on 2026-09-12.
# ---------------------------------------------------------------------------
#
# WHAT THIS SCRIPT DOES NOT ESTABLISH, said here rather than implied by silence:
#
#   - It cannot prove the KDS is CONNECTED. That is a WebSocket the browser
#     opens; all this script can check is that the KDS dev server answers, that
#     apps\kds\.env.dev names the LAN host it was given, and that the LAN port
#     accepts a TCP connection. The indicator on the KDS screen is the
#     observation, and step [10/10] asks for it by name.
#   - It cannot prove the phone will create an order. GET /api/session
#     authenticates against the credential cache but never touches the `device`
#     row whose foreign key S-CAP-20 breaks, so a probe passing there would pass
#     on a broken device too. The proof is demo step 1a, from the phone.
#   - Killing the POS leaves a PLAINTEXT edge.db beside the sealed .enc: no exit
#     path on this build fires RunEvent::Exit (gap A6, carried). See
#     scripts\demo-down.ps1, which says so every time.

[CmdletBinding()]
param(
    # 32-byte key, hex-encoded, for the edge database's encryption at rest
    # (ADR-011). NO DEFAULT, same rule as dev-bootstrap.ps1 and demo-reset.ps1:
    # a default here is consent by omission. It is the OPERATOR'S --
    # apps\pos\.env.dev is deny-ruled to agents and no agent supplies a literal
    # key (T24). Passed straight through; this script never reads or logs it.
    [string]$DbKeyHex = "",

    # The LAN IPv4 address the phone and any second machine must reach. PASS IT
    # EXPLICITLY ON DEMO DAY: a hotspot's address changes on every reconnect,
    # and a stale address baked into apps\kds\.env.dev is a KDS that loads,
    # looks fine and never connects. Empty lets dev-bootstrap.ps1 work it out
    # from the default-route interface and print which adapter it chose.
    [string]$LanHost = "",

    # Reset cloud + edge to the demo seed first (scripts\demo-reset.ps1 -Force).
    # DESTRUCTIVE -- it drops the public Postgres schema and deletes the edge
    # database. Without it this script starts the stack on whatever state is
    # already there.
    [switch]$Fresh,

    # Both required, and both must resolve outside the real tree, when this runs
    # under a Claude Code shell -- see scripts\agent-guard.ps1.
    [string]$RepoRoot = "",
    [Alias('DataDir')]
    [string]$EdgeDataDir = (Join-Path $env:APPDATA "com.holler.pos"),

    [string]$CloudBaseUrl = "http://localhost:8080",
    [int]$BackendPort = 8080,
    [int]$PosVitePort = 5173,
    [int]$KdsPort = 5174,
    [int]$LanPort = 9310,
    [int]$CaptainPort = 9320,

    # The ports step [1/10] requires to be free. Defaults to the five above.
    # A PARAMETER so the refusal path can be falsified on scratch ports: the
    # rule in force until the demo is that no test binds 8080, 9310, 9320, 5173,
    # 5174 or 5175, so a test that proves this check fires must be able to point
    # it somewhere else.
    [int[]]$PreflightPorts = @(),

    # Passed through to dev-bootstrap.ps1, which REMEMBERS them between runs in
    # %LOCALAPPDATA%\Holler\dev-bootstrap-state.json. Leave both empty on a
    # machine that has been given them once. An unset VPA means NO QR RENDERS AT
    # ALL on the bill screen and on the printed receipt.
    [string]$UpiVpa = "",
    [string]$UpiPayeeName = "",

    # Where every print is written instead of being sent to a device
    # (HOLLER_PRINTER_FILE_SINK_DIR). Empty resolves to <repo>\.dev-prints.
    #
    # ALWAYS PASSED, AND THAT IS NOT OPTIONAL. dev-bootstrap.ps1 rewrites
    # apps\pos\.env.dev WHOLESALE and only emits this line when it is given the
    # parameter -- so a bootstrap run without it silently DELETES the file sink,
    # after which "Print Bill" is sent to a thermal printer that does not exist
    # and no .escpos, no .html and no .pdf appear at all. That is the same
    # failure the UPI payee had before the bootstrap started remembering it, and
    # it would land squarely on demo step 2.
    [string]$PrinterFileSinkDir = "",

    # Fixed, because POST /devices/enroll matches an existing device by
    # (tenant, outlet, name) -- that is what makes step [7/10] re-runnable.
    [string]$WaiterDeviceName = "Demo Captain Phone",

    # Enrolment is gated on outlet.manage, which the seeded cashier and buyer
    # deliberately do not hold.
    [string]$SyncEnrollEmail = "owner@holler.test",
    [string]$SyncEnrollPassword = "holler123",

    [string]$DatabaseUrl = "postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable",
    [string]$PostgresContainer = "holler-postgres-1",
    [string]$TokenSigningKey = "holler-dev-signing-key-not-for-prod",
    [string]$AdminOrigin = "http://localhost:5175",

    # S-BE-09. Only the COUNT moves, and only for this dev script: the
    # production default in backend/internal/auth/ratelimit.go is untouched, as
    # are the identical 401 body, the IP+tenant key pair and the fail-closed
    # limiter path.
    [int]$LoginRateLimitAttempts = 50,

    [switch]$SkipInfra,

    # Where the pids this run started are recorded for demo-down.ps1.
    [string]$StateFile = (Join-Path $env:LOCALAPPDATA "Holler\demo-up-state.json"),

    # Report every step and start NOTHING: no process, no port, no file, no
    # mutating cloud call. The preflight still runs, because it only reads.
    # This is also the only mode an agent shell may use -- see the guard below.
    [switch]$WhatIf
)

$ErrorActionPreference = "Stop"

# STRUCTURAL GUARD, FIRST THING. Dot-sourced so a missing guard file stops the
# script rather than silently disabling the control.
#
# A REAL RUN IS REFUSED UNDER AN AGENT SHELL, for the same reason dev-up.ps1 and
# run-dev.ps1 are: it starts the operator's stack on live ports and has no
# scratch mode. -WhatIf is allowed through because it is read-only -- it starts
# nothing and writes nothing, exactly like scripts\check-cloud-unreachable.ps1,
# which is deliberately unguarded. That exemption is what lets the preflight and
# the argument handling be tested at all; if -WhatIf ever grows a side effect,
# this exemption has to go with it.
. (Join-Path $PSScriptRoot "agent-guard.ps1")
if (-not $WhatIf) {
    Assert-NotAgentShell -ScriptName "demo-up.ps1" `
        -Ports "$BackendPort, $LanPort, $CaptainPort, $PosVitePort, $KdsPort"
}

$repoRoot = if ([string]::IsNullOrWhiteSpace($RepoRoot)) { Split-Path -Parent $PSScriptRoot } else { $RepoRoot }

if ($WhatIf -and (Test-IsAgentShell)) {
    # Even read-only, an agent shell must name its scratch paths: the failure
    # mode this guard exists for is a default quietly resolving to the
    # operator's real tree, and a -WhatIf that PRINTS the real paths is a
    # -WhatIf whose output gets acted on against the real tree.
    Assert-AgentSafePaths -ScriptName "demo-up.ps1 -WhatIf" `
        -BoundParameters $PSBoundParameters `
        -RepoRootValue $repoRoot `
        -DataDirValue $EdgeDataDir `
        -DataDirParameterName "EdgeDataDir"
}

$startedAt = Get-Date
$script:Recorded = @{ started_at = $startedAt.ToString("o"); processes = @() }

$script:PortsWereOverridden = ($PreflightPorts.Count -gt 0)
if (-not $script:PortsWereOverridden) {
    $PreflightPorts = @($PosVitePort, $KdsPort, $BackendPort, $LanPort, $CaptainPort)
}

$script:TotalSteps = 10

function Write-Step($n, $msg)  { Write-Host ""; Write-Host "[$n/$($script:TotalSteps)] $msg" -ForegroundColor Cyan }
function Write-Note($msg)      { Write-Host "       $msg" -ForegroundColor DarkGray }
function Write-Ok($msg)        { Write-Host "       OK   $msg" -ForegroundColor Green }
function Write-Warn($msg)      { Write-Host "       WARN $msg" -ForegroundColor Yellow }

function Save-RecordedState {
    if ($WhatIf) { return }
    try {
        $dir = Split-Path -Parent $StateFile
        if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
        ($script:Recorded | ConvertTo-Json -Depth 5) | Out-File -FilePath $StateFile -Encoding ascii
    } catch {
        # Bookkeeping for demo-down, never a reason to fail a run that is
        # otherwise up. Say so rather than dying at step 9 of 10.
        Write-Warn "could not write $StateFile ($($_.Exception.Message)) -- demo-down will need the pids by hand"
    }
}

# EVERY fatal path goes through this, so the message a human reads always names
# the next action rather than a bare exception. Same rule demo-reset.ps1 holds
# itself to.
function Fail-WithAction($problem, $nextAction) {
    Write-Host ""
    Write-Host "FAILED: $problem" -ForegroundColor Red
    Write-Host "NEXT ACTION: $nextAction" -ForegroundColor Yellow
    if (-not $WhatIf) {
        Write-Host ""
        Write-Host "Nothing further was started. Whatever this run did start is listed in" -ForegroundColor DarkGray
        Write-Host "$StateFile -- stop it with .\scripts\demo-down.ps1 before re-running." -ForegroundColor DarkGray
        Save-RecordedState
    }
    exit 1
}

# A pid ALONE IS NOT AN IDENTITY: Windows reuses them. Everything recorded for
# demo-down carries its start time, and demo-down refuses to kill a pid whose
# start time has changed. Same rule as "verify a restart by the new process's
# identity, never by the port answering", applied to teardown.
function Record-Process($role, $proc, $extra) {
    if ($null -eq $proc) { return }
    $started = $null
    try { $started = $proc.StartTime.ToString("o") } catch { $started = $null }
    $script:Recorded.processes += @{
        role       = $role
        pid        = $proc.Id
        name       = $proc.ProcessName
        started_at = $started
        note       = $extra
    }
    Save-RecordedState
}

function Get-ListenerOwningPid($port) {
    $c = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue
    if ($c) { return ($c | Select-Object -First 1 -ExpandProperty OwningProcess) }
    return $null
}

function Describe-Pid($processId) {
    $p = Get-Process -Id $processId -ErrorAction SilentlyContinue
    if (-not $p) { return "pid $processId (gone)" }
    $started = "unknown start time"
    try { $started = "started $($p.StartTime.ToString('yyyy-MM-dd HH:mm:ss'))" } catch { }
    return "$($p.ProcessName) pid $processId, $started"
}

# Each service in its own titled window. Separate windows rather than one merged
# stream on purpose (dev-up.ps1's reason, unchanged): three interleaved logs with
# no prefixes are unreadable, and when the backend dies you want its stack trace
# still on screen. It matters for the POS specifically too -- a Tauri window
# launched from a process whose stdio is captured never appears, and
# Start-Process without redirection gives the child a real console.
function Start-ServiceWindow($title, $workDir, $command) {
    $inner = "`$Host.UI.RawUI.WindowTitle = '$title'; Set-Location '$workDir'; $command"
    return Start-Process powershell -PassThru -ArgumentList @("-NoExit", "-NoProfile", "-Command", $inner)
}

function Wait-ForListener($port, $timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $owner = Get-ListenerOwningPid $port
        if ($owner) { return $owner }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

# THE PORT WAS NEVER PART OF WHAT "SCRATCH" COVERED, and that is how a run aimed
# at a scratch directory and a scratch database took down the operator's backend
# three times in one day. -PreflightPorts exists so the refusal path can be
# falsified somewhere harmless, and it would introduce exactly that hole: a run
# that checks 48080 for a free port and then STOPS whatever owns 8080. So the
# override must be COHERENT -- every port this run will actually take has to be
# one it was asked to check.
if ($script:PortsWereOverridden) {
    $willTake = @(
        @{ Port = $BackendPort; Name = "-BackendPort" },
        @{ Port = $PosVitePort; Name = "-PosVitePort" },
        @{ Port = $KdsPort;     Name = "-KdsPort" },
        @{ Port = $LanPort;     Name = "-LanPort" },
        @{ Port = $CaptainPort; Name = "-CaptainPort" }
    )
    $uncovered = @($willTake | Where-Object { $PreflightPorts -notcontains $_.Port })
    if ($uncovered.Count -gt 0) {
        $lines = ($uncovered | ForEach-Object { "$($_.Name) $($_.Port)" }) -join ", "
        Write-Host ""
        Write-Host "REFUSED: -PreflightPorts was given but does not cover every port this run takes." -ForegroundColor Red
        Write-Host "  not covered: $lines" -ForegroundColor Red
        Write-Host "  checked    : $(($PreflightPorts | Sort-Object -Unique) -join ', ')" -ForegroundColor Red
        Write-Host "  NOTHING WAS STARTED." -ForegroundColor Red
        Write-Host ""
        Write-Host "  A run that checks a scratch port free and then stops whatever owns a LIVE one" -ForegroundColor Yellow
        Write-Host "  is the exact failure this parameter would otherwise reintroduce. Move every" -ForegroundColor Yellow
        Write-Host "  port together, or pass no -PreflightPorts at all." -ForegroundColor Yellow
        exit 1
    }
}

Write-Host ""
Write-Host "Holler demo stack" -ForegroundColor Green
Write-Host "repo : $repoRoot"
Write-Host "edge : $EdgeDataDir"
Write-Host "cloud: $CloudBaseUrl"
if ($WhatIf) {
    Write-Host ""
    Write-Host "-WhatIf: reporting only. NOTHING will be started, bound, written or enrolled." -ForegroundColor Yellow
}

# =====================================================================
Write-Step 1 "preflight -- no POS, the demo ports free, nothing holding the edge database"
# =====================================================================

# (i) a POS process at all, whether or not it currently holds a handle: it takes
#     one the moment it opens the database, and it also owns 5173, 9310 and
#     9320, so every later step would fail behind it.
function Get-HollerPosProcess {
    $byName = @(Get-Process -Name "holler-pos", "holler_pos" -ErrorAction SilentlyContinue)
    # The TAURI BUILD OUTPUT directory specifically, not all of apps\pos: a check
    # over apps\pos sweeps in esbuild running out of the POS's node_modules and
    # names a bundler's pid ahead of the actual till. The compiled POS binary is
    # the only thing under this tree that opens the edge database.
    $posBuildDir = (Join-Path $repoRoot "apps\pos\src-tauri\target")
    $byPath = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $path = $null
        try { $path = $_.Path } catch { $path = $null }   # Access denied on system processes
        $path -and $path.StartsWith($posBuildDir, [System.StringComparison]::OrdinalIgnoreCase)
    })
    return @(@($byName) + @($byPath) | Sort-Object -Property Id -Unique |
             Sort-Object -Property @{ Expression = { $_.ProcessName -notlike "holler*" } })
}

# (ii) the files actually being locked, which catches every other holder: a
#      sqlite3 shell, an editor previewing the file, a backup agent, a previous
#      cargo run. Opening for WRITE with NO sharing is the check that matters --
#      it is exactly what a delete needs and will fail on.
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
        return $true
    }
}

$posProcesses = Get-HollerPosProcess
if ($posProcesses.Count -gt 0) {
    $named = ($posProcesses | ForEach-Object { "$($_.ProcessName) pid $($_.Id)" }) -join ", "
    Fail-WithAction `
        "a Holler POS process is already running ($named). NOTHING WAS STARTED." `
        "Close the till window, or run .\scripts\demo-down.ps1, then re-run. A running POS holds the edge database open and owns $PosVitePort, $LanPort and $CaptainPort."
}
Write-Ok "no Holler POS process is running"

$busy = @()
foreach ($port in ($PreflightPorts | Sort-Object -Unique)) {
    $owner = Get-ListenerOwningPid $port
    if ($owner) { $busy += "port ${port}: $(Describe-Pid $owner)" }
}
if ($busy.Count -gt 0) {
    # THE START TIME IS PRINTED FOR EVERY HOLDER, and the next action leads with
    # it: on the sweep that produced this rule, three of four listeners were the
    # operator's own, started minutes earlier, and killing them took down the
    # stack the run was meant to leave alone.
    Fail-WithAction `
        "these demo ports are already in use. NOTHING WAS STARTED.`n         $($busy -join "`n         ")" `
        "CHECK EACH START TIME ABOVE FIRST -- one of these may be yours, started minutes ago, and killing it takes down the stack you meant to keep. Run .\scripts\demo-down.ps1 to stop what a previous demo-up started, or stop the named pid yourself, then re-run."
}
Write-Ok "ports free: $(($PreflightPorts | Sort-Object -Unique) -join ', ')"

$edgePlaintextPath = Join-Path $EdgeDataDir "edge.db"
$edgeSealedPath    = Join-Path $EdgeDataDir "edge.db.enc"
$lockedFiles = @(
    $edgePlaintextPath, "$edgePlaintextPath-wal", "$edgePlaintextPath-shm", $edgeSealedPath
) | Where-Object { Test-FileIsLocked $_ }
if ($lockedFiles.Count -gt 0) {
    # Windows exposes no handle-to-pid mapping without an external tool, so this
    # names the candidates it can enumerate BY PROCESS NAME rather than inventing
    # a pid. A path match on 'holler' hits every process running out of this
    # repository, which is a guess dressed as a finding.
    $candidates = @(Get-Process -Name "holler-pos", "holler_pos", "sqlite3", "devseed" -ErrorAction SilentlyContinue |
        ForEach-Object { "$($_.ProcessName) pid $($_.Id)" })
    $who = if ($candidates.Count -gt 0) {
        " Processes that could plausibly hold it: $($candidates -join ', ')."
    } else {
        " No process this script can name accounts for it -- the holder is something it cannot see."
    }
    Fail-WithAction `
        "another process is holding $($lockedFiles -join ', '). NOTHING WAS STARTED.$who" `
        "Close whatever has the edge database open -- a POS, a sqlite shell, an editor previewing the file, or a backup agent -- and re-run. If you cannot find it, 'handle64.exe $edgePlaintextPath' (Sysinternals) names the owner."
}
Write-Ok "nothing holds $edgeSealedPath or $edgePlaintextPath"

if ((Test-Path $edgePlaintextPath) -and (-not $Fresh)) {
    # Gap A6: no exit path on this build fires RunEvent::Exit, so a plaintext
    # leftover is the NORMAL state after a POS is closed. Reported, never deleted
    # here, and never offered as a recovery route -- it is the data-loss bug
    # itself (operator's ruling, docs\RESUME.md).
    Write-Warn "a plaintext edge.db is sitting beside the sealed file (gap A6 -- no exit path seals)."
    Write-Warn "the sealed .enc is only as current as the last successful seal. -Fresh clears both."
}

# The phone reaches nothing through a closed firewall, and that failure appears
# on the phone, at the demo, and nowhere earlier. Checked, never created:
# creating a rule needs elevation, and a script that silently opens ports on a
# laptop is worse than one that says which are shut.
$fwRules = @(Get-NetFirewallRule -DisplayName "Holler demo -*" -ErrorAction SilentlyContinue |
    Where-Object { $_.Enabled -eq $true -and $_.Direction -eq "Inbound" -and $_.Action -eq "Allow" })
if ($fwRules.Count -eq 0) {
    Write-Warn "no enabled inbound 'Holler demo -*' firewall rules found."
    Write-Warn "the phone and any second machine will be refused on $LanPort / $CaptainPort / $KdsPort."
    Write-Warn "add them from an ELEVATED shell -- docs\lan-setup.md section 4 has the three commands."
} else {
    Write-Ok "$($fwRules.Count) enabled inbound 'Holler demo -*' firewall rule(s) present (docs\lan-setup.md section 4)"
}

# =====================================================================
Write-Step 2 "infra containers (postgres, redis, nats)"
# =====================================================================
if ($SkipInfra) {
    Write-Note "-SkipInfra: not touching Docker"
} elseif ($WhatIf) {
    Write-Note "-WhatIf: would run 'docker compose up -d postgres redis nats' in $repoRoot"
} else {
    # NOT 'make dev' and NOT a bare 'docker compose up': the compose file's
    # backend service fails to build (go build -o /out/api ./cmd/api exits 1)
    # and is not used here. Docker Desktop does not autostart on this box.
    $engineUp = $false
    try {
        docker info --format '{{.ServerVersion}}' *> $null
        $engineUp = ($LASTEXITCODE -eq 0)
    } catch { $engineUp = $false }
    if (-not $engineUp) {
        $dd = Join-Path $env:ProgramFiles "Docker\Docker\Docker Desktop.exe"
        if (-not (Test-Path $dd)) {
            Fail-WithAction "the Docker engine is down and Docker Desktop was not found at $dd." `
                            "Start the Docker engine by hand, then re-run."
        }
        Write-Note "engine down -- launching Docker Desktop and waiting (up to 180s)"
        Start-Process $dd | Out-Null
        $deadline = (Get-Date).AddSeconds(180)
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Seconds 5
            try {
                docker info *> $null
                if ($LASTEXITCODE -eq 0) { $engineUp = $true; break }
            } catch { }
        }
        if (-not $engineUp) {
            Fail-WithAction "the Docker engine did not come up within 180s." `
                            "Open Docker Desktop, wait for it to report Running, then re-run."
        }
    }
    Push-Location $repoRoot
    try {
        docker compose up -d postgres redis nats
        if ($LASTEXITCODE -ne 0) {
            Fail-WithAction "'docker compose up -d postgres redis nats' exited $LASTEXITCODE." `
                            "Read the compose output above. Do NOT fall back to 'make dev' or a bare 'docker compose up' -- the compose file's backend service does not build."
        }
    } finally { Pop-Location }
    Write-Ok "postgres, redis, nats up"
}

# =====================================================================
Write-Step 3 "reset cloud + edge to the demo seed"
# =====================================================================
if (-not $Fresh) {
    Write-Note "-Fresh not given: keeping whatever state the cloud and the edge already hold"
    Write-Note "a seed change that renumbers ids is ONLY safe through a reset -- the cloud seeder"
    Write-Note "upserts without pruning and dies on idx_menu_item_variant_one_default."
} else {
    $resetArgs = @{ Force = $true }
    if ($DbKeyHex -ne "") { $resetArgs["DbKeyHex"] = $DbKeyHex }
    if ($PSBoundParameters.ContainsKey("RepoRoot"))    { $resetArgs["RepoRoot"] = $RepoRoot }
    if ($PSBoundParameters.ContainsKey("EdgeDataDir")) { $resetArgs["EdgeDataDir"] = $EdgeDataDir }
    $resetArgs["CloudBaseUrl"]      = $CloudBaseUrl
    $resetArgs["BackendPort"]       = $BackendPort
    $resetArgs["DatabaseUrl"]       = $DatabaseUrl
    $resetArgs["PostgresContainer"] = $PostgresContainer
    if ($WhatIf) {
        Write-Note "-WhatIf: would run scripts\demo-reset.ps1 -Force with $(($resetArgs.Keys | Sort-Object) -join ', ')"
        Write-Note "         that is DESTRUCTIVE: it drops the public Postgres schema and deletes the edge database"
    } else {
        # A HASHTABLE, not an array. Array splatting binds POSITIONALLY, so
        # @("-Force") binds the literal string "-Force" to the first positional
        # parameter -- which is $DbKeyHex. Cost dev-up.ps1 a debugging session on
        # 2026-08-27.
        & (Join-Path $PSScriptRoot "demo-reset.ps1") @resetArgs
        if ($LASTEXITCODE -ne 0) {
            Fail-WithAction "scripts\demo-reset.ps1 exited $LASTEXITCODE." `
                            "Read its output above -- it names the next action for every failure it has."
        }
        Write-Ok "cloud and edge reset to the demo seed"
        Write-Note "the reset started its own backend; step 4 replaces it for the reason in this file's header"
    }
}

# =====================================================================
Write-Step 4 "backend API in its own window, verified by a NEW pid"
# =====================================================================
$oldBackendPid = Get-ListenerOwningPid $BackendPort
if ($WhatIf) {
    if ($oldBackendPid) { Write-Note "-WhatIf: would stop $(Describe-Pid $oldBackendPid) on port $BackendPort" }
    Write-Note "-WhatIf: would start 'go run ./cmd/api' with PORT=$BackendPort and HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS=$LoginRateLimitAttempts"
    Write-Note "-WhatIf: would then confirm /health answers AND that a DIFFERENT pid owns $BackendPort"
} else {
    if ($oldBackendPid) {
        Write-Note "port $BackendPort is owned by $(Describe-Pid $oldBackendPid) -- stopping it"
        Stop-Process -Id $oldBackendPid -Force -ErrorAction SilentlyContinue
    } else {
        Write-Note "nothing is listening on port $BackendPort"
    }
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-ListenerOwningPid $BackendPort) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    if (Get-ListenerOwningPid $BackendPort) {
        Fail-WithAction "port $BackendPort still has a listener 30s after stopping pid $oldBackendPid." `
                        "Find what keeps re-binding it (Get-NetTCPConnection -LocalPort $BackendPort) and stop it, then re-run."
    }
    Write-Note "port $BackendPort confirmed free"

    # PORT IS SET EXPLICITLY. backend/internal/platform/config/config.go reads
    # PORT (default 8080) and nothing else; demo-reset.ps1's -BackendPort does
    # NOT reach the backend it launches, for exactly this reason, which is how a
    # run aimed at port 8099 bound 8080 and took down the operator's backend.
    $cmd = "`$env:PORT='$BackendPort'; " +
           "`$env:DATABASE_URL='$DatabaseUrl'; " +
           "`$env:TOKEN_SIGNING_KEY='$TokenSigningKey'; " +
           "`$env:HOLLER_CORS_ALLOWED_ORIGINS='$AdminOrigin'; " +
           "`$env:HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS='$LoginRateLimitAttempts'; " +
           "go run ./cmd/api"
    $backendWindow = Start-ServiceWindow "holler-backend (demo-up)" (Join-Path $repoRoot "backend") $cmd
    Record-Process "backend-window" $backendWindow "powershell wrapper; 'go run' execs a child with a different pid"
    Write-Note "launched window pid $($backendWindow.Id) ('go run' execs its own child -- the port's owner is checked below)"

    $deadline = (Get-Date).AddSeconds(90)
    $healthy = $false
    do {
        try {
            Invoke-RestMethod -Uri "$CloudBaseUrl/health" -TimeoutSec 3 | Out-Null
            $healthy = $true
        } catch { Start-Sleep -Seconds 2 }
    } while ((-not $healthy) -and (Get-Date) -lt $deadline)
    if (-not $healthy) {
        Fail-WithAction "the backend did not answer $CloudBaseUrl/health within 90s." `
                        "Read the 'holler-backend (demo-up)' window for a Go build or runtime error."
    }
    $newBackendPid = Get-ListenerOwningPid $BackendPort
    if (-not $newBackendPid) {
        Fail-WithAction "the backend answered /health but no process owns port $BackendPort." `
                        "Something else is serving $CloudBaseUrl. Inspect it before continuing -- this run would be pointed at a backend it does not control."
    }
    if ($oldBackendPid -and ($newBackendPid -eq $oldBackendPid)) {
        # THE OLD PROCESS ANSWERS IDENTICALLY, so a health check alone is
        # consistent with "nothing restarted". That has already cost a debugging
        # detour: a rate-limit window the restart was meant to clear survived,
        # and the unchanged symptom read as a credential fault.
        Fail-WithAction "port $BackendPort is owned by the SAME pid ($oldBackendPid) that was supposedly stopped." `
                        "The restart did not happen. Do not trust this backend's in-memory state (the login rate limiter especially); investigate before continuing."
    }
    Record-Process "backend" (Get-Process -Id $newBackendPid -ErrorAction SilentlyContinue) "owns port $BackendPort"
    Write-Ok "backend verified: port $BackendPort now owned by $(Describe-Pid $newBackendPid)"
    Write-Note "login budget widened to $LoginRateLimitAttempts attempts / 15 min, this dev backend only (S-BE-09)"
}

# =====================================================================
Write-Step 5 "bootstrap -- edge seed, env files, POS and KDS credentials"
# =====================================================================
$resolvedSinkDir = if ($PrinterFileSinkDir -ne "") { $PrinterFileSinkDir } else { Join-Path $repoRoot ".dev-prints" }

$bootstrapArgs = @{ WithBilling = $true; PrinterFileSinkDir = $resolvedSinkDir }
if ($DbKeyHex -ne "")     { $bootstrapArgs["DbKeyHex"] = $DbKeyHex }
if ($LanHost -ne "")      { $bootstrapArgs["LanHost"]  = $LanHost }
if ($UpiVpa -ne "")       { $bootstrapArgs["UpiVpa"] = $UpiVpa }
if ($UpiPayeeName -ne "") { $bootstrapArgs["UpiPayeeName"] = $UpiPayeeName }
if ($PSBoundParameters.ContainsKey("RepoRoot"))    { $bootstrapArgs["RepoRoot"] = $RepoRoot }
if ($PSBoundParameters.ContainsKey("EdgeDataDir")) { $bootstrapArgs["EdgeDataDir"] = $EdgeDataDir }
$bootstrapArgs["CloudBaseUrl"] = $CloudBaseUrl
$bootstrapArgs["LanPort"] = $LanPort
$bootstrapArgs["SyncEnrollEmail"] = $SyncEnrollEmail
$bootstrapArgs["SyncEnrollPassword"] = $SyncEnrollPassword
# -SkipInfra always: step 2 above owns the containers, and letting the bootstrap
# start them too would mean two places deciding what infra means.
$bootstrapArgs["SkipInfra"] = $true

if ($WhatIf) {
    Write-Note "-WhatIf: would run scripts\dev-bootstrap.ps1 with $(($bootstrapArgs.Keys | Sort-Object) -join ', ')"
    Write-Note "         -UpiVpa/-UpiPayeeName are passed only when given; the bootstrap REMEMBERS the last value"
    Write-Note "         prints would go to $resolvedSinkDir"
} else {
    & (Join-Path $repoRoot "scripts\dev-bootstrap.ps1") @bootstrapArgs
    if ($LASTEXITCODE -ne 0) {
        Fail-WithAction "scripts\dev-bootstrap.ps1 exited $LASTEXITCODE." `
                        "Read its output above. A red [3c/4] means the KDS will throw a config error at startup and must be fixed before step 9."
    }
    Write-Ok "bootstrap complete"
    Write-Note "READ ITS OUTPUT: '[3c/4] KDS credential ENABLED' in cyan, and 'UPI QR ENABLED: <vpa>' in cyan."
    Write-Note "A yellow 'UPI QR DISABLED' means NO QR renders at all on the bill screen or the receipt."
    Write-Ok "prints go to $resolvedSinkDir (.escpos bytes + .txt + .html + .pdf, the PDF opens on print)"
}

# =====================================================================
Write-Step 6 "rebuild apps\captain\dist (the POS serves this directory from disk)"
# =====================================================================
$captainDir = Join-Path $repoRoot "apps\captain"
if ($WhatIf) {
    Write-Note "-WhatIf: would run 'pnpm build' in $captainDir"
} else {
    if (-not (Test-Path (Join-Path $captainDir "node_modules"))) {
        Write-Note "installing captain dependencies (first run)"
        Push-Location $captainDir
        try { pnpm install } finally { Pop-Location }
    }
    Push-Location $captainDir
    try {
        pnpm build
        if ($LASTEXITCODE -ne 0) {
            Fail-WithAction "'pnpm build' in apps\captain exited $LASTEXITCODE." `
                            "Read the tsc/vite output above. The POS serves apps\captain\dist from disk, so a stale or missing dist is a phone that loads nothing."
        }
    } finally { Pop-Location }
    $indexPath = Join-Path $captainDir "dist\index.html"
    if (-not (Test-Path $indexPath)) {
        Fail-WithAction "'pnpm build' reported success but $indexPath does not exist." `
                        "Check apps\captain\vite.config.ts's outDir. The POS serves this exact path (captain.rs dist_dir())."
    }
    Write-Ok "captain dist built: $indexPath ($((Get-Item $indexPath).LastWriteTime))"
}

# =====================================================================
Write-Step 7 "WAITER device -- enrol or rotate, and capture its token"
# =====================================================================
# Rotated on EVERY run, deliberately -- see decision 2 in this file's header.
$waiterToken = $null
$waiterDeviceId = $null
if ($WhatIf) {
    Write-Note "-WhatIf: would log in as $SyncEnrollEmail and enrol or rotate a WAITER device named '$WaiterDeviceName'"
    Write-Note "-WhatIf: the token is issued ONCE and is never written to disk by this script"
} else {
    $posEnvFile = Join-Path $repoRoot "apps\pos\.env.dev"
    if (-not (Test-Path $posEnvFile)) {
        Fail-WithAction "$posEnvFile does not exist, so this run cannot learn the tenant and outlet ids." `
                        "Step 5 writes it. If step 5 was skipped, run scripts\dev-bootstrap.ps1 first."
    }
    # Read for the two ids ONLY. The key in this file is never read, logged or
    # passed on by this script -- it reaches the sub-scripts as -DbKeyHex from
    # the operator's own command line.
    $envValues = @{}
    foreach ($line in Get-Content $posEnvFile) {
        $trimmed = $line.Trim()
        if ($trimmed -eq "" -or $trimmed.StartsWith("#")) { continue }
        if ($trimmed -match '^([A-Za-z_][A-Za-z0-9_]*)=(.*)$') { $envValues[$Matches[1]] = $Matches[2].Trim() }
    }
    $tenantId = $envValues['HOLLER_TENANT_ID']
    $outletId = $envValues['HOLLER_OUTLET_ID']
    if ((-not $tenantId) -or (-not $outletId)) {
        Fail-WithAction "$posEnvFile carries no HOLLER_TENANT_ID / HOLLER_OUTLET_ID." `
                        "Re-run scripts\dev-bootstrap.ps1 -- it writes both."
    }

    $headers = $null
    try {
        $loginBody = @{ email = $SyncEnrollEmail; password = $SyncEnrollPassword; outlet_id = $outletId } | ConvertTo-Json
        $session = Invoke-RestMethod -Uri "$CloudBaseUrl/auth/login" -Method Post -Body $loginBody `
            -ContentType 'application/json' -Headers @{ 'X-Tenant-ID' = $tenantId }
        $headers = @{ 'X-Tenant-ID' = $tenantId; 'Authorization' = "Bearer $($session.access_token)" }
    } catch {
        $detail = $_.ErrorDetails.Message
        if (-not $detail) { $detail = $_.Exception.Message }
        # A 401 here has THREE causes that look identical from outside: a wrong
        # password, the login rate limiter (ADR-012 leaks nothing about which),
        # and -- the one that has actually bitten -- a dev database whose user
        # rows were overwritten with '$argon2id$fixture-hash-not-a-r...' by a Go
        # test run against HOLLER_TEST_DATABASE_URL pointed at it.
        Fail-WithAction "could not log in as $SyncEnrollEmail against $CloudBaseUrl : $detail" `
                        "A 401 is one of three things and they are indistinguishable by design: a wrong password, the login rate limiter, or the dev database's user rows overwritten by a Go test fixture hash. Re-run with -Fresh, which reseeds them."
    }

    # POST /devices/enroll matches by (tenant, outlet, name) and 409s if the
    # device exists, so the existing id has to come from somewhere. There is
    # deliberately no device LIST route: on a LOCAL cloud Postgres is the
    # authority -- INCLUDING when it says there is no such device.
    $isLocal = $false
    try {
        $targetHost = ([Uri]$CloudBaseUrl).Host
        $isLocal = ($targetHost -eq "localhost" -or $targetHost -eq "127.0.0.1" -or $targetHost -eq "::1")
    } catch { $isLocal = $false }

    $existingId = $null
    if ($isLocal) {
        $existingId = (docker exec $PostgresContainer psql -U holler -d holler -t -A -c `
            "SELECT id FROM device WHERE outlet_id = '$outletId' AND name = '$WaiterDeviceName' AND revoked_at IS NULL LIMIT 1;" 2>$null)
        if ($existingId) { $existingId = $existingId.Trim() }
    }

    $waiter = $null
    if ($existingId) {
        Write-Note "WAITER device $existingId already exists -- rotating its credential"
        try {
            $waiter = Invoke-RestMethod -Uri "$CloudBaseUrl/devices/$existingId/credentials/rotate" -Method Post `
                -Body (@{ label = "demo-up" } | ConvertTo-Json) -ContentType 'application/json' -Headers $headers
            $waiterDeviceId = $existingId
        } catch {
            # 404 = THE DEVICE IS GONE, NOT THE ROUTE. A schema drop, a restored
            # database, a device deleted by hand. Enrolling is the correct
            # recovery; every other status still throws, because a 401 means the
            # principal lacks outlet.manage and enrolling a second device would
            # hide that.
            $statusCode = $null
            if ($_.Exception.Response) { $statusCode = [int]$_.Exception.Response.StatusCode }
            if ($statusCode -ne 404) {
                $detail = $_.ErrorDetails.Message
                if (-not $detail) { $detail = $_.Exception.Message }
                Fail-WithAction "rotating the WAITER credential failed: $detail" `
                                "A 401 means $SyncEnrollEmail does not hold outlet.manage. Any other status: read the backend window."
            }
            Write-Note "device $existingId is gone from $CloudBaseUrl (404 on rotate) -- enrolling a new one"
            $existingId = $null
        }
    }
    if (-not $waiter) {
        try {
            $waiter = Invoke-RestMethod -Uri "$CloudBaseUrl/devices/enroll" -Method Post `
                -Body (@{ outlet_id = $outletId; kind = 'WAITER'; name = $WaiterDeviceName; label = "demo-up" } | ConvertTo-Json) `
                -ContentType 'application/json' -Headers $headers
            $waiterDeviceId = $waiter.device_id
        } catch {
            $detail = $_.ErrorDetails.Message
            if (-not $detail) { $detail = $_.Exception.Message }
            Fail-WithAction "enrolling the WAITER device failed: $detail" `
                            "If this says 'already enrolled', a device named '$WaiterDeviceName' exists but this run could not find its id (the Postgres lookup only runs against a LOCAL cloud). Rotate it by hand, or pass a different -WaiterDeviceName."
        }
    }
    $waiterToken = $waiter.token
    if ([string]::IsNullOrWhiteSpace($waiterToken)) {
        Fail-WithAction "the enrol/rotate response carried no token." `
                        "Read the backend window. Without a token the phone cannot pair and demo step 1a is cut."
    }
    Write-Ok "WAITER device $waiterDeviceId has a FRESH credential (any previous token is now revoked)"
    Write-Note "its config_version is above the edge's cursor, so the till mints the device row on its next"
    Write-Note "config pull -- which is what makes S-CAP-20 structurally impossible on this run."
}

# =====================================================================
Write-Step 8 "POS in its own window -- run-dev.ps1, and it is THE ONLY VITE"
# =====================================================================
# There is ONE way to start the POS. tauri.conf.json's beforeDevCommand is
# 'pnpm dev', so 'tauri dev' ALWAYS starts Vite itself, and vite.config.ts sets
# strictPort, so a second one fails on $PosVitePort. A stale Vite that does end
# up serving the window serves a bundle built in a different environment -- that
# is exactly how the UPI QR went missing from the bill screen on 2026-09-12.
if ($WhatIf) {
    Write-Note "-WhatIf: would start apps\pos\run-dev.ps1 in its own window and wait for $CaptainPort and $LanPort"
} else {
    $posDir = Join-Path $repoRoot "apps\pos"
    if (-not (Test-Path (Join-Path $posDir "node_modules"))) {
        Write-Note "installing POS dependencies (first run)"
        Push-Location $posDir
        try { pnpm install } finally { Pop-Location }
    }
    $posWindow = Start-ServiceWindow "holler-pos (demo-up)" $posDir ".\run-dev.ps1"
    Record-Process "pos-window" $posWindow "runs run-dev.ps1; the till binds $PosVitePort, $LanPort, $CaptainPort"
    Write-Note "launched window pid $($posWindow.Id) -- a cold Rust build can take minutes"
    Write-Note "LNK1104 'cannot open file ...exe' is McAfee holding a fresh binary, not a code error: re-run."

    # Eight minutes: a cold cargo build on this box is genuinely that slow, and a
    # timeout shorter than the build turns a working run into a false failure.
    $captainOwner = Wait-ForListener $CaptainPort 480
    if (-not $captainOwner) {
        Fail-WithAction "nothing is listening on the captain port $CaptainPort 8 minutes after starting the POS." `
                        "Read the 'holler-pos (demo-up)' window. If the Rust build failed with LNK1104, re-run -- cargo caches every binary that linked. If the till window is up but $CaptainPort is silent, the listener failed to bind and said so in that window (a bind failure is deliberately never fatal to the till)."
    }
    $lanOwner = Get-ListenerOwningPid $LanPort
    if (-not $lanOwner) {
        Fail-WithAction "the captain listener is up on $CaptainPort but nothing owns the KDS LAN port $LanPort." `
                        "Read the 'holler-pos (demo-up)' window for the LAN server's bind error. Without it the KDS can never connect."
    }
    Record-Process "pos" (Get-Process -Id $captainOwner -ErrorAction SilentlyContinue) "owns $LanPort and $CaptainPort"
    $viteOwner = Get-ListenerOwningPid $PosVitePort
    if ($viteOwner) {
        Record-Process "pos-vite" (Get-Process -Id $viteOwner -ErrorAction SilentlyContinue) "owns $PosVitePort"
    }
    Write-Ok "POS up: $(Describe-Pid $captainOwner) owns $LanPort and $CaptainPort"
}

# =====================================================================
Write-Step 9 "KDS in its own window"
# =====================================================================
$kdsUrl = "http://localhost:$KdsPort/"
if ($WhatIf) {
    Write-Note "-WhatIf: would start 'pnpm dev --mode dev' in apps\kds and open $kdsUrl"
} else {
    $kdsDir = Join-Path $repoRoot "apps\kds"
    if (-not (Test-Path (Join-Path $kdsDir "node_modules"))) {
        Write-Note "installing KDS dependencies (first run)"
        Push-Location $kdsDir
        try { pnpm install } finally { Pop-Location }
    }
    # --mode dev, NOT a plain 'pnpm dev': Vite's default mode is 'development',
    # under which it ignores .env.dev entirely and the KDS throws
    # 'VITE_KDS_LAN_URL is not configured' at startup.
    $kdsWindow = Start-ServiceWindow "holler-kds (demo-up)" $kdsDir "pnpm dev --mode dev"
    Record-Process "kds-window" $kdsWindow "vite dev server on $KdsPort"
    $kdsOwner = Wait-ForListener $KdsPort 120
    if (-not $kdsOwner) {
        Fail-WithAction "the KDS dev server did not listen on $KdsPort within 120s." `
                        "Read the 'holler-kds (demo-up)' window."
    }
    Record-Process "kds" (Get-Process -Id $kdsOwner -ErrorAction SilentlyContinue) "owns $KdsPort"
    Write-Ok "KDS dev server: $(Describe-Pid $kdsOwner)"

    # WHAT THIS CHECKS AND WHAT IT DOES NOT. The env file naming the right host,
    # and the LAN port accepting a TCP connection, are each necessary and neither
    # is sufficient: "connected" is a WebSocket the browser opens, and the only
    # place it is visible is the indicator on the KDS screen.
    $kdsEnvFile = Join-Path $kdsDir ".env.dev"
    $expectedLanUrl = $null
    if ($LanHost -ne "") { $expectedLanUrl = "ws://${LanHost}:$LanPort/kds" }
    if (Test-Path $kdsEnvFile) {
        $lanLine = @(Get-Content $kdsEnvFile | Where-Object { $_ -match '^VITE_KDS_LAN_URL=' }) | Select-Object -First 1
        if (-not $lanLine) {
            Write-Warn "apps\kds\.env.dev carries no VITE_KDS_LAN_URL -- the KDS will throw at startup"
        } else {
            $actual = ($lanLine -split '=', 2)[1].Trim()
            if ($expectedLanUrl -and ($actual -ne $expectedLanUrl)) {
                Fail-WithAction "apps\kds\.env.dev says VITE_KDS_LAN_URL=$actual but -LanHost $LanHost implies $expectedLanUrl." `
                                "A stale address here is a KDS that loads, looks fine and never connects. Re-run with the correct -LanHost."
            }
            if ($actual -match 'localhost|127\.0\.0\.1') {
                Write-Warn "VITE_KDS_LAN_URL is $actual -- fine on this machine, unreachable from any other. Pass -LanHost for a second screen."
            }
            Write-Ok "VITE_KDS_LAN_URL=$actual"
        }
        if (-not @(Get-Content $kdsEnvFile | Where-Object { $_ -match '^VITE_KDS_DEVICE_TOKEN=.+' })) {
            Fail-WithAction "apps\kds\.env.dev has no VITE_KDS_DEVICE_TOKEN." `
                            "The KDS throws a config error at startup without it. That is the red [3c/4] in step 5's output -- fix its cause and re-run."
        }
    } else {
        Write-Warn "no apps\kds\.env.dev -- the KDS will throw a config error at startup"
    }
    $tcp = Test-NetConnection -ComputerName "127.0.0.1" -Port $LanPort -InformationLevel Quiet -WarningAction SilentlyContinue
    if ($tcp) { Write-Ok "TCP connect to the LAN port $LanPort succeeds (NOT proof the KDS connected -- read the indicator)" }
    else      { Write-Warn "TCP connect to $LanPort failed even locally; the KDS cannot connect" }

    Start-Process $kdsUrl | Out-Null
    Write-Note "opened $kdsUrl in the default browser"
}

# =====================================================================
Write-Step 10 "the checks to make on screen, before anyone is watching"
# =====================================================================
$reportedLanHost = if ($LanHost -ne "") { $LanHost } else { "<the address the bootstrap printed in step 5>" }
$captainUrl = "http://${reportedLanHost}:$CaptainPort/"

Write-Host ""
Write-Host "  EVERY LINE BELOW HAS A CHECK YOU CAN SEE. A step whose result you did not" -ForegroundColor White
Write-Host "  look at has not been done." -ForegroundColor White
Write-Host ""
Write-Host "  1. THE TILL    sign in cashier@holler.test / holler123" -ForegroundColor White
Write-Host "     - the outlet name reads SHINJUKU YAKITORI, not a dev fixture name" -ForegroundColor DarkGray
Write-Host "     - the menu renders real categories" -ForegroundColor DarkGray
Write-Host "     - the sync banner is ABSENT, not empty" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  2. THE QR      ring up anything, open the bill" -ForegroundColor White
Write-Host "     - 'Scan to pay via UPI' with a QR, the amount and the payee beneath it" -ForegroundColor DarkGray
Write-Host "     - 'UPI QR not configured' means the VPA never reached the bundle: re-run with" -ForegroundColor DarkGray
Write-Host "       -UpiVpa. Worth thirty seconds -- the screen looks finished without it." -ForegroundColor DarkGray
Write-Host ""
Write-Host "  3. THE KDS     $kdsUrl" -ForegroundColor White
Write-Host "     - the indicator reads CONNECTED (this script could not check that for you)" -ForegroundColor DarkGray
Write-Host "     - loads-but-never-connects is almost always a stale VITE_KDS_LAN_URL" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  4. THE PHONE   join the hotspot, then open" -ForegroundColor White
Write-Host "     $captainUrl" -ForegroundColor Green
if ($waiterToken) {
    Write-Host ""
    Write-Host "     PAIR TOKEN (issued once; this script writes it nowhere):" -ForegroundColor White
    Write-Host "     $waiterToken" -ForegroundColor Green
    Write-Host ""
    Write-Host "     - the phone must leave the pair screen and reach TABLES. That is proof the" -ForegroundColor DarkGray
    Write-Host "       credential verified against the till, not just that the page loaded." -ForegroundColor DarkGray
    Write-Host "     - 'That device token was rejected' means EITHER a mistyped token OR a" -ForegroundColor DarkGray
    Write-Host "       credential that has not synced yet. Same message for both. The till's" -ForegroundColor DarkGray
    Write-Host "       window prints 'config pull applied a new bundle' when it lands -- the only" -ForegroundColor DarkGray
    Write-Host "       in-product signal, and the pull runs every 60s." -ForegroundColor DarkGray
} elseif ($WhatIf) {
    Write-Host "     PAIR TOKEN: -WhatIf enrolled nothing, so there is none." -ForegroundColor Yellow
}
Write-Host ""
Write-Host "  5. THE CHAIN   pick a table, add an item, send -- a ticket appears on the KDS." -ForegroundColor White
Write-Host "     - this is the least-exercised path in the whole demo. Rehearse it properly." -ForegroundColor DarkGray
Write-Host "     - it is also the ONLY thing that proves the WAITER's device row was minted;" -ForegroundColor DarkGray
Write-Host "       pairing alone never touches it." -ForegroundColor DarkGray
Write-Host ""
Write-Host "  DO NOT BILL A BAR ITEM. Alcohol sits on a zero-rate profile (VAT is inexpressible" -ForegroundColor Yellow
Write-Host "  under contracts 0.8.2), so a bar line prints a tax figure that is deliberately 0." -ForegroundColor Yellow

if (-not $WhatIf) {
    Save-RecordedState
    Write-Host ""
    Write-Host "Started by this run (recorded in $StateFile):" -ForegroundColor Green
    foreach ($p in $script:Recorded.processes) {
        Write-Host ("  {0,-15} {1} pid {2}" -f $p.role, $p.name, $p.pid) -ForegroundColor DarkGray
    }
    Write-Host ""
    Write-Host "Stop all of it with: .\scripts\demo-down.ps1" -ForegroundColor Green
}

$elapsed = (Get-Date) - $startedAt
Write-Host ""
Write-Host ("demo-up finished in {0:mm\:ss}." -f $elapsed) -ForegroundColor Green
exit 0
