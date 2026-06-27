//! Keep the Screen Share web build in sync with the source on every native run.
//!
//! The phone loads the agg-gui **web** build — the `wasm-pack` package
//! (`demo/public/pkg/`) and the bundled TypeScript (`demo/public/dist/bundle.js`)
//! — served straight off disk by [`super::phone_server`].  Those are *build
//! outputs*, not source, so they silently go stale the moment the Rust/TS
//! sources move ahead of the last build: the phone then loads a wasm missing
//! newly-added exports (e.g. `default_font_request`) and dies at "Loading
//! WASM", or never runs at all because `bundle.js` is absent.
//!
//! To make Screen Share always serve the *current* app, [`ensure_current`]
//! rebuilds whatever is stale at launch.  Design constraints:
//!
//! * **Don't stall the desktop.** `start` runs on every native launch and the
//!   wasm shares source with the desktop app, so a blocking rebuild would add
//!   minutes to most launches.  The slow path (wasm) runs on a background
//!   thread; the desktop window comes up immediately.
//! * **Never serve a mismatched pair.** A freshly rebuilt `bundle.js` calling a
//!   not-yet-rebuilt wasm is the very `is not a function` failure we're fixing.
//!   So when the wasm is stale we rebuild the bundle alongside it and swap both
//!   in atomically (build into a staging dir / file, then rename over the live
//!   one).  A phone that connects mid-build keeps getting the previous,
//!   self-consistent pair until the swap lands.
//! * **Stay a convenience.** If `wasm-pack` / `bun` aren't on PATH or a build
//!   fails, we log and carry on serving what's on disk — the desktop app is
//!   unaffected.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// agg-gui repo root (the parent of the `demo-native` crate dir).  Baked in at
/// compile time, exactly like `phone_server`'s `demo_web_dir`.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("demo-native lives under agg-gui/")
        .to_path_buf()
}

/// Rebuild any stale Screen Share web assets so the phone always loads the
/// current app.  Returns immediately; a needed wasm rebuild runs in the
/// background and atomically replaces the live package when done.
pub fn ensure_current() {
    let root = repo_root();
    let pkg = root.join("demo").join("public").join("pkg");
    let wasm = pkg.join("demo_wasm_bg.wasm");
    let bundle = root.join("demo").join("public").join("dist").join("bundle.js");

    // Rust sources that compile into the wasm (over-inclusive is fine — it only
    // costs an extra background rebuild, never a wrong answer).
    let rust_dirs: Vec<PathBuf> = ["agg-gui", "demo-ui", "demo-wgpu", "demo-wasm", "node-editor"]
        .iter()
        .map(|c| root.join(c).join("src"))
        .collect();
    let lock = root.join("Cargo.lock");
    let ts_dirs = vec![root.join("demo").join("src")];
    let index = root.join("demo").join("index.html");
    // `bun` inlines the wasm-bindgen glue (`demo_wasm.js`) into the bundle, so a
    // newer glue (e.g. someone ran `bun run build:wasm` directly) makes the
    // bundle stale even when no .ts changed — otherwise the bundle carries an
    // old glue that mismatches the current wasm and the phone hits a LinkError.
    let glue = pkg.join("demo_wasm.js");

    let wasm_stale = is_stale(&wasm, &rust_dirs, &["rs"], &[lock]);
    let bundle_stale = is_stale(&bundle, &ts_dirs, &["ts"], &[index, glue]);

    if wasm_stale {
        // Rebuild wasm (slow) AND the bundle, swapping both together so the
        // served pair never mixes a new bundle with an old wasm.
        let root = root.clone();
        std::thread::spawn(move || rebuild_wasm_and_bundle(&root));
    } else if bundle_stale {
        // wasm is already current, so the current sources' bundle matches it —
        // a standalone bundle rebuild stays consistent.
        rebuild_bundle(&root);
    }
}

/// `out` is stale if it's missing or older than the newest matching source.
fn is_stale(out: &Path, dirs: &[PathBuf], exts: &[&str], extra: &[PathBuf]) -> bool {
    let out_t = match mtime(out) {
        Some(t) => t,
        None => return true, // never built
    };
    let mut newest: Option<SystemTime> = None;
    for f in extra {
        bump(&mut newest, mtime(f));
    }
    for d in dirs {
        scan_newest(d, exts, &mut newest);
    }
    newest.map_or(false, |n| n > out_t)
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn bump(newest: &mut Option<SystemTime>, t: Option<SystemTime>) {
    if let Some(t) = t {
        if newest.map_or(true, |n| t > n) {
            *newest = Some(t);
        }
    }
}

fn scan_newest(dir: &Path, exts: &[&str], newest: &mut Option<SystemTime>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_newest(&p, exts, newest);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .map_or(false, |e| exts.contains(&e))
        {
            bump(newest, mtime(&p));
        }
    }
}

// The build STAGES under `target/` — which file-watchers (cargo-watch and the
// like) ignore — and only the final atomic swap touches the watched
// `demo/public/`.  This is essential: a watcher that restarts demo-native on any
// `demo/public/` write would otherwise kill the slow (~1 min) wasm build before
// it finished swapping, leaving the wasm "stale" so the next launch rebuilds
// again — an endless rebuild/restart loop.  Staging in `target/` lets the build
// run to completion; the lone swap causes at most one restart, after which the
// freshness check sees current assets and stops.
fn stage_dir(root: &Path) -> PathBuf {
    root.join("target").join("web-build")
}

/// Background path: rebuild the wasm package and the JS bundle into the unwatched
/// staging dir, then swap both over the live ones.  Anything that fails leaves
/// the existing assets untouched.
fn rebuild_wasm_and_bundle(root: &Path) {
    eprintln!("screen-share: web sources changed — rebuilding wasm in the background…");
    let public = root.join("demo").join("public");
    let pkg = public.join("pkg");
    let bundle = public.join("dist").join("bundle.js");
    let stage = stage_dir(root);
    let pkg_stage = stage.join("pkg");
    let bundle_stage = stage.join("bundle.js");

    let _ = std::fs::create_dir_all(&stage);
    let _ = std::fs::remove_dir_all(&pkg_stage);
    // wasm-pack resolves --out-dir relative to the crate dir (demo-wasm), hence
    // the `../target/...` prefix.  target/ is unwatched, so this slow build runs
    // to completion instead of being killed by a watcher restart.
    if !run_in(
        "wasm-pack",
        root,
        &[
            "build",
            "demo-wasm",
            "--target",
            "web",
            "--out-dir",
            "../target/web-build/pkg",
            "--no-typescript",
        ],
    ) {
        let _ = std::fs::remove_dir_all(&pkg_stage);
        return;
    }

    // Swap the new wasm package in BEFORE bundling: `bun` inlines the glue from
    // the *live* `public/pkg/demo_wasm.js`, so the bundle must see the new glue
    // to match the new wasm.
    if let Err(e) = swap_dir(&pkg, &pkg_stage) {
        eprintln!("screen-share: wasm built but swap failed ({e}); keeping previous pkg");
        return;
    }
    if !build_bundle(root, "../target/web-build/bundle.js") {
        eprintln!(
            "screen-share: wasm updated but bundle rebuild failed; the served bundle's glue \
             now mismatches the wasm — run `bun run build` to refresh it"
        );
        let _ = std::fs::remove_file(&bundle_stage);
        return;
    }
    if let Err(e) = swap_file(&bundle, &bundle_stage) {
        eprintln!("screen-share: bundle built but swap failed ({e}); run `bun run build`");
        return;
    }
    eprintln!("screen-share: web assets updated — reload the phone to pick them up");
}

/// Fast path: the bundle is the only stale artifact.  Still build into the
/// unwatched staging dir and swap, so a watcher doesn't restart mid-write.
fn rebuild_bundle(root: &Path) {
    eprintln!("screen-share: rebuilding JS bundle…");
    let stage = stage_dir(root);
    let bundle = root.join("demo").join("public").join("dist").join("bundle.js");
    let bundle_stage = stage.join("bundle.js");
    let _ = std::fs::create_dir_all(&stage);
    if !build_bundle(root, "../target/web-build/bundle.js") {
        return;
    }
    if let Err(e) = swap_file(&bundle, &bundle_stage) {
        eprintln!("screen-share: bundle built but swap failed ({e}); run `bun run build`");
        return;
    }
    eprintln!("screen-share: JS bundle updated");
}

fn build_bundle(root: &Path, outfile: &str) -> bool {
    let demo = root.join("demo");
    run_in(
        "bun",
        &demo,
        &[
            "build",
            "src/app.ts",
            "--outfile",
            outfile,
            "--minify",
            "--target",
            "browser",
        ],
    )
}

fn run_in(program: &str, cwd: &Path, args: &[&str]) -> bool {
    match Command::new(program).current_dir(cwd).args(args).status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!("screen-share: `{program}` exited with {s}; serving existing assets");
            false
        }
        Err(e) => {
            eprintln!(
                "screen-share: could not run `{program}` ({e}); serving existing assets. \
                 Install it (or run `bun run build:wasm` / `bun run build`) to refresh by hand."
            );
            false
        }
    }
}

/// Replace directory `target` with `staging` as atomically as the platform
/// allows (Windows can't rename onto an existing dir), restoring on failure.
///
/// The temporary `prev` is kept next to `staging` (under the unwatched
/// `target/` dir), never next to `target` (the watched, but gitignored,
/// `demo/public/`).  A `demo/public/pkg.prev` would NOT match the
/// `demo/public/pkg/` gitignore entry, so a gitignore-respecting watcher would
/// see it and restart us — re-creating the rebuild loop this whole module
/// exists to avoid.
fn swap_dir(target: &Path, staging: &Path) -> std::io::Result<()> {
    let prev = staging.with_extension("prev");
    let _ = std::fs::remove_dir_all(&prev);
    if target.exists() {
        std::fs::rename(target, &prev)?;
    }
    match std::fs::rename(staging, target) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&prev);
            Ok(())
        }
        Err(e) => {
            if prev.exists() {
                let _ = std::fs::rename(&prev, target);
            }
            Err(e)
        }
    }
}

fn swap_file(target: &Path, staging: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(target);
    std::fs::rename(staging, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn touch(p: &Path, mtime: SystemTime) {
        std::fs::write(p, b"x").unwrap();
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(mtime).unwrap();
    }

    #[test]
    fn missing_output_is_stale() {
        let dir = std::env::temp_dir().join("aggui_web_build_missing");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        touch(&src.join("a.rs"), SystemTime::now());
        // Output that was never built is always stale.
        assert!(is_stale(&dir.join("nope.wasm"), &[src], &["rs"], &[]));
    }

    #[test]
    fn newer_source_is_stale_older_is_fresh() {
        let dir = std::env::temp_dir().join("aggui_web_build_mtime");
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();

        let t0 = SystemTime::now() - Duration::from_secs(100);
        let out = dir.join("out.wasm");
        touch(&out, t0); // built at t0

        // Source older than the build → fresh (no rebuild).
        touch(&src.join("old.rs"), t0 - Duration::from_secs(10));
        assert!(!is_stale(&out, &[src.clone()], &["rs"], &[]));

        // A source edited after the build → stale; non-.rs is ignored.
        touch(&src.join("ignored.txt"), t0 + Duration::from_secs(50));
        assert!(!is_stale(&out, &[src.clone()], &["rs"], &[]));
        touch(&src.join("new.rs"), t0 + Duration::from_secs(50));
        assert!(is_stale(&out, &[src.clone()], &["rs"], &[]));
    }

    #[test]
    fn extra_file_counts_as_a_source() {
        let dir = std::env::temp_dir().join("aggui_web_build_extra");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let t0 = SystemTime::now() - Duration::from_secs(100);
        let out = dir.join("out.wasm");
        touch(&out, t0);
        let lock = dir.join("Cargo.lock");
        touch(&lock, t0 + Duration::from_secs(50)); // e.g. a dep version bump
        assert!(is_stale(&out, &[], &["rs"], &[lock]));
    }
}
