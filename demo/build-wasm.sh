#!/bin/bash
# Build the WASM package from demo-wasm/
# Output goes to demo/public/pkg/
wasm-pack build demo-wasm --target web --out-dir ../demo/public/pkg --no-typescript
