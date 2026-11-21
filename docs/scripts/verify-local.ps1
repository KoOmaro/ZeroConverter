# Windows local verification (the same checks CI runs, plus a secret scan). Requires Node/npm, Rust/Cargo, git and Gitleaks.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File .\docs\scripts\verify-local.ps1
# Skip switches are developer diagnostics ONLY: a skipped stage cannot count as release evidence.
param(
    [switch]$SkipNpmCi,
    [switch]$SkipSecretScan
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$rust = Join-Path $root 'src-tauri'

function Require-Tool([string]$name) {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
        throw "Required tool missing: $name"
    }
}
function Invoke-Checked([string]$workingDir, [string]$command, [string[]]$arguments) {
    Push-Location $workingDir
    try {
        Write-Host ("[RUN] " + $command + " " + ($arguments -join ' '))
        & $command @arguments
        if ($LASTEXITCODE -ne 0) { throw "$command exited with code $LASTEXITCODE" }
    }
    finally { Pop-Location }
}

Require-Tool git
Require-Tool node
Require-Tool npm
Require-Tool cargo
if (-not $SkipSecretScan) { Require-Tool gitleaks }

Invoke-Checked $root git @('rev-parse', 'HEAD')
Invoke-Checked $root node @('--version')
Invoke-Checked $root npm @('--version')
Invoke-Checked $root cargo @('--version')
if (-not $SkipNpmCi) { Invoke-Checked $root npm @('ci') }
Invoke-Checked $root npm @('run', 'build')
Invoke-Checked $root npm @('run', 'check:frontend-boundaries')
Invoke-Checked $root npm @('run', 'test:ui')
Invoke-Checked $root node @('scripts/licenses.mjs', '--check')
# Running the frontend build first creates dist/ for the Tauri build script.
Invoke-Checked $rust cargo @('metadata', '--locked', '--format-version', '1', '--no-deps')
Invoke-Checked $rust cargo @('fmt', '--all', '--check')
Invoke-Checked $rust cargo @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
Invoke-Checked $rust cargo @('test', '--workspace', '--no-fail-fast', '--locked')
Invoke-Checked $rust cargo @('check', '-p', 'zeroconverter-core', '-p', 'zeroconverter-cli', '--no-default-features', '--locked')
if (-not $SkipSecretScan) {
    # History scan; Gitleaks version and redacted report should be preserved privately.
    Invoke-Checked $root gitleaks @('git', '--redact', '--source', '.')
} else {
    Write-Warning 'SKIPPED: all-history secret scan; NOT valid for publication'
}
if ($SkipNpmCi) { Write-Warning 'SKIPPED: npm ci; NOT valid for publication' }
Write-Host '[PASS] All requested non-skipped local verification stages exited with code 0.'
