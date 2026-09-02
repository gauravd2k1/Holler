# Verifies that the cloud is GENUINELY UNREACHABLE from this machine, for
# acceptance steps that name an offline precondition.
#
# ============================================================================
# WHY THIS EXISTS
# ============================================================================
#
# Every acceptance procedure in this project said "with the network
# disconnected" from Milestone 1 onward, and the operator switched WiFi off.
# The cloud is http://localhost:8080. That traffic never leaves the machine, so
# the step passed identically with WiFi on, off, or every adapter disabled.
#
# The offline precondition was never established, in any run, in any milestone.
# A condition the environment cannot produce yields a step that is green
# regardless of the code, which is indistinguishable from a step that is green
# because the code is right (docs/retro.md, 2026-09-02).
#
# ============================================================================
# WHY IT DOES NOT SIMPLY CATCH AN EXCEPTION
# ============================================================================
#
# The first version of this check was `try { Invoke-RestMethod .../health }
# catch { "offline confirmed" }`. `Invoke-RestMethod` throws a WebException for
# a REFUSED CONNECTION and for an HTTP 404 alike, so a running server that
# answers 404 on the probed path prints "offline confirmed". It passed
# precisely when the precondition was false -- the defect it exists to prevent,
# inside the guard.
#
# So this inspects the FAILURE MODE rather than the fact that something threw:
#   - WebExceptionStatus ProtocolError means the server ANSWERED (any status).
#     That is REACHABLE, however unhappy the response.
#   - ConnectFailure / Timeout / NameResolutionFailure means nothing answered.
#
# ============================================================================
# FAIL-CLOSED
# ============================================================================
#
# Exit 0 means OFFLINE CONFIRMED and is printed only when every probe agrees.
# Anything unexpected -- an exception type not enumerated here, a listener still
# bound, a probe that unexpectedly succeeds -- reports REACHABLE and exits 1.
# A verifier that guesses in the ambiguous case is worth nothing.

[CmdletBinding()]
param(
    [string]$BaseUrl = "http://localhost:8080",
    [int]$Port = 8080,
    [string]$HealthPath = "/health"
)

$ErrorActionPreference = "Continue"
$reachable = @()

Write-Host "checking that $BaseUrl is unreachable..." -ForegroundColor Cyan

# --- 1. is anything BOUND to the port -----------------------------------------
# The most direct evidence, and it does not depend on any HTTP behaviour.
$listeners = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
if ($listeners) {
    $listenerPids = ($listeners | Select-Object -ExpandProperty OwningProcess -Unique) -join ","
    $reachable += "a process is LISTENING on port $Port (pid $listenerPids)"
} else {
    Write-Host "  [ok] nothing is listening on port $Port"
}

# --- 2. can a TCP connection be established -----------------------------------
# Independent of HTTP entirely: if the handshake completes, something is there.
$client = New-Object System.Net.Sockets.TcpClient
try {
    $async = $client.BeginConnect("127.0.0.1", $Port, $null, $null)
    if ($async.AsyncWaitHandle.WaitOne(3000, $false) -and $client.Connected) {
        $reachable += "a TCP connection to 127.0.0.1:$Port SUCCEEDED"
    } else {
        Write-Host "  [ok] TCP connection to 127.0.0.1:$Port refused or timed out"
    }
} catch {
    Write-Host "  [ok] TCP connection to 127.0.0.1:$Port failed: $($_.Exception.Message)"
} finally {
    $client.Close()
}

# --- 3. the HTTP probe, classified by FAILURE MODE ----------------------------
$url = "$BaseUrl$HealthPath"
try {
    $response = Invoke-WebRequest -Uri $url -TimeoutSec 4 -UseBasicParsing
    $reachable += "$url ANSWERED with HTTP $($response.StatusCode)"
} catch [System.Net.WebException] {
    $status = $_.Exception.Status
    switch ($status) {
        # The server answered. A 404 lands here, and treating it as "offline" is
        # exactly the bug this file documents.
        "ProtocolError" {
            $code = "unknown"
            if ($_.Exception.Response) {
                try { $code = [int]$_.Exception.Response.StatusCode } catch { $code = "$($_.Exception.Response.StatusCode)" }
            }
            $reachable += "$url ANSWERED with HTTP $code (the server is up; only the path is wrong)"
        }
        "ConnectFailure"        { Write-Host "  [ok] $url refused the connection" }
        "Timeout"               { Write-Host "  [ok] $url timed out with no answer" }
        "NameResolutionFailure" { Write-Host "  [ok] $url could not be resolved" }
        default {
            # Fail closed: an unenumerated mode is not evidence of anything.
            $reachable += "$url failed in an UNRECOGNISED way (WebExceptionStatus=$status): treating as reachable, because an unclassified failure proves nothing"
        }
    }
} catch {
    $reachable += "$url threw $($_.Exception.GetType().Name), which this check does not classify: treating as reachable"
}

Write-Host ""
if ($reachable.Count -gt 0) {
    Write-Host "STOP - cloud is REACHABLE. The offline precondition is NOT established." -ForegroundColor Red
    foreach ($r in $reachable) { Write-Host "  - $r" -ForegroundColor Red }
    Write-Host ""
    Write-Host "Nothing observed after this point counts as offline evidence." -ForegroundColor Red
    exit 1
}

Write-Host "offline confirmed - nothing is listening, TCP is refused, and HTTP does not answer." -ForegroundColor Green
Write-Host "The offline precondition is established. Proceed." -ForegroundColor Green
exit 0
