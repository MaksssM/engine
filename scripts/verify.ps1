param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"

function Invoke-Check {
    param(
        [string]$Name,
        [scriptblock]$Action
    )
    Write-Host ""
    Write-Host "=== $Name ===" -ForegroundColor Cyan
    & $Action
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "FAILED: $Name (exit code $LASTEXITCODE)" -ForegroundColor Red
        exit $LASTEXITCODE
    }
    Write-Host "PASSED: $Name" -ForegroundColor Green
}

Invoke-Check "cargo fmt --all -- --check" { cargo fmt --all -- --check }
Invoke-Check "cargo check --all-targets" { cargo check --all-targets }
Invoke-Check "cargo test --all-targets" { cargo test --all-targets }
Invoke-Check "cargo clippy --all-targets" { cargo clippy --all-targets }

if ($Release) {
    Invoke-Check "cargo build --release" { cargo build --release }
}

Write-Host ""
Write-Host "All checks passed." -ForegroundColor Green
exit 0
