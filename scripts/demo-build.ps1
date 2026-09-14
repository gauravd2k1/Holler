# BUILD EVERYTHING THE DEMO RUNS, CORRECTLY, IN ONE COMMAND.
#
#   .\scripts\demo-build.ps1
#
# Then:
#
#   .\scripts\demo-up.ps1 -DbKeyHex <key> -LanHost <ip> -Fresh -Release
#
# WHY THIS EXISTS. `demo-up.ps1` STARTS the stack; it has never BUILT the
# release binary, it assumes one is already there. So the build was four
# separate commands held in someone's head, in an order that matters, one of
# which was wrong for three sessions:
#
#   `cargo build --release` DOES NOT PRODUCE A PRODUCTION TAURI APP. It
#   produces a dev-mode app in the release profile, whose window loads
#   http://localhost:5173 instead of the UI compiled into it and shows
#   "can't reach this page" when no dev server is running. Only the Tauri CLI
#   sets the environment that embeds `frontendDist`. Found 2026-09-15, the
#   night before the rehearsal, after it had passed every existing check --
#   see scripts\check-release-binary.ps1 for how something that broken stayed
#   invisible for so long.
#
# So this script is the ONE place that knows how to build this product, and it
# finishes by PROVING the binary carries its UI rather than assuming it.
#
# WHAT GETS BUILT, and why each one is here:
#
#   apps\captain   served from DISK by the POS at request time (captain.rs), so
#                  it needs no relink -- but it does need to exist, and a stale
#                  one is a phone showing yesterday's menu.
#   apps\kds       a real web build. `vite preview` serves it; the dev server
#                  is a different runtime from what this produces.
#   apps\admin     same.
#   apps\pos       via the TAURI CLI, which runs `pnpm build` itself and then
#                  links the result INTO the executable.
#
# The POS is built LAST, deliberately: it is the slow one and the one that can
# fail on a file lock, and finding that out after three fast builds have
# succeeded is better than the reverse.

[CmdletBinding()]
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,

    # Skip the three web builds and only re-link the POS. For when you have
    # changed Rust and nothing else.
    [switch]$PosOnly,

    # Produce the NSIS installer too. Slow, and not needed to run a demo from
    # this machine -- the demo launches the .exe directly.
    [switch]$WithInstaller
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "agent-guard.ps1")

$script:StepNo = 0
function Step($msg) {
    $script:StepNo++
    Write-Host ""
    Write-Host "[$script:StepNo] $msg" -ForegroundColor Cyan
}
function Ok($msg)   { Write-Host "    OK  $msg" -ForegroundColor Green }
function Note($msg) { Write-Host "    $msg" -ForegroundColor Gray }

function Invoke-Build($name, $dir) {
    Step "$name"
    Push-Location $dir
    try {
        if (-not (Test-Path "node_modules")) {
            Note "installing dependencies (first run here)"
            pnpm install --silent
            if ($LASTEXITCODE -ne 0) { throw "pnpm install failed in $dir" }
        }
        pnpm build
        if ($LASTEXITCODE -ne 0) { throw "$name build FAILED -- nothing further was built." }
        Ok "$name"
    } finally { Pop-Location }
}

Write-Host ""
Write-Host "=== Holler demo build ===" -ForegroundColor White
Write-Host "repo: $RepoRoot" -ForegroundColor Gray

# ---------------------------------------------------------------- preflight --
# The POS holds its own .exe open, so a build with the till running fails at
# the LINK step -- after several minutes of compiling -- with
# "failed to remove file ... Access is denied. (os error 5)". Refuse up front
# and name the pid, rather than spending the compile first.
Step "preflight -- nothing may be holding the binary"
$running = @(Get-Process holler-pos -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    Write-Host ""
    Write-Host "    REFUSED: the POS is running and holds its own executable." -ForegroundColor Red
    foreach ($p in $running) {
        Write-Host "      pid $($p.Id)  started $($p.StartTime)  $($p.Path)" -ForegroundColor Red
    }
    Write-Host ""
    Write-Host "    Close the POS window, then run this again." -ForegroundColor Yellow
    Write-Host "    (Refusing now rather than at the link step, which is several minutes in.)" -ForegroundColor Yellow
    Write-Host ""
    exit 1
}
Ok "no holler-pos process"

# ------------------------------------------------------------- the web apps --
if (-not $PosOnly) {
    Invoke-Build "captain  (served from disk by the POS -- no relink needed)" (Join-Path $RepoRoot "apps\captain")
    Invoke-Build "KDS      (kitchen screen)"                                  (Join-Path $RepoRoot "apps\kds")
    Invoke-Build "admin    (back office)"                                     (Join-Path $RepoRoot "apps\admin")
} else {
    Note "-PosOnly: skipping captain, KDS and admin"
}

# ------------------------------------------------------------------ the POS --
# THE TAURI CLI, NOT CARGO. This is the whole point of the script.
Step "POS      (Tauri CLI -- builds the frontend AND embeds it)"
Push-Location (Join-Path $RepoRoot "apps\pos")
try {
    # Written out in full rather than splatted. `pnpm exec tauri build @args`
    # with an array reaches cargo as a bare `-` under Windows PowerShell 5.1
    # ("error: unexpected argument '-' found"), because splatting an array to
    # a NATIVE command is not the same as splatting to a cmdlet. Two literal
    # invocations cannot be mangled.
    Note "this recompiles Rust and takes a few minutes"
    if ($WithInstaller) {
        Note "pnpm exec tauri build"
        pnpm exec tauri build
    } else {
        Note "pnpm exec tauri build --no-bundle"
        pnpm exec tauri build --no-bundle
    }
    if ($LASTEXITCODE -ne 0) {
        throw "the POS build FAILED. If it says 'Access is denied (os error 5)', the POS is running -- close it."
    }
    Ok "POS linked"
} finally { Pop-Location }

# ------------------------------------------------------------ PROVE it works --
# Not "the file exists" and not "it is newer than dist" -- both of those passed
# on the binary that could not draw a window. This asserts the UI is INSIDE it.
Step "verify -- is the UI actually inside the binary?"
& (Join-Path $PSScriptRoot "check-release-binary.ps1") -RepoRoot $RepoRoot
if ($LASTEXITCODE -ne 0) {
    Write-Host "    The build produced a binary that does not carry its UI. Do not demo this." -ForegroundColor Red
    exit 1
}

# ----------------------------------------------------------------- what next --
$exe = Join-Path $RepoRoot "apps\pos\src-tauri\target\release\holler-pos.exe"
$sha = (Get-FileHash -Algorithm SHA256 $exe).Hash.ToLower()

Write-Host ""
Write-Host "=====================================================================" -ForegroundColor Green
Write-Host " BUILD COMPLETE" -ForegroundColor Green
Write-Host "=====================================================================" -ForegroundColor Green
Write-Host ""
Write-Host "  binary : $exe" -ForegroundColor White
Write-Host "  sha256 : $sha" -ForegroundColor White
Write-Host ""
Write-Host "  Next -- find the address the phones must reach:" -ForegroundColor White
Write-Host "    .\scripts\lan-ip.ps1 -Urls" -ForegroundColor Cyan
Write-Host ""
Write-Host "  Then start the stack:" -ForegroundColor White
Write-Host "    .\scripts\demo-up.ps1 -DbKeyHex <key> -LanHost <ip> -Fresh -Release" -ForegroundColor Cyan
Write-Host ""
Write-Host "  The firewall rule names this exact path. If the path ever changes," -ForegroundColor Yellow
Write-Host "  the rule stops covering it and the phones fail with no error anywhere." -ForegroundColor Yellow
Write-Host ""
