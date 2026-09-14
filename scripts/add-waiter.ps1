# Enrol ONE MORE waiter phone, mid-demo, without touching the running stack.
#
#   .\scripts\add-waiter.ps1 -Name "Panel phone 2"
#
# WHY THIS EXISTS. `demo-up.ps1` enrols exactly one WAITER device and prints
# one token. That is enough for one phone. It is not enough to hand a device
# to three people in a room and have the orders come back distinguishable --
# and "distinguishable" is the whole point, because an order's author is
# recorded from the credential that created it (`captain.rs` attributes to the
# RESOLVED credential's own device_id, never to the till and never to anything
# the request body claims).
#
# Sharing one token across three phones DOES work and is a legitimate fallback
# -- but all three then are one device, and every order reads as having come
# from the same waiter. Enrolling a device each is what makes "Rahul's phone"
# and "Priya's phone" separate names on the same screen.
#
# WHAT IT DOES NOT DO: start, stop, restart or reconfigure anything. It calls
# the cloud API and prints a token. The till picks the new credential up on
# its own schedule -- see THE SIXTY SECONDS below, which is the one thing that
# will make this look broken if you do not know about it.
#
# THE SIXTY SECONDS. The captain listener verifies a token against the edge's
# LOCAL device_credential_cache, and that cache is only written by the config
# pull, which the POS runs at startup and then every 60s. So a phone paired
# IMMEDIATELY after enrolment sees "That device token was rejected" -- the
# same message a mistyped token gives -- until the next pull lands. Enrol the
# extra phones BEFORE the panel arrives, or enrol, wait for the pull, then
# hand the phone over. Do not debug it in the first thirty seconds; it is not
# broken, it is early.

[CmdletBinding()]
param(
    # The device's name, and the thing that makes it distinguishable later.
    # Use a person's name if you are handing it to a person -- "Panel phone 2"
    # is fine, "WAITER-2" tells you nothing on a screen three minutes later.
    [Parameter(Mandatory = $true)]
    [string]$Name,

    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,

    [string]$CloudBaseUrl = "http://localhost:8080",

    # The operator account demo-up already uses to enrol. Needs outlet.manage.
    [string]$Email = "owner@holler.test",
    [string]$Password = "holler123",

    # Print the pairing URL with this host. Defaults to the machine's
    # default-route IPv4, which is what the phones must reach.
    [string]$LanHost = "",
    [int]$CaptainPort = 9320
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "agent-guard.ps1")

function Say($m, $c = "Gray") { Write-Host $m -ForegroundColor $c }

if ($LanHost -eq "") {
    $LanHost = (Get-NetIPConfiguration |
        Where-Object { $_.IPv4DefaultGateway -ne $null } |
        Select-Object -First 1).IPv4Address.IPAddress
}
if (-not $LanHost) { throw "could not work out this machine's LAN address -- pass -LanHost" }

# WHICH TENANT AND OUTLET -- read from apps\pos\.env.dev, exactly as
# demo-up.ps1 does (its step 7). This is NOT optional decoration: /auth/login
# requires an X-Tenant-ID header AND outlet_id in the body, and without them it
# returns {"code":"unauthorized","message":"authentication required"} -- which
# reads like a wrong password and is not. The first version of this script sent
# only email and password and failed exactly that way (2026-09-15).
#
# The file is read HERE, at run time, by the operator's own shell. It carries
# the edge encryption key and is deny-ruled to agents; nothing in this script
# prints it or any part of it.
$posEnvFile = Join-Path $RepoRoot "apps\pos\.env.dev"
if (-not (Test-Path $posEnvFile)) {
    throw "$posEnvFile not found -- run scripts\dev-bootstrap.ps1 first; it writes the tenant and outlet ids."
}
$envValues = @{}
foreach ($line in Get-Content $posEnvFile) {
    $trimmed = $line.Trim()
    if ($trimmed -and -not $trimmed.StartsWith("#") -and $trimmed -match '^([A-Za-z_][A-Za-z0-9_]*)=(.*)$') {
        $envValues[$Matches[1]] = $Matches[2].Trim()
    }
}
$tenantId = $envValues['HOLLER_TENANT_ID']
$outletId = $envValues['HOLLER_OUTLET_ID']
if ((-not $tenantId) -or (-not $outletId)) {
    throw "$posEnvFile carries no HOLLER_TENANT_ID / HOLLER_OUTLET_ID. Re-run scripts\dev-bootstrap.ps1 -- it writes both."
}

Say "signing in as $Email" "Cyan"
try {
    $session = Invoke-RestMethod -Uri "$CloudBaseUrl/auth/login" -Method Post `
        -Body (@{ email = $Email; password = $Password; outlet_id = $outletId } | ConvertTo-Json) `
        -ContentType "application/json" -Headers @{ 'X-Tenant-ID' = $tenantId }
} catch {
    $detail = $_.ErrorDetails.Message
    if (-not $detail) { $detail = $_.Exception.Message }
    Write-Host ""
    Write-Host "  Could not log in as $Email against $CloudBaseUrl" -ForegroundColor Red
    Write-Host "  $detail" -ForegroundColor Red
    Write-Host ""
    Write-Host "  A 401 here is one of three things, indistinguishable by design (ADR-012):" -ForegroundColor Yellow
    Write-Host "    - a wrong password" -ForegroundColor Yellow
    Write-Host "    - the login rate limiter" -ForegroundColor Yellow
    Write-Host "    - the dev database's user rows overwritten by a Go test fixture hash" -ForegroundColor Yellow
    Write-Host "  The last one is fixed by re-running demo-up.ps1 with -Fresh." -ForegroundColor Yellow
    Write-Host ""
    exit 1
}
$headers = @{ 'X-Tenant-ID' = $tenantId; 'Authorization' = "Bearer $($session.access_token)" }

Say "enrolling WAITER device '$Name'" "Cyan"
try {
    $device = Invoke-RestMethod -Uri "$CloudBaseUrl/devices/enroll" -Method Post `
        -Headers $headers -ContentType "application/json" `
        -Body (@{ outlet_id = $outletId; kind = 'WAITER'; name = $Name; label = 'add-waiter' } | ConvertTo-Json)
} catch {
    $detail = $_.ErrorDetails.Message
    if ($detail -match 'already') {
        Write-Host ""
        Write-Host "A device named '$Name' is already enrolled." -ForegroundColor Yellow
        Write-Host "Pick a different -Name. Do NOT reuse a name and assume you got a new token:" -ForegroundColor Yellow
        Write-Host "the token is issued ONCE at enrolment and is never retrievable afterwards." -ForegroundColor Yellow
        exit 1
    }
    throw
}

$token = $device.token
if (-not $token) { throw "the enrol response carried no token -- nothing to pair with" }

Write-Host ""
Write-Host "=====================================================================" -ForegroundColor Green
Write-Host " WAITER DEVICE ENROLLED: $Name" -ForegroundColor Green
Write-Host "=====================================================================" -ForegroundColor Green
Write-Host ""
Write-Host "  Captain URL (open this on the phone):" -ForegroundColor White
Write-Host "    http://${LanHost}:$CaptainPort/" -ForegroundColor Cyan
Write-Host ""
Write-Host "  Pair token (paste it once; the phone remembers it):" -ForegroundColor White
Write-Host "    $token" -ForegroundColor Cyan
Write-Host ""
Write-Host "  THE TOKEN IS SHOWN ONCE AND IS NOT STORED ANYWHERE." -ForegroundColor Yellow
Write-Host "  Lose it and the fix is enrolling another device, not recovering this one." -ForegroundColor Yellow
Write-Host ""
Write-Host "  The till caches new credentials on its config pull, which runs every" -ForegroundColor Yellow
Write-Host "  60 seconds. If the phone says the token was rejected, wait a minute" -ForegroundColor Yellow
Write-Host "  and pair again before assuming anything is wrong." -ForegroundColor Yellow
Write-Host ""
