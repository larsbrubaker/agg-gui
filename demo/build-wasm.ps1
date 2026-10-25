# Build the demo's wasm (PowerShell twin of build-wasm.sh; run from anywhere):
#   demo/public/pkg/      demo-wasm, the UI module
#   demo/public/whistle/  the dictation worker (build-whistle.ps1)
# Usage: demo/build-wasm.ps1 [-Dev]   (-Dev builds the UI module unoptimised)
param([switch]$Dev)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$profileFlag = if ($Dev) { "--dev" } else { "--release" }
wasm-pack build demo-wasm $profileFlag --target web --out-dir ../demo/public/pkg --no-typescript
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& (Join-Path $PSScriptRoot "build-whistle.ps1")
exit $LASTEXITCODE
