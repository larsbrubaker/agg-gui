// Build script — bundles TypeScript source and assembles the full site for deployment.
// Usage: bun run build.ts
//
// Output: dist/ directory containing the complete deployable site:
//   dist/
//     index.html
//     public/pkg/   (WASM — must be built first via build:wasm)
//     public/whistle/ (the dictation worker and model, same)
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
mkdirSync(join(DIST_DIR, "agg-gui"), { recursive: true });

// Copy index.html and the favicon it links
cpSync(join(DEMO_DIR, "index.html"), join(DIST_DIR, "index.html"));
cpSync(join(DEMO_DIR, "public", "favicon.svg"), join(DIST_DIR, "public", "favicon.svg"));

// Copy app-owned font assets for lazy loading.
cpSync(join(DEMO_DIR, "assets"), join(DIST_DIR, "assets"), { recursive: true });
cpSync(join(DEMO_DIR, "..", "agg-gui", "readme_hero.png"), join(DIST_DIR, "agg-gui", "readme_hero.png"));

// Copy WASM package (must be pre-built with wasm-pack)
if (!existsSync(PKG_DIR)) {
  console.error("ERROR: public/pkg/ not found. Run 'bun run build:wasm' first.");
  process.exit(1);
}
cpSync(PKG_DIR, join(DIST_DIR, "public", "pkg"), { recursive: true });

// The on-screen keyboard's dictation worker (build-whistle.sh, run by
// build-wasm.sh). whistle.cact is copied when it was fetched into public/whistle/;
// otherwise the page falls back to whistle-rust's GitHub Pages copy.
const WHISTLE_DIR = join(DEMO_DIR, "public", "whistle");
if (!existsSync(join(WHISTLE_DIR, "worker.js"))) {
  console.error("ERROR: public/whistle/ not found. Run build-wasm.sh (or build-whistle.sh) first.");
  process.exit(1);
}
cpSync(WHISTLE_DIR, join(DIST_DIR, "public", "whistle"), { recursive: true });
// Apache-2.0 notices for whistle-rust and the Whistle weights, served beside them.
for (const file of ["THIRD_PARTY_NOTICES.md", "LICENSE-APACHE"]) {
  cpSync(join(DEMO_DIR, "..", "agg-gui-whistle", file), join(DIST_DIR, "public", "whistle", file));
}

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
