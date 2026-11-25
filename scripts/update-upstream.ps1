# Part of Agent Record Adapter (see NOTICE).
# Takes the latest main of agent-desktop and agent-browser (the submodules
# under vendor/, never edited) and checks that our layer still fits them.
#
#   powershell -ExecutionPolicy Bypass -File scripts\update-upstream.ps1
#
# Stops at the first step that fails, with what to look at.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Step($name, [scriptblock]$work) {
    Write-Host "== $name"
    & $work
    if ($LASTEXITCODE -ne 0) { throw "Step failed: $name" }
}

Step 'Pull upstream main (submodules)' { git submodule update --remote vendor/agent-desktop vendor/agent-browser }
git -C vendor/agent-desktop log --oneline -1
git -C vendor/agent-browser log --oneline -1

# A new or changed adapter method upstream must be forwarded, or it would fall
# to its not-supported default instead of reaching upstream's WindowsAdapter.
Step 'Regenerate the adapter forwarding' { python scripts/gen_forwarding.py }

# agent-browser inside agent-record: upstream's manifest under our name, and
# our lock pinned to upstream's dependency versions where cargo allows.
Step 'Sync the web engine with agent-browser' { python scripts/sync_web_engine.py --pin }

# The engine reports upstream's version.
$version = (Select-String -Path vendor/agent-desktop/Cargo.toml -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value
(Get-Content crates/engine/Cargo.toml) -replace '^version = ".*"', "version = `"$version`"" |
    Set-Content -Encoding utf8 crates/engine/Cargo.toml
Write-Host "engine version $version"

Step 'Build everything' { cargo build --workspace }
Step 'Our unit tests' { cargo test -p agent-record-core -p agent-record-windows -p agent-record -p agent-record-browser }
Step 'Upstream CLI tests with our adapter (background on)' {
    cargo test -p agent-record-engine --bins -- --skip shell_surface --skip surfaces --skip contract_command_surface
}
$env:AGENT_DESKTOP_BACKGROUND = '0'
Step 'Upstream CLI tests with our adapter (background off)' {
    cargo test -p agent-record-engine --bins -- --skip shell_surface --skip surfaces --skip contract_command_surface
}
Remove-Item Env:AGENT_DESKTOP_BACKGROUND

Write-Host ''
Write-Host 'Now run the live checks (tests/background/README.md) and, if all pass, commit the new submodule commits.'
Write-Host 'Files copied from upstream private helpers to keep in step (docs/architecture.md, "Copies of upstream helpers").'
