#!/bin/bash
# Build the demo's wasm (run from anywhere; paths are the repository's):
#   demo/public/pkg/      demo-wasm, the UI module
#   demo/public/whistle/  the dictation worker (demo/build-whistle.sh)
# Usage: demo/build-wasm.sh [--dev]   (--dev builds the UI module unoptimised)
set -euo pipefail
cd "$(dirname "$0")/.."
profile=--release
if [ "${1:-}" = "--dev" ]; then
  profile=--dev
fi
wasm-pack build demo-wasm "$profile" --target web --out-dir ../demo/public/pkg --no-typescript
demo/build-whistle.sh
