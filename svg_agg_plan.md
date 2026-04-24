# SVG Rendering Through the Gfx Bridge: Implementation Plan

**Audience:** Developers integrating SVG support into `agg-gui` using the existing graphics-context bridge over `agg-rust` and hardware renderers.
**Goal:** Parse arbitrary SVG documents and render them at production quality through the existing `agg-gui` graphics bridge, so the same SVG rendering code can target RGBA software bitmaps, LCD coverage bitmaps, and hardware output.

---

## 1. Library Selection

### Parser: `usvg`

Use [`usvg`](https://crates.io/crates/usvg) (part of the `resvg` project) as the SVG parser. It is the most robust and actively maintained SVG parser in the Rust ecosystem.

**Why usvg specifically:**

- It does not just parse — it **normalizes**. SVG's messy features (CSS inheritance, `use` elements, nested transforms, relative units, `currentColor`, percentages, etc.) are all resolved into a flat, strongly-typed tree where every node carries fully-resolved absolute values.
- It is explicitly designed as a **preprocessing layer in front of a rasterizer** — exactly the role we need it to fill in front of AGG.
- Output paths are flattened to absolute-coordinate `MoveTo`/`LineTo`/`QuadTo`/`CubicTo`/`Close` segments, which map 1:1 onto the bridge path vocabulary.
- Stroke dashing, text shaping (optional), gradient stops, clip paths, and filter graphs are all pre-resolved.
- Battle-tested in production (Chromium-quality SVG conformance is the project's stated goal).

### Reference implementation: `resvg`

We will not use `resvg` directly (it renders via `tiny-skia`, not our gfx bridge), but the **`resvg` source code is the canonical reference for how to walk a `usvg` tree and dispatch drawing calls**. Developers should keep the `resvg` repository checked out as a reference. The integration work is essentially: take what `resvg` does with `tiny-skia`, and do the equivalent through `DrawCtx`-style bridge calls.

### Companion libraries

- **`clipper2-rust`** (already in our toolchain): used for SVG clip-path intersection and any stroke-to-fill conversion needed beyond the bridge stroke primitive.
- **`fontdb`** + **`rustybuzz`** (pulled in transitively by `usvg` if text-to-path is enabled): for converting `<text>` elements to outlined paths so they can flow through the same rendering path as everything else.

### Libraries we considered and rejected

| Crate | Why not |
|-------|---------|
| `svg` | Low-level reader/writer only. No CSS resolution, no inheritance, no unit conversion. Would require us to reimplement most of `usvg`. |
| `roxmltree` / `xmlparser` | XML-level only. These are what `usvg` uses internally. Too low-level. |
| `svgtypes` | Useful for parsing individual SVG value types (paths, transforms, colors). Already a dependency of `usvg`; not a full parser. |

---

## 2. Architecture Overview

```
SVG file/string
     │
     ▼
┌─────────────────┐
│   usvg::Tree    │  Parse + normalize (CSS, transforms, units, dashing)
└─────────────────┘
     │
     ▼
┌─────────────────┐
│  SVG walker     │  Depth-first traversal, accumulating SVG paint state
└─────────────────┘
     │
     ▼
┌─────────────────┐
│   Gfx bridge    │  Emits only DrawCtx-style primitives/state changes
└─────────────────┘
     │
     ├──► agg_rgb      ──► GfxCtx over RGBA Framebuffer
     │
     ├──► agg_lcd      ──► LcdGfxCtx over LcdBuffer
     │
     └──► agg_hardware ──► hardware DrawCtx / GL-backed renderer
```

The important architectural constraint is that SVG rendering does **not** create a separate AGG-only raster path. The SVG walker converts `usvg` nodes into calls on our graphics bridge (`DrawCtx`-shaped state, path, image, text, layer, and clip operations). Backend-specific code lives behind that bridge:

- `agg_rgb`: the normal software bitmap target, currently `GfxCtx` over an RGBA `Framebuffer`.
- `agg_lcd`: the LCD coverage bitmap target, currently `LcdGfxCtx` over an `LcdBuffer`.
- `agg_hardware`: the hardware target, using the same logical drawing operations through the hardware context.

Every SVG feature must first be expressed in bridge-level terms. If a feature needs a new primitive, add it to the bridge deliberately and implement it for all required targets, rather than bypassing the bridge from the SVG renderer.

The SVG renderer itself is part of the `agg-gui` library crate, not test or demo infrastructure. Put the implementation under the library source tree (for example `agg-gui/src/svg.rs` plus submodules as it grows), export the public API from `agg-gui/src/lib.rs`, and have tests, demos, and applications call that library API.

The SVG walker is still a streaming traversal — no intermediate scene graph beyond what `usvg` already gives us.

```
SVG path node
     │
     ├──► convert segments ──► ctx.begin_path()
     │                         ctx.move_to()/line_to()/quad_to()/cubic_to()
     │                         ctx.close_path()
     │
     ├──► fill paint       ──► ctx.set_fill_color()/gradient/pattern primitive
     │                         ctx.fill()
     │
     ├──► stroke paint     ──► ctx.set_stroke_color()
     │                         ctx.set_line_width()/cap/join()
     │                         ctx.stroke()
     │
     ├──► group/transform  ──► ctx.save(), ctx.set_transform()/translate()/scale()
     │                         recurse, ctx.restore()
     │
     ├──► image node       ──► bridge image-blit primitive
     │
     └──► clip/mask        ──► bridge clip/layer/mask primitive
```

---

## 3. Phased Implementation Plan

The plan is broken into phases that each produce a working, testable subset of SVG. Ship phase by phase.

### Phase 1 — Foundation (Week 1)

**Deliverable:** Render solid-filled, untransformed paths from a parsed SVG through the gfx bridge, with the same renderer callable for `agg_rgb`, `agg_lcd`, and `agg_hardware` targets.

Tasks:

1. Add dependencies. Pin `usvg` to a specific recent version (0.44 or later — the segment iterator API changed significantly post-0.40).
2. Add the renderer as library code in the `agg-gui` crate and expose it from `lib.rs`. The SVG test suite and demo viewer must import and call this API; they must not contain their own renderer copy.
3. Build the tree-walker scaffold. Recursive descent over `usvg::Node`, with an SVG paint-state stack that maps into `DrawCtx` state.
4. Define the public rendering entry point around the bridge, not around a concrete framebuffer:
   ```rust
   pub fn render_svg(tree: &usvg::Tree, ctx: &mut dyn DrawCtx) -> Result<(), SvgRenderError> {
       // Walk tree and emit bridge calls only.
       Ok(())
   }
   ```
5. Implement the path-segment converter as bridge calls:
   ```rust
   fn emit_usvg_path(path: &usvg::Path, ctx: &mut dyn DrawCtx) {
       ctx.begin_path();
       for seg in path.data().segments() {
           match seg {
               PathSegment::MoveTo(p)         => ctx.move_to(p.x as f64, p.y as f64),
               PathSegment::LineTo(p)         => ctx.line_to(p.x as f64, p.y as f64),
               PathSegment::QuadTo(p1, p2)    => ctx.quad_to(p1.x as f64, p1.y as f64,
                                                             p2.x as f64, p2.y as f64),
               PathSegment::CubicTo(p1,p2,p3) => ctx.cubic_to(p1.x as f64, p1.y as f64,
                                                              p2.x as f64, p2.y as f64,
                                                              p3.x as f64, p3.y as f64),
               PathSegment::Close             => ctx.close_path(),
           }
       }
   }
   ```
6. Implement solid fill dispatch by setting the bridge fill paint and calling `ctx.fill()`. Map `usvg::FillRule` to a bridge-level fill-rule setting; if the bridge lacks this today, add it there and implement it for each target.
7. Set up test/demo target factories for `agg_rgb`, `agg_lcd`, and `agg_hardware`. These factories own the framebuffer/buffer/window setup, then call the same library `render_svg(..., &mut dyn DrawCtx)` function.

**Acceptance:** All tests under `resvg-test-suite/svg/shapes/` and `resvg-test-suite/svg/paths/` pass.

---

### Phase 2 — Strokes and transforms (Week 2)

Tasks:

1. Map each node's SVG transform into the bridge transform stack. `usvg` exposes transforms as `tiny_skia_path::Transform` (a 6-element affine); convert them into `TransAffine`, including the SVG-to-project coordinate conversion described below.
2. Map `usvg::Stroke` fields onto bridge state:
   - `width` → stroke width
   - `linecap` (Butt/Round/Square) → AGG line cap
   - `linejoin` (Miter/Round/Bevel) → AGG line join
   - `miterlimit` → miter limit
3. Dashing: easiest path is to let `usvg` pre-flatten dashes (it does this by default). If we ever need to preserve dashes through non-affine effects, add a bridge dash primitive and implement it behind the bridge rather than calling `ConvDash` directly from the SVG layer.
4. Stroke-to-fill ordering: per SVG spec, fill is painted before stroke. Render in that order per node.

**Acceptance:** All tests under `resvg-test-suite/svg/painting-stroke-*` and `resvg-test-suite/svg/structure-transform-*` pass.

---

### Phase 3 — Gradients and patterns (Weeks 3–4)

Tasks:

1. **Linear gradients:** Map `usvg::LinearGradient` to a bridge gradient paint. The RGBA and LCD backends can implement it with AGG `GradientX` / `GradientY` / `SpanGradient`; the hardware backend can implement the same paint with shader uniforms/textures.
2. **Radial gradients:** Map `usvg::RadialGradient` to a bridge radial/focal gradient paint. The RGBA and LCD backends can implement it with AGG `GradientRadial` / `GradientRadialFocus`; the hardware backend can use the matching shader path.
3. **Spread modes:** Pad / Reflect / Repeat — represent these at the bridge paint level and implement them per target.
4. **Patterns:** Render the pattern's content tree into an offscreen target through the same bridge, then feed the resulting pattern source back through a bridge pattern primitive. Pattern transforms apply on top.

**Acceptance:** All tests under `resvg-test-suite/svg/pservers-grad-*` and `resvg-test-suite/svg/pservers-pattern-*` pass.

---

### Phase 4 — Clipping and masking (Week 5)

This is where we lean on `clipper2-rust`.

Tasks:

1. **Simple clip paths (single shape):** Express clip state as a bridge primitive. Software implementations may rasterize into `alpha_mask_u8` and render through `PixfmtAmaskAdaptor`; hardware may use stencil/scissor/texture masks.
2. **Compound clip paths (multiple shapes intersected):** Use `clipper2-rust`'s `intersect_64` to compute the polygon intersection, then submit the result through the bridge clip-mask primitive.
3. **Nested clipping:** Push/pop a clip stack. Each push intersects the new clip with the current effective clip via `clipper2-rust`, then updates bridge clip state.
4. **Masks (`<mask>`):** Render the mask content through the bridge into a grayscale/luminance mask target, then apply that mask through the bridge mask primitive.

**Acceptance:** All tests under `resvg-test-suite/svg/masking-path-*` and `resvg-test-suite/svg/masking-mask-*` pass.

---

### Phase 5 — Text (Week 6)

Recommended approach: enable `usvg`'s built-in text-to-path conversion. Text becomes outlined paths and flows through the existing path renderer with no additional code.

Tasks:

1. Configure `usvg::Options` with a populated `fontdb`. Bundle a fallback font (DejaVu Sans or Noto Sans) for missing-font cases.
2. Call `usvg::Tree::postprocess()` (or the equivalent in the version we pin) to convert all `<text>` to paths before walking.
3. Verify font fallback behavior on systems without the requested fonts.

**Acceptance:** All tests under `resvg-test-suite/svg/text-*` render with correct glyphs and positioning.

---

### Phase 6 — Images and `<use>` (Week 7)

Tasks:

1. **Raster images:** Decode `usvg::Image` data (PNG/JPEG) using `image` crate. Blit through a bridge image primitive with interpolation selected from the SVG `image-rendering` hint; software backends can use AGG image span generators behind that primitive.
2. **Nested SVG images:** `usvg` flattens these into the main tree, so no special handling is usually needed.
3. **`<use>` elements:** Already expanded by `usvg`. No work required.

**Acceptance:** Images appear with correct positioning, scaling, and interpolation quality.

---

### Phase 7 — Filters (Weeks 8+, optional)

This is the largest feature surface and lowest priority. `usvg` resolves the filter graph but does not execute it.

Strategy: implement filters **on demand**, starting with the most common:

1. `feGaussianBlur` — AGG's stack blur is a fast, visually acceptable approximation. True Gaussian if needed.
2. `feColorMatrix` — straightforward per-pixel transform.
3. `feOffset`, `feFlood`, `feComposite`, `feMerge` — needed for drop shadows, which account for the majority of real-world filter usage.
4. Everything else — defer until a real document needs it.

**Acceptance:** Drop-shadow filter chain renders correctly. Other filters degrade gracefully (skip with a warning rather than crash).

---

## 4. Mapping Reference

Quick lookup for developers:

| SVG / usvg concept | gfx bridge operation |
|---|---|
| `usvg::Path` segments | `DrawCtx::begin_path` plus `move_to` / `line_to` / `quad_to` / `cubic_to` / `close_path` |
| `usvg::Transform` | `DrawCtx` transform stack using `TransAffine` |
| `usvg::Stroke` | Bridge stroke state (width, cap, join, miter limit) followed by `DrawCtx::stroke` |
| `usvg::Fill` (solid) | Bridge fill paint followed by `DrawCtx::fill` |
| `usvg::LinearGradient` | Bridge gradient paint; backend may use AGG `SpanGradient` or GPU shader |
| `usvg::RadialGradient` | Bridge radial/focal gradient paint; backend may use AGG `GradientRadial` or GPU shader |
| `usvg::Pattern` | Bridge offscreen render + pattern paint |
| `usvg::FillRule` | Bridge fill-rule state implemented by each target |
| Single clip path | Bridge clip-mask primitive |
| Compound clip paths | `clipper2-rust::intersect_64` at bridge/SVG layer, then bridge clip-mask primitive |
| `<mask>` | Render mask content through bridge into mask target, then apply through bridge mask primitive |
| `<image>` | `image` crate decode → bridge image-blit primitive |
| `<text>` | `usvg` text-to-path → flows through bridge path pipeline |
| `feGaussianBlur` | Bridge filter/layer primitive; backend may use AGG stack blur or GPU shader |

---

## 5. Coordinate System and Numeric Type Notes

- The bridge and widget system use **Y-up** coordinates: origin at bottom-left, positive Y upward. SVG uses Y-down document coordinates. The SVG renderer owns this conversion at its root transform, so backends do not special-case SVG orientation.
- AGG 2.6 (and our port) uses `f64` throughout. `usvg` returns coordinates as `f32` (it builds on `tiny-skia-path`). Cast at the bridge boundary; no precision concerns for typical SVG content.
- `usvg` resolves the SVG `viewBox` and outer transform into the root node's transform. Compose that with the SVG-to-Y-up transform for the chosen viewport before emitting bridge calls.
- Keep target dimensions in physical pixels at the target factory boundary. The SVG walker should operate in logical SVG/user units and let the target context's transform map those units to the backing store.

---

## 6. Testing Strategy

We use a single source of test SVGs — the **`resvg` test suite** — for both automated regression testing and a developer-facing visual demo. One corpus, two consumers. This keeps maintenance low and ensures the demo always reflects what CI is actually testing.

### 6.1 Test corpus: `resvg-test-suite`

**Source:** https://github.com/RazrFalcon/resvg-test-suite (also mirrored under `linebender/resvg-test-suite`)
**License:** MIT
**Size:** ~1,500 SVG files with paired reference PNGs

**Why this suite specifically:**

- It was built to test an SVG renderer in Rust by the team that built `usvg`, so it is exactly aligned with the parser we're using.
- Reference PNGs were rendered by a known-good implementation, not hand-drawn or browser-screenshotted, so they're pixel-stable and reproducible.
- It is organized by feature, with directory and filename conventions that map cleanly to our implementation phases.
- It focuses on the pathological edge cases that actually trip renderers up — gradient stops at identical offsets, miter limits at acute angles, dashed strokes on cubic Béziers, nested clip paths, etc.
- It bundles its own fonts (in `fonts/`) and referenced images (in `images/`), so tests are deterministic across machines.

**Repository layout we'll consume:**

```
resvg-test-suite/
├── svg/              ← test SVG files, organized by feature
├── png/              ← expected reference rendering for each SVG
├── images/           ← raster assets referenced by SVGs
└── fonts/            ← bundled fonts for text tests
```

**Vendoring:** Add `resvg-test-suite` as a git submodule under `tests/resvg-test-suite/`. Pin to a specific commit hash so reference renderings don't drift under us. Update the pin deliberately when we want to pull in new tests.

### 6.2 Integration tests (CI)

**Location:** `tests/svg_regression.rs`

**Mechanism:** A single parameterized integration test that walks `tests/resvg-test-suite/svg/`, imports the SVG renderer from the `agg-gui` library crate, renders each SVG through that shared library API into each available target (`agg_rgb`, `agg_lcd`, and headless/recorded `agg_hardware` where CI supports it), and diffs against the corresponding PNG in `tests/resvg-test-suite/png/`.

```rust
// tests/svg_regression.rs
use std::path::PathBuf;

#[test]
fn resvg_test_suite() {
    let suite_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resvg-test-suite");
    let svg_dir = suite_root.join("svg");
    let png_dir = suite_root.join("png");

    let mut failures = Vec::new();
    let mut total = 0;

    for entry in walkdir::WalkDir::new(&svg_dir).into_iter().filter_map(Result::ok) {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        total += 1;

        let rel = entry.path().strip_prefix(&svg_dir).unwrap();
        let expected_png = png_dir.join(rel).with_extension("png");
        if !expected_png.exists() {
            continue; // some SVGs in the suite have no reference; skip
        }

        match render_all_targets_and_compare(entry.path(), &expected_png) {
            Ok(diff_ratio) if diff_ratio <= TOLERANCE => {}
            Ok(diff_ratio) => failures.push((rel.to_path_buf(), diff_ratio)),
            Err(e)         => failures.push((rel.to_path_buf(), f64::NAN)), // log e
        }
    }

    // Write a JSON results file for the demo viewer to consume
    write_results_json(&failures, total);

    if !failures.is_empty() {
        panic!("{} of {} SVG tests failed", failures.len(), total);
    }
}
```

**Pixel-diff tolerance:** Anti-aliased rasterizers will not produce byte-identical output to a different rasterizer. We compare with a small per-pixel tolerance and an overall mismatched-pixel ratio threshold.

- Per-pixel: max channel delta ≤ 4 (out of 255) is considered a match.
- Per-image: ≤ 0.1% of pixels exceeding the per-pixel threshold is considered a pass.
- These thresholds are starting points; tune empirically once we have baseline numbers. Use `image-compare` or `dssim` crates for the diff math.

**Allowlist for known divergences:** Some tests will legitimately differ from `tiny-skia`'s output (e.g., subpixel text positioning, edge anti-aliasing on near-horizontal strokes). Maintain `tests/known_diffs.toml` listing tests where we accept a higher tolerance, with a one-line justification per entry. Anything not on the allowlist must pass at the default tolerance.

**Running locally:**
```bash
cargo test --test svg_regression --release
```
Use `--release` — debug builds of the rasterizer are 10–20× slower and the suite is large.

**Sharded CI runs:** ~1,500 tests at release-build speed should complete in a few minutes single-threaded. Use `cargo nextest` with parallelism for faster feedback.

**Output artifact:** The test writes `target/svg-regression/results.json` containing `{ test_path, status_by_target, diff_ratio_by_target, reference_path, agg_rgb_path, agg_lcd_path, agg_hardware_path }` per test. The demo viewer reads this file directly. This means **the demo always reflects the latest CI run** — there's no separate process for generating demo data.

### 6.3 Visual demo viewer

A standalone interactive viewer that lets developers and users page through every test SVG with side-by-side comparison across every production output path.

**Format:** Single-page web app, served as a static site. Compiled to WebAssembly using `agg-rust`'s existing WASM setup (the same toolchain that powers the AGG demo gallery at `larsbrubaker.github.io/agg-rust/`). This means **the demo renders SVGs live in the browser using the actual pipeline** — not just showing pre-rendered PNGs.

**Location:** `demos/svg-viewer/`

**Layout per test:** when a demo window is open it always shows exactly four render columns in this order:

```
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ ← Prev    [ Test 247 / 1,492 ]   gradients/linear-04.svg   Next →                             │
│           Status: RGB ✓  LCD ✓  HW ✓                                                         │
├────────────────────┬────────────────────┬────────────────────┬────────────────────┤
│   reference.png    │ agg-rgba-bitmap    │  agg-lcd-bitmap    │  hardware render   │
│   resvg suite      │ shared SVG walker  │ shared SVG walker  │ shared SVG walker  │
├────────────────────┴────────────────────┴────────────────────┴────────────────────┤
│ Per-target diff controls / overlay toggles                                         │
│ <svg> source (collapsible)                                                         │
│ <svg viewBox="0 0 100 100">                                                        │
│   <linearGradient id="g">...                                                       │
└────────────────────────────────────────────────────────────────────────────────────┘
```

**Four columns per test:**
1. **`reference.png`** — the PNG from `resvg-test-suite/png/`, displayed as-is.
2. **`agg-rgba-bitmap render`** — the SVG rendered through the shared walker into the RGBA bitmap target.
3. **`agg-lcd-bitmap render`** — the same SVG rendered through the shared walker into the LCD coverage bitmap target, then composited for display.
4. **`hardware render`** — the same SVG rendered through the shared walker into the hardware target.

Diff overlays are per target and compare each rendered output to `reference.png`. They are controls/overlays on the corresponding render column, not a replacement for any of the four required columns.

**Navigation:**
- Prev/Next buttons and arrow-key shortcuts.
- A **filter sidebar** with checkboxes for each feature category (paths, strokes, gradients, clipping, text, filters, etc.), derived from the suite's directory structure.
- A **status filter** — show all / passing only / failing only / known-diff. Driven by the `results.json` from CI.
- A **search box** that matches against test path/filename.
- URL hash reflects current test (`#test=gradients/linear-04`) so links are shareable.

**Aggregate dashboard view:** A landing page with summary stats — pass count, fail count, pass rate per feature category, and a small chart over time if we persist history. Same idea as the resvg suite's own support table at `razrfalcon.github.io/resvg-test-suite/svg-support-table.html`.

**Build:**
```bash
cd demos/svg-viewer
wasm-pack build --target web --release
# Outputs static files to demos/svg-viewer/dist/ — serve with any static server
```

**Hosting:** Publish to GitHub Pages on each push to main, alongside the existing AGG demo gallery. URL like `larsbrubaker.github.io/agg-rust/svg-viewer/`.

### 6.4 Performance benchmarks

Separate from correctness. A small set of large SVGs (city maps, complex illustrations) timed via Criterion. Compare wall-clock render time against `resvg`/`tiny-skia` rendering the same files. We expect AGG to be in the same ballpark; if we're significantly slower, profile and optimize. Located in `benches/svg_render.rs`.

---

## 7. Open Questions for the Team

1. **Color management.** SVG 2 specifies linear-light blending for many operations. Do we need correct color-space handling now, or is sRGB-naive acceptable for v1? (resvg is sRGB-naive in many places too.)
2. **Text fallback policy.** Which fonts do we bundle? Do we surface "font not found" warnings to callers, or silently substitute?
3. **Filter scope for v1.** Is drop-shadow enough, or do we have known documents that use more exotic filters?
4. **Threading.** AGG renders single-threaded. For very large SVGs, do we want to tile the framebuffer and render tiles in parallel? (This is a sizable architectural decision; defer unless benchmarks show we need it.)

---

## 8. References

- `usvg` on crates.io: https://crates.io/crates/usvg
- `resvg` source (reference implementation): https://github.com/linebender/resvg
- `resvg-test-suite` (our test corpus): https://github.com/RazrFalcon/resvg-test-suite
- `resvg-test-suite` support table (example dashboard): https://razrfalcon.github.io/resvg-test-suite/svg-support-table.html
- AGG 2.6 documentation (C++): https://agg.sourceforge.net/antigrain.com/doc/index.html
- `agg-rust` interactive demos: https://larsbrubaker.github.io/agg-rust/
- `clipper2-rust`: https://github.com/larsbrubaker/clipper2-rust
