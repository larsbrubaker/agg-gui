// Build script — bundles TypeScript source and assembles the full site for deployment.
// Usage: bun run build.ts
//
// Output: dist/ directory containing the complete deployable site:
//   dist/
//     index.html
//     public/pkg/   (WASM — must be built first via build:wasm)
//     public/dist/  (bundled JS)

import { join } from "path";
import { mkdirSync, cpSync, rmSync, existsSync } from "fs";

const DEMO_DIR = import.meta.dir;
const DIST_DIR = join(DEMO_DIR, "dist");
const PKG_DIR  = join(DEMO_DIR, "public", "pkg");

// Clean and recreate dist/
if (existsSync(DIST_DIR)) rmSync(DIST_DIR, { recursive: true });
mkdirSync(join(DIST_DIR, "public", "dist"), { recursive: true });
mkdirSync(join(DIST_DIR, "public", "pkg"),  { recursive: true });

// Copy index.html
cpSync(join(DEMO_DIR, "index.html"), join(DIST_DIR, "index.html"));

// Copy WASM package (must be pre-built with wasm-pack)
if (!existsSync(PKG_DIR)) {
  console.error("ERROR: public/pkg/ not found. Run 'bun run build:wasm' first.");
  process.exit(1);
}
cpSync(PKG_DIR, join(DIST_DIR, "public", "pkg"), { recursive: true });

// Bundle TypeScript
const result = await Bun.build({
  entrypoints: [join(DEMO_DIR, "src", "app.ts")],
  outdir: join(DIST_DIR, "public", "dist"),
  naming: "bundle.js",
  minify: true,
  target: "browser",
});

if (!result.success) {
  for (const log of result.logs) console.error(log);
  process.exit(1);
}

console.log("Build complete → dist/");
