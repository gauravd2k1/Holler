# WHICH ADDRESS DO THE PHONES TYPE? Answers it, and says why.
#
#   .\scripts\lan-ip.ps1
#
# WHY THIS EXISTS. Passing the wrong address is the most expensive silent
# failure in this system, and it has now fired twice. `-LanHost` is accepted
# without being checked against the machine's own addresses, so the wrong value
# is written into apps\kds\.env.dev, every downstream check passes, and the KDS
# loads, looks completely normal and never connects -- for ever, with no error
# on either side, because lanClient.ts retries a dead address indefinitely.
# It cost an evening on 2026-09-13 and a session on 2026-09-14
# (docs\backlog.md, the -LanHost row).
#
# The trap is not that the address is hard to find. It is that this machine has
# SEVERAL and the wrong ones look plausible:
#
#   172.28.176.1   the WSL virtual switch. Looks like a LAN address. NO phone
#                  on earth can route to it. This is the one that gets picked
#                  by "just take the first non-loopback address".
#   192.168.0.100  the home WiFi lease. Correct at home, wrong at the venue,
#                  and it MOVES -- a DHCP renewal is all it takes.
#   192.168.137.1  the Mobile Hotspot. The one the phones want on demo day.
#
# So this prints all of them, ranked, and says what each is for.

[CmdletBinding()]
param(
    # Also print the URLs to type into the phones and the second laptop.
    [switch]$Urls
)

$ErrorActionPreference = "Stop"

$addrs = Get-NetIPAddress -AddressFamily IPv4 |
    Where-Object { $_.IPAddress -ne '127.0.0.1' -and $_.IPAddress -notlike '169.254.*' }

# The interface that carries the default route is the one with real upstream
# connectivity. It is the right answer for a normal LAN and the WRONG answer on
# demo day, because a hotspot carries no default route -- traffic flows the
# other way. Both facts are printed rather than one being guessed at.
$defaultIf = (Get-NetIPConfiguration | Where-Object { $_.IPv4DefaultGateway -ne $null } |
    Select-Object -First 1).InterfaceAlias

function Classify($a) {
    $alias = $a.InterfaceAlias
    $ip = $a.IPAddress
    if ($alias -match 'WSL|Hyper-V|vEthernet|Default Switch') {
        return @{ Rank = 99; Verdict = "VIRTUAL -- NO phone can reach this. Never pass it."; Colour = "DarkGray" }
    }
    if ($ip -like '192.168.137.*' -or $alias -like 'Local Area Connection*') {
        return @{ Rank = 1; Verdict = "MOBILE HOTSPOT -- this is the demo-day address."; Colour = "Green" }
    }
    if ($alias -eq $defaultIf) {
        return @{ Rank = 2; Verdict = "This machine's normal network (has the default route). Fine at a desk; it MOVES."; Colour = "Yellow" }
    }
    return @{ Rank = 3; Verdict = "Another real adapter. Usable only if the phones are on THIS network."; Colour = "Gray" }
}

$rows = foreach ($a in $addrs) {
    $c = Classify $a
    [pscustomobject]@{
        Rank    = $c.Rank
        IP      = $a.IPAddress
        Adapter = $a.InterfaceAlias
        Verdict = $c.Verdict
        Colour  = $c.Colour
    }
}
$rows = @($rows | Sort-Object Rank, IP)

Write-Host ""
Write-Host "This machine's IPv4 addresses, best first:" -ForegroundColor White
Write-Host ""
foreach ($r in $rows) {
    Write-Host ("  {0,-15} {1,-34} {2}" -f $r.IP, $r.Adapter, $r.Verdict) -ForegroundColor $r.Colour
}

$best = $rows | Where-Object { $_.Rank -le 2 } | Select-Object -First 1
Write-Host ""
if (-not $best) {
    Write-Host "  NO USABLE ADDRESS FOUND." -ForegroundColor Red
    Write-Host "  Every address here is virtual. Turn the hotspot on, or join a network." -ForegroundColor Red
    Write-Host ""
    exit 1
}

if ($best.Rank -eq 1) {
    Write-Host "  USE: $($best.IP)   (hotspot -- what you want on demo day)" -ForegroundColor Green
} else {
    Write-Host "  USE: $($best.IP)   (no hotspot is up -- this is your normal network)" -ForegroundColor Yellow
    Write-Host "  On demo day, turn Mobile Hotspot ON and run this again." -ForegroundColor Yellow
}
Write-Host ""

if ($Urls) {
    $ip = $best.IP
    Write-Host "  Give these out:" -ForegroundColor White
    Write-Host "    Waiter phones      http://${ip}:9320/" -ForegroundColor Cyan
    Write-Host "    Kitchen screen     http://${ip}:5174/" -ForegroundColor Cyan
    Write-Host "    Back office        http://${ip}:5175/" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  And start the stack with:" -ForegroundColor White
    Write-Host "    .\scripts\demo-up.ps1 -DbKeyHex <key> -LanHost $ip -Fresh -Release" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  The phones must be on the SAME network as this adapter." -ForegroundColor Yellow
    Write-Host "  On demo day that means YOUR hotspot, never the venue's WiFi." -ForegroundColor Yellow
    Write-Host ""
}
