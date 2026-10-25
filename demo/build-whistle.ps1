# Build the on-screen keyboard's dictation worker into demo/public/whistle/
# (PowerShell twin of build-whistle.sh, which explains the simd128 split):
# the agg-gui-whistle-worker module, agg-gui-whistle's worker.js and
# capture-worklet.js, and whistle.cact when ../whistle-rust/models has it.
# As there, a relative CARGO_TARGET_DIR is made absolute against the
# repository root, since wasm-pack runs cargo from the crate's directory.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$out = "demo/public/whistle"
$savedFlags = $env:RUSTFLAGS
$savedTarget = $env:CARGO_TARGET_DIR
try {
    $env:RUSTFLAGS = "$savedFlags -C target-feature=+simd128".Trim()
    $base = if ($savedTarget) { $savedTarget } else { "target" }
    if (-not [System.IO.Path]::IsPathRooted($base)) { $base = Join-Path (Get-Location).Path $base }
    $env:CARGO_TARGET_DIR = Join-Path $base "wasm-simd128"
    wasm-pack build agg-gui-whistle-worker --release --target web --out-dir "../$out" --no-typescript
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    $env:RUSTFLAGS = $savedFlags
    $env:CARGO_TARGET_DIR = $savedTarget
}
Copy-Item agg-gui-whistle/web/worker.js, agg-gui-whistle/web/capture-worklet.js $out
$model = "../whistle-rust/models/whistle.cact"
if (Test-Path $model) { Copy-Item $model (Join-Path $out "whistle.cact") }
