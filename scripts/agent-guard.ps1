# A STRUCTURAL GUARD AGAINST AN AGENT WRITING THE OPERATOR'S REAL FILES.
#
# WHY THIS EXISTS. Three times in one day an agent wrote to the operator's live
# stack despite an explicit, repeated, written instruction to use scratch paths
# only: the backend on 8080 was displaced twice, an admin dev server was left
# holding 5175 for hours, and a test that was "pointed at a scratch file"
# overwrote apps\pos\.env.dev -- zeroing the edge database key -- because the
# code it invoked recomputed its own paths from $repoRoot and ignored the
# scratch path it was handed.
#
# The operator's ruling, and it is correct: INSTRUCTIONS ARE NOT A CONTROL. An
# instruction is followed right up until it isn't, and the failure is silent
# and destructive. A refusal is neither.
#
# WHAT IT DOES. Claude Code sets CLAUDECODE=1 in every shell it spawns. Under
# such a shell:
#
#   - Scripts that CAN operate on a scratch tree (dev-bootstrap, demo-reset)
#     must be given BOTH -RepoRoot and -DataDir EXPLICITLY, and both must point
#     OUTSIDE the real tree. Defaults are forbidden: the whole failure mode is
#     a default quietly resolving to the real path.
#
#   - Scripts whose entire purpose is to start the operator's stack on live
#     ports (dev-up, apps\pos\run-dev) refuse outright. They have no scratch
#     mode -- "start the POS on a scratch port" is not a thing the demo needs --
#     so the honest guard is a refusal with the reason.
#
# A human shell has no CLAUDECODE, so nothing about the operator's own workflow
# changes. Read-only probes are deliberately NOT guarded
# (scripts\check-cloud-unreachable.ps1 issues a GET and binds nothing); this
# guard is about writing files and taking ports, not about looking.
#
# FAIL-CLOSED BY CONSTRUCTION: every guarded script dot-sources this file at
# the top. If it is missing or unreadable the dot-source throws and the script
# does not run.

$script:AgentGuardRealRepoRoot = "C:\Code\Holler"
$script:AgentGuardRealDataDir = (Join-Path $env:APPDATA "com.holler.pos")

function Test-IsAgentShell {
    return -not [string]::IsNullOrWhiteSpace($env:CLAUDECODE)
}

# True when $Path IS $Root or sits underneath it. Compared on canonical full
# paths, because "..\Holler\apps" and "C:\Code\Holler\apps" are the same
# directory and a string compare says otherwise.
function Test-PathIsInside {
    param([string]$Path, [string]$Root)
    if ([string]::IsNullOrWhiteSpace($Path)) { return $true }   # empty = the default = inside
    try {
        $full = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
        $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd('\')
    } catch {
        # An unparseable path is not provably outside, so it is treated as
        # inside. A guard that fails open is not a guard.
        return $true
    }
    if ($full -ieq $rootFull) { return $true }
    return $full.StartsWith($rootFull + '\', [System.StringComparison]::OrdinalIgnoreCase)
}

function Deny-AgentRun {
    param([string]$ScriptName, [string]$Problem, [string]$WhatToDo)
    Write-Host ""
    Write-Host "REFUSED: $ScriptName will not run under a Claude Code shell." -ForegroundColor Red
    Write-Host "  $Problem" -ForegroundColor Red
    Write-Host "  NOTHING WAS CHANGED." -ForegroundColor Red
    Write-Host ""
    Write-Host "  $WhatToDo" -ForegroundColor Yellow
    Write-Host "  (This guard exists because an agent overwrote the operator's real" -ForegroundColor DarkGray
    Write-Host "   apps\pos\.env.dev on 2026-09-12 while believing it was writing a" -ForegroundColor DarkGray
    Write-Host "   scratch copy. See scripts\agent-guard.ps1.)" -ForegroundColor DarkGray
    exit 1
}

# For scripts that can legitimately run against a scratch tree.
function Assert-AgentSafePaths {
    param(
        [string]$ScriptName,
        [System.Collections.IDictionary]$BoundParameters,
        [string]$RepoRootValue,
        [string]$DataDirValue,
        [string]$RepoRootParameterName = "RepoRoot",
        [string]$DataDirParameterName = "DataDir"
    )
    if (-not (Test-IsAgentShell)) { return }

    if (-not $BoundParameters.ContainsKey($RepoRootParameterName)) {
        Deny-AgentRun -ScriptName $ScriptName `
            -Problem "-$RepoRootParameterName was not given, so it would default to $script:AgentGuardRealRepoRoot." `
            -WhatToDo "Pass -$RepoRootParameterName AND -$DataDirParameterName explicitly, both outside the real tree, or ask the operator to run this."
    }
    if (-not $BoundParameters.ContainsKey($DataDirParameterName)) {
        Deny-AgentRun -ScriptName $ScriptName `
            -Problem "-$DataDirParameterName was not given, so it would default to $script:AgentGuardRealDataDir." `
            -WhatToDo "Pass -$DataDirParameterName explicitly, outside %APPDATA%\com.holler.pos."
    }
    if (Test-PathIsInside -Path $RepoRootValue -Root $script:AgentGuardRealRepoRoot) {
        Deny-AgentRun -ScriptName $ScriptName `
            -Problem "-$RepoRootParameterName '$RepoRootValue' is inside $script:AgentGuardRealRepoRoot." `
            -WhatToDo "Point it at a scratch tree, e.g. `$env:TEMP\holler-scratch."
    }
    if (Test-PathIsInside -Path $DataDirValue -Root $script:AgentGuardRealDataDir) {
        Deny-AgentRun -ScriptName $ScriptName `
            -Problem "-$DataDirParameterName '$DataDirValue' is inside $script:AgentGuardRealDataDir." `
            -WhatToDo "Point it at a scratch directory, e.g. `$env:TEMP\holler-scratch-data."
    }
    Write-Host "agent-guard: agent shell detected; running against scratch paths only" -ForegroundColor DarkGray
    Write-Host "             repo: $RepoRootValue" -ForegroundColor DarkGray
    Write-Host "             data: $DataDirValue" -ForegroundColor DarkGray
}

# ---------------------------------------------------------------------------
# THE SCRATCH-DATABASE RULE
#
# A run that DROPS a schema, or that migrates and seeds one, may only ever name
# a database whose name begins with the scratch prefix below. Two separate
# incidents made this structural rather than instructional:
#
#   - demo-reset.ps1 took -DatabaseUrl and -PostgresDb as independent
#     parameters, and the destructive DROP SCHEMA read -PostgresDb while every
#     seeder read -DatabaseUrl. A scratch -DatabaseUrl on its own therefore
#     left the drop aimed at 'holler'. Seen in a -WhatIf banner announcing
#     exactly that.
#   - The Go suite run with HOLLER_TEST_DATABASE_URL pointed at the shared dev
#     database overwrote owner@holler.test and cashier@holler.test with fixture
#     hashes, so the till and the console then refused a CORRECT password. The
#     401 was misread as a credential fault and then as the rate limiter.
#
# Both are the same shape as the -BackendPort 8099 incident in CLAUDE.md: a
# scratch target named in one argument that the dangerous step never reads. The
# name of the database is the one thing every one of those steps does read, so
# it is where the rule goes.
#
# THE SAME RULE IS SPELLED IN GO, in backend/internal/platform/testdb, because
# that suite has no PowerShell anywhere near it.
# scripts/check-scratch-db-guard.mjs fails the build if the two spellings
# disagree - the ADR-028 joint, applied to a guard instead of an enum.
$script:ScratchDatabasePrefix = "holler_scratch_"

# The database name out of a postgres URL: the last path segment, with any
# query string removed. Returns "" when there is no name to find, and a caller
# treats that as a refusal rather than as permission - a URL this cannot parse
# is not a URL this can clear.
function Get-DatabaseNameFromUrl {
    param([string]$Url)
    if ([string]::IsNullOrWhiteSpace($Url)) { return "" }
    $withoutQuery = ($Url -split '\?')[0]
    $segments = $withoutQuery.TrimEnd('/') -split '/'
    if ($segments.Length -lt 2) { return "" }
    return $segments[-1]
}

function Test-IsScratchDatabaseName {
    param([string]$Name)
    if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
    return $Name.StartsWith($script:ScratchDatabasePrefix, [System.StringComparison]::OrdinalIgnoreCase)
}

# For a destructive or schema-writing run under an agent shell. A human shell
# is unaffected: the operator's own demo reset targets 'holler' and must.
function Assert-ScratchDatabaseTarget {
    param([string]$ScriptName, [string]$DatabaseName, [string]$Where)
    if (-not (Test-IsAgentShell)) { return }
    if (Test-IsScratchDatabaseName -Name $DatabaseName) { return }
    $shown = if ([string]::IsNullOrWhiteSpace($DatabaseName)) { "<unparseable>" } else { $DatabaseName }
    Deny-AgentRun -ScriptName $ScriptName `
        -Problem "$Where names database '$shown', which is not a scratch database." `
        -WhatToDo "Name a database starting '$script:ScratchDatabasePrefix' (create and drop it yourself), or ask the operator to run this."
}

# For scripts whose only purpose is starting the operator's live stack.
function Assert-NotAgentShell {
    param([string]$ScriptName, [string]$Ports)
    if (-not (Test-IsAgentShell)) { return }
    Deny-AgentRun -ScriptName $ScriptName `
        -Problem "It starts the operator's stack on live ports ($Ports) and has no scratch mode." `
        -WhatToDo "The operator runs this one. An agent that needs a server starts its own on a scratch port, with its own database."
}
