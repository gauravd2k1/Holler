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

    # The repository this run operates on. Normally computed from the script's
    # own location; a parameter because scripts\agent-guard.ps1 requires an
    # agent shell to name a scratch tree EXPLICITLY rather than inherit the
    # real one.
    [string]$RepoRoot = "",

    # Where the POS keeps its edge database. Must match Tauri's app_data_dir()
    # for identifier com.holler.pos, or the POS will open a different (empty)
    # database than the one this script seeds.
    [Alias('DataDir')]
    [string]$EdgeDataDir = (Join-Path $env:APPDATA "com.holler.pos"),

    # Seeded KDS device row (edge/database/src/bin/devseed.rs KDS_DEVICE_ID).
    # Fixed like every other devseed id -- re-stated here rather than parsed
    # out of the Rust seeder's output because it prints nothing today.
    [string]$KdsDeviceId = "0191a000-0000-7000-8000-00000000000d",

    # Bind port for the KDS LAN server, embedded in the POS
    # (apps/pos/src-tauri/src/state.rs::DEFAULT_LAN_BIND_ADDR) or run
    # standalone (edge/device/src/bin/kds_lan_server.rs). Pinned to 9310.
    [int]$LanPort = 9310,

    # The LAN IPv4 address a SECOND MACHINE (the KDS screen, the waiter's
    # phone) must use to reach this till. Empty means "work it out", which
    # picks the interface owning the default route -- see Get-LanIPv4 for why
    # that rule and not "the first adapter".
    #
    # SET THIS ON DEMO DAY. On a phone hotspot the till's address changes
    # every time it reconnects, and an address baked into apps\kds\.env.dev
    # from a previous network is a KDS that loads and never connects. Check it
    # with `ipconfig` and pass it explicitly: -LanHost 192.168.43.12
    [string]$LanHost = "",

    # The UPI payee the invoice screen's QR and the printed receipt encode.
    #
    # WRITTEN INTO apps\pos\.env.dev BY THIS SCRIPT, and REMEMBERED between
    # runs in the same state file that holds device ids. Before that it was
    # hand-added to .env.dev, and every bootstrap re-run silently wiped it --
    # after which NO QR RENDERS AT ALL (there is no empty-QR state; an unset
    # VPA produces nothing, with nothing on screen to say why). A day-of
    # re-run would have removed the QR from demo step 2 and nobody would have
    # known until the bill was on the screen.
    #
    # Empty and never previously set means no QR, which stays the correct
    # default: a QR aimed at nobody opens a payment app on a customer's phone
    # pointed at no payee.
    # Where the POS env file is written. A PARAMETER so the real write can be
    # aimed at a scratch path and checked, instead of a test re-implementing
    # the write and proving only that the copy agrees with itself.
    [string]$PosEnvFile = "",

    [string]$UpiVpa = "",
    [string]$UpiPayeeName = "",

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
    [string]$SyncEnrollPassword = "holler123",

    # Required to proceed when apps\pos\.env.dev already carries a
    # HOLLER_DB_KEY_HEX that DIFFERS from the incoming key (T24). Without
    # this flag the script refuses rather than silently rewriting the file:
    # a different key fails to open the existing sealed database -- it does
    # NOT create or open a different, empty one. Encryption::open_file
    # refuses outright; the crash-recovery path (T25) refuses too, before
    # touching anything on disk, rather than resealing the real database
    # under the wrong key.
    [switch]$RotateKey
)

# --- T24: NO AGENT SUPPLIES A LITERAL KEY TO THIS SCRIPT --------------------
# apps\pos\.env.dev is deny-ruled to agents specifically because it carries
# the edge database's encryption key. That rule was honoured by every agent
# in the session that motivated this change, and was bypassed anyway --
# through the front door -- because this script rewrote the file wholesale
# from whatever -DbKeyHex/HOLLER_DB_KEY_HEX it was given, and a hand-typed
# placeholder ("2222...2222") satisfied the hex/length check perfectly. A
# key comes from the operator's own environment, or is minted by the
# operator with the command this script prints below -- never typed into a
# brief, a command line or a script by an agent on the operator's behalf.

# The LAN IPv4 address a SECOND MACHINE must be able to reach, used to build
# apps/kds/.env.dev's VITE_KDS_LAN_URL and printed for the captain page.
# localhost/127.0.0.1 is never right here.
#
# THE OLD VERSION TOOK THE FIRST NON-LOOPBACK ADDRESS AND WAS WRONG ON THIS
# DEVELOPER MACHINE, WHICH IS THE ORDINARY CASE RATHER THAN AN EDGE ONE: it
# picked 172.28.176.1, the WSL Hyper-V vEthernet adapter, while the real LAN
# was 192.168.0.106 on Wi-Fi. A phone on the hotspot cannot route to a WSL
# virtual switch, so the KDS and the captain page would have pointed at an
# address that answers only on this machine -- and the failure appears at the
# demo, on the phone, and nowhere earlier.
#
# The rule now: THE INTERFACE THAT OWNS THE DEFAULT ROUTE is the one another
# device on the same network can reach. That is what "on the LAN" means, and
# it is a fact Windows already knows, rather than a guess from an adapter
# ordering nobody controls. -LanHost overrides it outright, which is what the
# demo uses when the till is on a phone hotspot whose address changes.
function Get-LanIPv4 {
    param([string]$Explicit = "")

    if ($Explicit -ne "") {
        return $Explicit
    }

    $candidates = @(Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object { $_.IPAddress -ne "127.0.0.1" -and $_.IPAddress -notlike "169.254.*" })
    if ($candidates.Count -eq 0) { return "127.0.0.1" }

    # 1. The default-route interface, lowest metric first.
    $defaultRoutes = @(Get-NetRoute -DestinationPrefix "0.0.0.0/0" -ErrorAction SilentlyContinue |
        Sort-Object -Property RouteMetric, ifMetric)
    foreach ($route in $defaultRoutes) {
        $match = $candidates | Where-Object { $_.InterfaceIndex -eq $route.ifIndex } | Select-Object -First 1
        if ($match) {
            Write-Host "       LAN address $($match.IPAddress) on '$($match.InterfaceAlias)' (the default-route interface)" -ForegroundColor DarkGray
            return $match.IPAddress
        }
    }

    # 2. No default route (an offline outlet is the NORMAL case -- ADR-013).
    #    Fall back to the first address that is not on an obviously virtual
    #    adapter, so a machine with WSL, Docker or a hypervisor installed does
    #    not hand out a switch address no phone can reach.
    $virtual = "vEthernet|WSL|Hyper-V|VirtualBox|VMware|Loopback|Bluetooth|Npcap|TAP-"
    $physical = $candidates | Where-Object { $_.InterfaceAlias -notmatch $virtual } | Select-Object -First 1
    if ($physical) {
        Write-Host "       LAN address $($physical.IPAddress) on '$($physical.InterfaceAlias)' (no default route; first non-virtual adapter)" -ForegroundColor Yellow
        return $physical.IPAddress
    }

    $fallback = $candidates | Select-Object -First 1
    Write-Host "       WARNING: only virtual adapters found; using $($fallback.IPAddress) on '$($fallback.InterfaceAlias)'." -ForegroundColor Red
    Write-Host "       A phone or a second machine will NOT reach this address. Re-run with -LanHost <ip> (see docs\lan-setup.md)." -ForegroundColor Red
    return $fallback.IPAddress
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

    # ON A LOCAL CLOUD, POSTGRES IS AUTHORITATIVE -- INCLUDING WHEN IT SAYS
    # "NO SUCH DEVICE". The state file below exists for a REMOTE cloud, where
    # this lookup cannot run at all; consulting it after a successful local
    # lookup found nothing is how a stale id survives its own database.
    #
    # That is not hypothetical: demo-reset.ps1 drops the entire public schema,
    # so every `device` row goes with it while the state file keeps naming the
    # ids. The next bootstrap then rotated a device that no longer existed and
    # the backend answered 404 -- which reads as a missing ROUTE rather than a
    # missing ROW, and sends you looking in the router.
    $localLookupRan = $false
    if ($IsLocalCloud) {
        $existingId = (docker exec holler-postgres-1 psql -U holler -d holler -t -A -c `
            "SELECT d.id FROM device d JOIN device_credential c ON c.device_id = d.id AND c.revoked_at IS NULL WHERE d.outlet_id = '$OutletId' AND d.name = '$Name' LIMIT 1;" 2>$null)
        if ($LASTEXITCODE -eq 0) { $localLookupRan = $true }
        if ($existingId) { $existingId = $existingId.Trim() }
        if ($localLookupRan -and -not $existingId) {
            # The database has spoken. Drop any stale entry so the next run
            # does not have to rediscover this.
            $map = Get-BootstrapStateMap
            if ($map.ContainsKey($stateKey)) {
                Write-Host "       state file named device $($map[$stateKey]) for '$Name', but this cloud's database has no such device -- enrolling fresh" -ForegroundColor DarkGray
                $map.Remove($stateKey)
                ($map | ConvertTo-Json) | Out-File -FilePath $script:BootstrapStateFile -Encoding ascii
            }
        }
    }
    if (-not $existingId -and -not $localLookupRan) {
        $map = Get-BootstrapStateMap
        if ($map.ContainsKey($stateKey)) { $existingId = $map[$stateKey] }
    }

    if ($existingId) {
        $rotateBody = @{ label = "dev-bootstrap" } | ConvertTo-Json
        try {
            $enrolled = Invoke-RestMethod -Uri "$CloudBaseUrl/devices/$existingId/credentials/rotate" `
                -Method Post -Body $rotateBody -ContentType 'application/json' -Headers $Headers
            Set-BootstrapStateEntry -Key $stateKey -Value $existingId
            return @{ Token = $enrolled.token; DeviceId = $existingId; Status = "rotated" }
        } catch {
            # 404 = THE DEVICE IS GONE, NOT THE ROUTE. The route exists
            # (backend/internal/outlet/device_http.go:29); what does not exist
            # is the row this id names -- a schema drop, a restored database,
            # or a device deleted by hand. Enrolling is the correct recovery
            # and the only one that leaves a usable credential behind.
            # Every other status still throws: a 401 means the caller lacks
            # outlet.manage and silently enrolling a second device would hide
            # that.
            $status = $null
            if ($_.Exception.Response) { $status = [int]$_.Exception.Response.StatusCode }
            if ($status -ne 404) { throw }
            Write-Host "       device $existingId is gone from $CloudBaseUrl (404 on rotate) -- enrolling a new one" -ForegroundColor DarkGray
            $existingId = $null
        }
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

# STRUCTURAL GUARD, FIRST THING. Dot-sourced rather than copied: if the file is
# missing this throws and the script does not run, which is the correct
# direction for a control to fail. See scripts\agent-guard.ps1 for why it
# exists.
. (Join-Path $PSScriptRoot "agent-guard.ps1")

$repoRoot = if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    Split-Path -Parent $PSScriptRoot
} else {
    $RepoRoot
}

Assert-AgentSafePaths -ScriptName "dev-bootstrap.ps1" `
    -BoundParameters $PSBoundParameters `
    -RepoRootValue $repoRoot `
    -DataDirValue $EdgeDataDir `
    -DataDirParameterName "EdgeDataDir"

# --- T24: key-quality helpers -------------------------------------------------
# Shared shape with scripts\demo-reset.ps1's copy of the same two functions
# (no shared module between the two owned scripts; kept identical on purpose).

# HEURISTIC, stated as such wherever it is used. It cannot prove a key is
# cryptographically random -- it exists to catch the ONE failure that actually
# happened: a human or an agent typing a pattern to satisfy the hex/length
# regex. Returns $null when the key looks fine, or a short reason string when
# it looks like a placeholder.
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
# of SHA-256 over the raw key bytes -- enough to tell two keys apart in
# conversation, nowhere near enough to reconstruct either.
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
# between this script and whatever invoked it with `.\dev-bootstrap.ps1`. A
# `Remove-Item Env:\X` in a `finally` therefore does not "clean up a local",
# it deletes whatever the operator's shell had, including a value the
# operator set before running this script. Every site that needs a variable
# in the current process (to be inherited by a child like `cargo run`) must
# save the caller's prior value first and restore EXACTLY that in `finally`
# -- including restoring absence when the caller had not set it, which a
# bare `Remove-Item -ErrorAction SilentlyContinue` gets right only by
# accident (it also fires when the caller DID have a value, discarding it).
# Identical copy of scripts\demo-reset.ps1's two functions -- no shared
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

    `$b = New-Object byte[] 32
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes(`$b)
    `$env:HOLLER_DB_KEY_HEX = -join (`$b | ForEach-Object { '{0:x2}' -f `$_ })

Then re-run this script. Use the SAME value every time on this machine -- a
different key fails to open the existing sealed database; it never falls back
to a different, empty one. If a crash-recovery leftover happens to be present
at the same time, a wrong key used to be able to silently OVERWRITE the real
sealed database with that leftover (T25) -- the edge crate now verifies the
key against the sealed file first and refuses, touching nothing on disk, but
the risk a wrong key represents is data loss, not merely "looks empty".
"@
}
if ($DbKeyHex -notmatch '^[0-9a-fA-F]{64}$') {
    throw "HOLLER_DB_KEY_HEX must be exactly 64 hex characters (32 bytes); got $($DbKeyHex.Length) character(s)."
}

$weakReason = Get-KeyWeaknessReason -HexKey $DbKeyHex
if ($weakReason) {
    throw @"
HOLLER_DB_KEY_HEX looks like a placeholder, not a random key: $weakReason

This is a HEURISTIC (see the comment above Get-KeyWeaknessReason): it cannot
prove a key is cryptographically random, it only catches the specific failure
that has actually happened here -- a hand-typed or pasted pattern that still
satisfies the hex/length check. NO AGENT MAY SUPPLY A LITERAL KEY to satisfy
this check either; if you are an agent reading this, stop and ask the operator
to mint one.

Mint a real one and keep it out of the repository:

    `$b = New-Object byte[] 32
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes(`$b)
    `$env:HOLLER_DB_KEY_HEX = -join (`$b | ForEach-Object { '{0:x2}' -f `$_ })

Then re-run this script.
"@
}

# --- 0b. refuse to silently rotate the key apps\pos\.env.dev already carries -
# A different key fails to open the existing sealed database. Read the
# existing file (this script may; an agent's deny-rule on it is unaffected)
# and compare by FINGERPRINT ONLY -- neither key is ever printed.
$posEnvFile = Join-Path $repoRoot "apps\pos\.env.dev"
if (Test-Path $posEnvFile) {
    $existingKeyLine = Get-Content $posEnvFile -ErrorAction SilentlyContinue |
        Where-Object { $_ -match '^HOLLER_DB_KEY_HEX=' } |
        Select-Object -First 1
    if ($existingKeyLine) {
        $existingKeyHex = ($existingKeyLine -replace '^HOLLER_DB_KEY_HEX=', '').Trim()
        if ($existingKeyHex -and $existingKeyHex -ne $DbKeyHex) {
            $existingFp = Get-KeyFingerprint -HexKey $existingKeyHex
            $incomingFp = Get-KeyFingerprint -HexKey $DbKeyHex
            if (-not $RotateKey) {
                throw @"
apps\pos\.env.dev already carries a HOLLER_DB_KEY_HEX that DIFFERS from the
key this run would use.

  existing key fingerprint: $existingFp
  incoming key fingerprint: $incomingFp

Proceeding would silently rewrite the file with the new key. The existing
sealed database at this machine's edge data directory was encrypted under the
OLD key and would become UNOPENABLE under the new one -- the POS fails to
open it and reports an error; it does not fall back to a different, empty
database.

If this rotation is intentional, re-run with -RotateKey. Otherwise, set
-DbKeyHex / `$env:HOLLER_DB_KEY_HEX to the SAME key already in
apps\pos\.env.dev (fingerprint $existingFp above) and re-run.
"@
            } else {
                Write-Host "WARNING: rotating HOLLER_DB_KEY_HEX (-RotateKey given)." -ForegroundColor Yellow
                Write-Host "  existing key fingerprint: $existingFp" -ForegroundColor Yellow
                Write-Host "  incoming key fingerprint: $incomingFp" -ForegroundColor Yellow
                Write-Host "  the existing sealed edge database is now UNOPENABLE under the new key." -ForegroundColor Yellow
            }
        }
    }
}

Write-Host "Holler dev bootstrap" -ForegroundColor Cyan
Write-Host "repo: $repoRoot"

# --- preflight: NOTHING MAY HOLD THE EDGE DATABASE ---------------------------
# Step 3 seeds the edge database, which means opening and rewriting
# edge.db/edge.db.enc. With the POS running that fails with
# "os error 32: The process cannot access the file because it is being used by
# another process" -- AFTER steps 1 and 2 have already brought up
# infrastructure and reseeded the cloud, so the run is half-applied and the
# error names a file rather than the reason.
#
# Identical in intent and wording to scripts\demo-reset.ps1's preflight, and
# deliberately a SECOND COPY rather than a shared module: these two scripts
# already keep duplicate copies of the key-quality helpers for the same reason
# (no module boundary exists between them), and a preflight that only one of
# the two destructive scripts runs is the one a tired operator meets.
#
# Two independent checks, because they fail in different situations: a POS
# process existing at all, and the files actually being locked by anything
# else.
function Get-HollerPosProcess {
    $byName = @(Get-Process -Name "holler-pos", "holler_pos" -ErrorAction SilentlyContinue)
    # The Tauri BUILD OUTPUT directory specifically. Matching all of apps\pos
    # sweeps in esbuild and Vite from node_modules, which cannot hold the edge
    # database and whose pids are the wrong thing to name (demo-reset.ps1 hit
    # exactly that).
    $posBuildDir = (Join-Path $repoRoot "apps\pos\src-tauri\target")
    $byPath = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $path = $null
        try { $path = $_.Path } catch { $path = $null }   # Access denied on system processes
        $path -and $path.StartsWith($posBuildDir, [System.StringComparison]::OrdinalIgnoreCase)
    })
    return @(@($byName) + @($byPath) | Sort-Object -Property Id -Unique |
             Sort-Object -Property @{ Expression = { $_.ProcessName -notlike "holler*" } })
}

# Opening for WRITE with NO sharing is exactly what the seeder needs and will
# fail on, so this cannot report "free" for a file that then refuses to open.
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

$edgeSealed = Join-Path $EdgeDataDir "edge.db.enc"
$edgePlaintext = Join-Path $EdgeDataDir "edge.db"

$posProcesses = Get-HollerPosProcess
if ($posProcesses.Count -gt 0) {
    $named = ($posProcesses | ForEach-Object { "$($_.ProcessName) pid $($_.Id)" }) -join ", "
    Write-Host ""
    Write-Host "FAILED: a Holler POS process is running ($named). NOTHING HAS BEEN CHANGED." -ForegroundColor Red
    Write-Host "NEXT ACTION: close the POS window (or Stop-Process -Id $($posProcesses[0].Id)) and re-run." -ForegroundColor Yellow
    Write-Host "  Step [3/4] seeds the edge database, which a running POS holds open -- it fails" -ForegroundColor Yellow
    Write-Host "  with 'os error 32' AFTER steps 1 and 2 have already reseeded the cloud, leaving" -ForegroundColor Yellow
    Write-Host "  a half-applied run. Refusing here costs you nothing; refusing there costs a reseed." -ForegroundColor Yellow
    exit 1
}

$lockedFiles = @(
    $edgePlaintext, "$edgePlaintext-wal", "$edgePlaintext-shm", $edgeSealed
) | Where-Object { Test-FileIsLocked $_ }
if ($lockedFiles.Count -gt 0) {
    # Names only, never a path match on "holler": the repo path contains it, so
    # a path match names every process running from this checkout and is a
    # guess dressed as a finding (demo-reset.ps1 learned this the same way).
    $candidates = @(Get-Process -Name "holler-pos", "holler_pos", "sqlite3", "devseed" -ErrorAction SilentlyContinue |
        ForEach-Object { "$($_.ProcessName) pid $($_.Id)" })
    $who = if ($candidates.Count -gt 0) { "Processes that could plausibly hold it: $($candidates -join ', ')." }
           else { "No process this script can name accounts for it." }
    Write-Host ""
    Write-Host "FAILED: another process is holding $($lockedFiles -join ', '). NOTHING HAS BEEN CHANGED." -ForegroundColor Red
    Write-Host "NEXT ACTION: close whatever has the edge database open and re-run. $who" -ForegroundColor Yellow
    Write-Host "  If you cannot find it, 'handle64.exe $edgePlaintext' (Sysinternals) names the owner." -ForegroundColor Yellow
    exit 1
}
Write-Host "preflight: no Holler POS process, and nothing holds the edge database" -ForegroundColor DarkGray

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
$savedEdgeEnv = Save-CallerEnv -Names @(
    "HOLLER_DB_KEY_HEX", "HOLLER_EDGE_DATA_DIR",
    "HOLLER_SEED_PASSWORD_HASH", "HOLLER_SEED_PASSWORD", "HOLLER_SEED_BILLING")
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
    Restore-CallerEnv -Saved $savedEdgeEnv
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

# --- the UPI payee, resolved BEFORE anything that writes it ------------------
# ORDER IS THE DEFECT THIS FIXES. This block used to sit AFTER the two env
# files were written, so `$resolvedUpiVpa` was still $null when they were
# built. Both files got `VITE_HOLLER_DEMO_UPI_VPA=` with nothing after the
# equals sign, the bill screen correctly reported "UPI QR not configured", and
# the state file held the right value the whole time -- which is what made it
# look like a write problem rather than an ordering one.
# The UPI payee, resolved the same way the LAN host is: an explicit
# parameter wins, otherwise the value this script remembered last time. Only a
# NEW value is written back, so a re-run with no -UpiVpa keeps what the demo
# was set up with instead of wiping it.
$upiStateKey = "$CloudBaseUrl|upi"
$upiState = Get-BootstrapStateMap
$resolvedUpiVpa = $UpiVpa
$resolvedUpiPayee = $UpiPayeeName
if ([string]::IsNullOrWhiteSpace($resolvedUpiVpa) -and $upiState.ContainsKey("$upiStateKey|vpa")) {
    $resolvedUpiVpa = $upiState["$upiStateKey|vpa"]
    Write-Host "       UPI payee $resolvedUpiVpa (remembered from a previous run; pass -UpiVpa to change it)" -ForegroundColor DarkGray
}
if ([string]::IsNullOrWhiteSpace($resolvedUpiPayee) -and $upiState.ContainsKey("$upiStateKey|payee")) {
    $resolvedUpiPayee = $upiState["$upiStateKey|payee"]
}
if (-not [string]::IsNullOrWhiteSpace($UpiVpa)) { Set-BootstrapStateEntry -Key "$upiStateKey|vpa" -Value $UpiVpa }
if (-not [string]::IsNullOrWhiteSpace($UpiPayeeName)) { Set-BootstrapStateEntry -Key "$upiStateKey|payee" -Value $UpiPayeeName }

# --- 4. env files for the launchers ------------------------------------------
# apps/pos/run-dev.ps1 reads this instead of hardcoding device identity and the
# encryption key. Gitignored: it carries the edge database key.
$envFile = if ([string]::IsNullOrWhiteSpace($PosEnvFile)) {
    Join-Path $repoRoot "apps\pos\.env.dev"
} else {
    $PosEnvFile
}
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
# BOTH SPELLINGS, FROM ONE VALUE. Vite only exposes a variable to the client
# bundle when it is prefixed `VITE_`, so the invoice SCREEN reads
# VITE_HOLLER_DEMO_UPI_VPA while the native receipt renderer reads the
# unprefixed name -- two variables that must agree and nothing that detects a
# mismatch (docs/demo-status.md). Writing them from one parameter here is what
# makes a mismatch unrepresentable rather than merely unlikely.
if (-not [string]::IsNullOrWhiteSpace($resolvedUpiVpa)) {
    $envLines += "HOLLER_DEMO_UPI_VPA=$resolvedUpiVpa"
    $envLines += "VITE_HOLLER_DEMO_UPI_VPA=$resolvedUpiVpa"
    if (-not [string]::IsNullOrWhiteSpace($resolvedUpiPayee)) {
        $envLines += "HOLLER_DEMO_UPI_PAYEE_NAME=$resolvedUpiPayee"
        $envLines += "VITE_HOLLER_DEMO_UPI_PAYEE_NAME=$resolvedUpiPayee"
    }
}

# ASCII so Windows PowerShell 5.1 reads it back without a BOM surprise.
$envLines | Out-File -FilePath $envFile -Encoding ascii

# --- apps\pos\.env.local: the file VITE ITSELF READS ------------------------
# WHY A SECOND FILE. `.env.dev` is this repository's own convention, read by
# apps\pos\run-dev.ps1, which exports every line into the process it launches.
# That reaches Vite only when Vite is started BY that script, in that
# environment -- and `tauri dev` does not start Vite at all if one is already
# serving 5173 (run-dev.ps1 says so in its own output). A Vite left running
# from an earlier shell then serves a bundle with no VITE_ variables in it.
#
# Observed exactly that on 2026-09-12: the invoice screen showed no UPI QR
# while .env.dev carried both lines, and the running dev server's own
# transformed module reported
#   import.meta.env = {"BASE_URL":"/","DEV":true,"MODE":"development",...}
# -- no VITE_ keys at all. Note MODE is "development", not "dev", so `.env.dev`
# is not loaded as a Vite mode file either. Two independent reasons it was
# absent; one file that is immune to both.
#
# `.env.local` is Vite's own convention, loaded in every mode, before any
# process env matters. Gitignored, and it carries NO SECRET: a VPA is a payee
# address printed on a QR for customers to scan. The encryption key stays in
# .env.dev and is never written here.
if (-not [string]::IsNullOrWhiteSpace($resolvedUpiVpa)) {
    # Beside whatever env file this run is writing, so a scratch run writes
    # its .env.local beside its scratch .env.dev and never near the real one.
    $viteEnvFile = Join-Path (Split-Path -Parent $envFile) ".env.local"
    $viteLines = @(
        "# Generated by scripts/dev-bootstrap.ps1. DO NOT COMMIT.",
        "# Read by VITE ITSELF (every mode), so the UPI QR survives a dev server",
        "# started from a shell without these variables exported.",
        "VITE_HOLLER_DEMO_UPI_VPA=$resolvedUpiVpa"
    )
    if (-not [string]::IsNullOrWhiteSpace($resolvedUpiPayee)) {
        $viteLines += "VITE_HOLLER_DEMO_UPI_PAYEE_NAME=$resolvedUpiPayee"
    }
    $viteLines | Out-File -FilePath $viteEnvFile -Encoding ascii
    Write-Host "wrote $viteEnvFile (the file Vite reads for the UPI QR)" -ForegroundColor Cyan
    Write-Host "  RESTART THE VITE DEV SERVER, not just the POS window: a Vite already" -ForegroundColor Yellow
    Write-Host "  serving 5173 keeps serving the old bundle, and tauri dev will not replace it." -ForegroundColor Yellow
}
Write-Host "`n[4/4] wrote $envFile" -ForegroundColor Cyan
if (-not [string]::IsNullOrWhiteSpace($resolvedUpiVpa)) {
    Write-Host "UPI QR ENABLED: $resolvedUpiVpa$(if (-not [string]::IsNullOrWhiteSpace($resolvedUpiPayee)) { " ($resolvedUpiPayee)" })" -ForegroundColor Cyan
} else {
    Write-Host "UPI QR DISABLED: no -UpiVpa given and none remembered." -ForegroundColor Yellow
    Write-Host "  The invoice screen and the printed receipt will show NO QR AT ALL -- there is" -ForegroundColor Yellow
    Write-Host "  no empty-QR state, and nothing on screen says why. Demo step 2 shows the QR," -ForegroundColor Yellow
    Write-Host "  so pass -UpiVpa <vpa> -UpiPayeeName <name> and re-run before the demo." -ForegroundColor Yellow
}
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
$lanIp = Get-LanIPv4 -Explicit $LanHost
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
