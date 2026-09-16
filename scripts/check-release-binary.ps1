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
# CURRENT dist's hashed asset filenames. Vite derives each one from a hash of
# that asset's own CONTENT, so the name changes whenever the file's bytes
# change and its presence inside the executable proves both that assets were
# embedded AND that they are THESE assets rather than an older embed.
#
# EXACTLY WHAT IS COMPARED, so nobody has to infer it:
#
#   HAYSTACK  every byte of apps\pos\src-tauri\target\release\holler-pos.exe,
#             read as Latin-1 so one byte is one char and an ASCII needle is
#             found without encoding guesswork
#   NEEDLES   every hashed asset filename in apps\pos\dist\assets -- the
#             index-<hash>.js entry chunk and every index-<hash>.css beside it
#   PASS      every needle is present in the haystack
#
#   NOT the modification time of anything. NOT a version string. NOT the
#   binary's SHA-256, which says only that it is the same file as last time,
#   not what is inside it. Mtimes appear ONCE more below, as a secondary
#   ordering check, and that check is explicitly not load-bearing -- see the
#   note on it.
#
# THE SECOND REFUSAL THAT WAS ASKED FOR AND IS NOT HERE, with the evidence.
# "Refuse if the binary contains localhost:5173" was proposed on 2026-09-16 as
# a way to catch the cargo trap directly. It was MEASURED against a
# known-good production binary before being written:
#
#     binary built 19:08 by `pnpm exec tauri build --no-bundle`
#     contains the current dist entry chunk .......... True   (it is good)
#     contains "localhost:5173" ...................... True
#
# A correct production binary carries the whole tauri.conf.json, devUrl
# included. That refusal would therefore reject EVERY good build, and the
# first thing anyone would do is bypass the check -- which is worse than not
# having it. Absence of a dev string is not evidence of a production build;
# presence of THIS frontend is. Recorded here so it is not proposed a third
# time.
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

# THE NEEDLES: every hashed asset name in dist\assets, not just the entry
# chunk. Each name is Vite's hash of that file's own content, so this compares
# CONTENT and nothing else. Widened from the single entry chunk on 2026-09-16:
# one needle is one chance for a partial embed to pass, and the JS and CSS are
# embedded by the same mechanism, so requiring both costs nothing and narrows
# the region this check cannot see.
$assets = @(Get-ChildItem (Join-Path $dist "assets") -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^index-[A-Za-z0-9_-]+\.(js|css)$' })
$entry = $assets | Where-Object { $_.Name -like '*.js' } |
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
    Write-Host "  comparing CONTENT, not timestamps:" -ForegroundColor Gray
    foreach ($a in $assets) {
        Write-Host "    needle: $($a.Name)" -ForegroundColor Gray
    }
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
$missing = @($assets | Where-Object { -not $hay.Contains($_.Name) })
$embedded = ($missing.Count -eq 0)

if (-not $embedded) {
    Fail "the UI is NOT inside it -- this is a DEV-MODE build" `
         @"
The binary does not contain $(($missing | ForEach-Object { $_.Name }) -join ', '), so this
frontend was never embedded. A window opened from it will try to load http://localhost:5173 and
show "can't reach this page" unless a Vite dev server happens to be running.

This is what `cargo build --release` produces. It is not a production build,
however correct its path, timestamp and SHA-256 look.
"@ `
         @("cd apps\pos",
           "pnpm exec tauri build --no-bundle",
           "",
           "(close the running POS first -- it holds the .exe open)")
}

# Freshness by mtime, kept as a SECOND check and DELIBERATELY NOT THE ONLY ONE.
#
# On 2026-09-16 a POS started at 11:39 was running a 00:40 build, and the
# newer-than-dist rule passed it happily -- because BOTH were stale, and a
# comparison between two stale things is satisfied. That is the whole reason
# the content comparison above exists and runs first: a freshness check must
# compare CONTENT, not timestamps. This one only catches the narrower case of a
# binary linked before a dist that has since been rebuilt.
$exeTime = (Get-Item $exe).LastWriteTime
if ($exeTime -lt $entry.LastWriteTime) {
    Fail "it carries an OLDER frontend than apps\pos\dist" `
         "The binary was linked at $($exeTime.ToString('HH:mm:ss')) but the frontend was built at $($entry.LastWriteTime.ToString('HH:mm:ss')). The window will show the previous UI, silently." `
         @("cd apps\pos", "pnpm exec tauri build --no-bundle")
}

if (-not $Quiet) {
    Write-Host ""
    Write-Host "  OK -- the release binary carries this frontend ($($assets.Count) asset name(s) found inside it)." -ForegroundColor Green
    Write-Host "  It will render without a dev server." -ForegroundColor Green
    Write-Host ""
}
exit 0
