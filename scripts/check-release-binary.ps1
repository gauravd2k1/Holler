# DOES THE RELEASE BINARY ACTUALLY CARRY THE UI? Refuses if it does not.
#
#   .\scripts\check-release-binary.ps1
#
# WHY THIS EXISTS -- 2026-09-15, found at 00:50 on the night before the
# rehearsal, with the demo on Wednesday.
#
# `cargo build --release` does NOT produce a production Tauri app. It produces
# a DEV-MODE app compiled in the release profile: the window fetches its UI
# from `build.devUrl` (http://localhost:5173) instead of serving the frontend
# baked into the executable. Only the Tauri CLI -- `pnpm exec tauri build` --
# sets the environment that makes `tauri-build` embed `frontendDist`.
#
# So the release binary opened a window, printed `build : RELEASE`, and showed
# "Hmmm... can't reach this page -- localhost refused to connect", because
# nothing was listening on 5173. It had been that way since the first release
# build and NOBODY NOTICED, across three sessions.
#
# HOW IT WAS MISSED, WHICH IS THE POINT OF THIS FILE. Every check on that
# binary was a check on the FILE:
#
#     does it exist at the path the firewall rule names   -- passed
#     is it newer than apps\pos\dist                      -- passed
#     what is its SHA-256                                 -- recorded twice
#     does Get-Process show target\release                -- passed
#     did the launcher print `build : RELEASE`            -- printed
#
# EVERY ONE OF THOSE PASSES ON A BINARY THAT CANNOT DRAW A WINDOW. They are
# existence and identity checks standing in for a function check, and the
# substitution went unnoticed because each one individually looks like
# diligence. This is the same shape as the incidents already in CLAUDE.md:
# "the action reports success while doing nothing, and it reads correctly in
# review."
#
# CLAUDE.md already required naming WHICH RUNTIME a frontend change was
# observed in, and listed three: the build output, the dev server, the
# browser. The Tauri RELEASE WINDOW is a fourth runtime and was not on that
# list, so the discipline was followed and still missed this -- the
# screenshots taken that day were honest browser-runtime evidence that
# explicitly disclaimed proving anything about the Tauri window.
#
# WHAT THIS CHECKS, AND WHY IT IS THIS AND NOT THE OBVIOUS THING.
#
# The obvious check -- "does the binary contain http://localhost:5173" -- is
# WRONG, and would fail open. A correctly built production binary still embeds
# the whole tauri.conf.json, devUrl included, so that string is present either
# way. A check keyed on it would pass the broken binary and prove nothing.
#
# The check is therefore POSITIVE evidence: the binary must contain the
# CURRENT dist's hashed asset filename (index-<hash>.js). That name changes
# with every frontend build, so its presence proves both that assets were
# embedded AND that they are THIS frontend rather than an older embed.
#
# Run it after every release build, and `run-dev.ps1 -Release` runs it before
# launching. A refusal here is cheap. The failure it prevents is a blank
# window in front of a client.

[CmdletBinding()]
param(
    [string]$RepoRoot = "C:\Code\Holler",
    [switch]$Quiet
)

$ErrorActionPreference = "Stop"

$exe  = Join-Path $RepoRoot "apps\pos\src-tauri\target\release\holler-pos.exe"
$dist = Join-Path $RepoRoot "apps\pos\dist"

function Fail($what, $why, $fix) {
    Write-Host ""
    Write-Host "  RELEASE BINARY REFUSED: $what" -ForegroundColor Red
    Write-Host ""
    Write-Host "  $why" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  FIX:" -ForegroundColor White
    foreach ($line in $fix) { Write-Host "    $line" -ForegroundColor Cyan }
    Write-Host ""
    exit 1
}

if (-not (Test-Path $exe)) {
    Fail "it does not exist" `
         "No release binary at $exe." `
         @("cd apps\pos", "pnpm exec tauri build --no-bundle")
}
if (-not (Test-Path $dist)) {
    Fail "there is no frontend to compare against" `
         "apps\pos\dist does not exist, so the frontend has never been built." `
         @("cd apps\pos", "pnpm exec tauri build --no-bundle")
}

# The hashed entry chunk. Vite renames it on every build whose input changed,
# which is exactly the property this check needs.
$entry = Get-ChildItem (Join-Path $dist "assets") -Filter "index-*.js" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $entry) {
    Fail "the frontend build looks wrong" `
         "No assets\index-*.js in apps\pos\dist -- nothing to look for inside the binary." `
         @("cd apps\pos", "pnpm build")
}

if (-not $Quiet) {
    Write-Host ""
    Write-Host "  binary : $exe" -ForegroundColor Gray
    Write-Host "  built  : $((Get-Item $exe).LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss'))" -ForegroundColor Gray
    Write-Host "  looking for embedded asset: $($entry.Name)" -ForegroundColor Gray
}

# Latin-1 maps every byte to exactly one char, so an ASCII needle is found
# reliably inside a binary haystack with no encoding guesswork.
#
# GetEncoding(28591), not ::Latin1 -- the named static property only exists on
# .NET 5+, and this box runs Windows PowerShell 5.1 on .NET Framework, where it
# silently evaluates to $null and the next line dies with "You cannot call a
# method on a null-valued expression". Found by running this script.
$bytes = [System.IO.File]::ReadAllBytes($exe)
$hay = [System.Text.Encoding]::GetEncoding(28591).GetString($bytes)
$embedded = $hay.Contains($entry.Name)

if (-not $embedded) {
    Fail "the UI is NOT inside it -- this is a DEV-MODE build" `
         @"
The binary does not contain $($entry.Name), so the frontend was never
embedded. A window opened from it will try to load http://localhost:5173 and
show "can't reach this page" unless a Vite dev server happens to be running.

This is what `cargo build --release` produces. It is not a production build,
however correct its path, timestamp and SHA-256 look.
"@ `
         @("cd apps\pos",
           "pnpm exec tauri build --no-bundle",
           "",
           "(close the running POS first -- it holds the .exe open)")
}

# Freshness, kept as a SECOND check rather than the only one: an embed of an
# older frontend still passes the containment test above if dist has not been
# rebuilt since. Comparing mtimes catches a binary built before the frontend
# it is supposed to carry.
$exeTime = (Get-Item $exe).LastWriteTime
if ($exeTime -lt $entry.LastWriteTime) {
    Fail "it carries an OLDER frontend than apps\pos\dist" `
         "The binary was linked at $($exeTime.ToString('HH:mm:ss')) but the frontend was built at $($entry.LastWriteTime.ToString('HH:mm:ss')). The window will show the previous UI, silently." `
         @("cd apps\pos", "pnpm exec tauri build --no-bundle")
}

if (-not $Quiet) {
    Write-Host ""
    Write-Host "  OK -- the release binary carries this frontend." -ForegroundColor Green
    Write-Host "  It will render without a dev server." -ForegroundColor Green
    Write-Host ""
}
exit 0
