#!/bin/bash
# Build the on-screen keyboard's dictation worker into demo/public/whistle/:
# the agg-gui-whistle-worker module (agg_gui_whistle_worker.js + _bg.wasm),
# agg-gui-whistle's worker.js and capture-worklet.js, and whistle.cact when
# the sibling whistle-rust checkout has it (../whistle-rust/models, from its
# scripts/fetch-weights.sh); without it the page fetches the model from
# whistle-rust's GitHub Pages copy.
#
# The worker is built with simd128, which the model's kernels use and every
# current browser runs. The UI module (demo-wasm) is built without it: rustc
# 1.99's LLVM fails to compile agg-gui with simd128 on (whistle-rust's
# demo/README.md, "simd128 and the UI module"). RUSTFLAGS reach every crate
# of a build, so the worker builds on its own, into its own target dir so
# the two builds don't invalidate each other's cache. Always a release
# build: an unoptimised model is too slow to dictate with.
# Usage: demo/build-whistle.sh
set -euo pipefail
cd "$(dirname "$0")/.."
out=demo/public/whistle
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+simd128" \
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/wasm-simd128" \
  wasm-pack build agg-gui-whistle-worker --release --target web \
    --out-dir "../$out" --no-typescript
cp agg-gui-whistle/web/worker.js agg-gui-whistle/web/capture-worklet.js "$out/"
model=../whistle-rust/models/whistle.cact
if [ -f "$model" ] && ! cmp -s "$model" "$out/whistle.cact"; then
  cp "$model" "$out/whistle.cact"
fi
