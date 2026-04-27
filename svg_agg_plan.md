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

## 3. Current Status

Done, briefly:

- Library-owned SVG walker lives in `agg-gui` and renders only through `DrawCtx`.
- RGBA, LCD coverage, and hardware demo targets share the same SVG render path.
- Implemented solid fills, fill rules, transforms, cubic/quadratic paths, strokes, line caps/joins, miter limits, dashes, opacity, embedded raster images, and explicit reference-size rendering.
- Started bridge-level linear/radial gradient fills and gradient strokes for RGBA, LCD, and hardware targets. The hardware path uses native shader ramps so the same SVG paint model reaches every active backend.
- Added bridge-level sampled pattern fills/strokes for RGBA and LCD targets; pattern corpus cases now smoke-render, while pixel-exact pattern tiling/viewBox/object-bounding-box work remains.
- Added `resvg-test-suite` as the reference corpus and use its paired PNGs in tests and demos.
- Added an opt-in data-driven SVG regression harness that discovers paired SVG/PNG cases, supports filters/shards/limits, and writes grouped JSON reports.
- Added render-only corpus mode (`AGG_GUI_SVG_RENDER_ONLY=1`): current smoke coverage is `1676 / 1679` cases rendering successfully; the 3 remaining render failures are invalid-size/invalid-encoding SVG inputs.
- SVG Test shows four fixed columns: `reference.png`, `agg-rgba-bitmap render`, `agg-lcd-bitmap render`, and `hardware render`.
- SVG Test supports fixed headers, bidirectional scrolling, default 50% zoom, 50%/100%/Custom zoom controls, and Ctrl+wheel zoom around the mouse position.
- LCD demo display now preserves and blits the LCD color/alpha planes instead of collapsing them to RGBA.

Current SVG Test rows are a 54-row curated smoke set covering broad capability boundaries:

- Basic shapes, path command variants, solid color parsing, fill rules, opacity, strokes, gradients, patterns, and embedded images.
- Keep this list around 40-50 differentiated examples long. Add rows for visual coverage, not as a replacement for the full regression harness.

---

## 4. Remaining Implementation Plan

Keep landing features through the bridge first, then add focused regression tests and one or more curated SVG Test rows only when the feature adds meaningfully different visual capability.

### Completed Foundation

Keep covered by tests; do not add more demo rows unless they show a visually distinct capability:

- Solid fills and paths.
- Fill rules.
- Transforms and root SVG Y-down to `agg-gui` Y-up mapping.
- Strokes, caps, joins, miter limits, and dashes.
- Opacity.
- Embedded raster images.
- Explicit output sizing to match reference PNG resolution.

---

### Next: Gradients and Patterns

Tasks:

1. **Linear gradients:** Filled-path and stroked-path support is in place for RGBA, LCD, and hardware via bridge gradient paint. Remaining work: object-bounding-box regression cases and deeper gradient transform coverage against the reference suite.
2. **Radial gradients:** Filled-path and stroked-path support is in place for RGBA, LCD, and hardware via bridge radial/focal gradient paint. Remaining work: deeper reference-suite diff coverage.
3. **Spread modes:** Pad / Reflect / Repeat — represent these at the bridge paint level and implement them per target.
4. **Patterns:** Basic sampled pattern paint is in place for RGBA/LCD. Remaining work: exact `viewBox`, `patternUnits`, `patternContentUnits`, object-bounding-box sizing, and hardware texture-pattern support.

Acceptance: representative `resvg-test-suite/tests/paint-servers/linearGradient`, `radialGradient`, and `pattern` cases pass through RGBA/LCD/hardware targets.

---

### Clipping and Masking

This is where we lean on `clipper2-rust`.

Tasks:

1. **Simple clip paths (single shape):** Express clip state as a bridge primitive. Software implementations may rasterize into `alpha_mask_u8` and render through `PixfmtAmaskAdaptor`; hardware may use stencil/scissor/texture masks.
2. **Compound clip paths (multiple shapes intersected):** Use `clipper2-rust`'s `intersect_64` to compute the polygon intersection, then submit the result through the bridge clip-mask primitive.
3. **Nested clipping:** Push/pop a clip stack. Each push intersects the new clip with the current effective clip via `clipper2-rust`, then updates bridge clip state.
4. **Masks (`<mask>`):** Render the mask content through the bridge into a grayscale/luminance mask target, then apply that mask through the bridge mask primitive.

Acceptance: representative `resvg-test-suite/tests/masking/clipPath` and `mask` cases pass.

---

### Text

Recommended approach: enable `usvg`'s built-in text-to-path conversion. Text becomes outlined paths and flows through the existing path renderer with no additional code.

Tasks:

1. Configure `usvg::Options` with a populated `fontdb`. Bundle a fallback font (DejaVu Sans or Noto Sans) for missing-font cases.
2. Call `usvg::Tree::postprocess()` (or the equivalent in the version we pin) to convert all `<text>` to paths before walking.
3. Verify font fallback behavior on systems without the requested fonts.

Acceptance: representative `resvg-test-suite/tests/text` cases render with correct glyphs and positioning.

---

### Images and `<use>`

Tasks:

1. **Raster images:** done for embedded images; continue with external images and `image-rendering` interpolation.
2. **Nested SVG images:** `usvg` flattens these into the main tree, so no special handling is usually needed.
3. **`<use>` elements:** Already expanded by `usvg`. No work required.

Acceptance: embedded and external image cases render with correct positioning, scaling, and interpolation quality.

---

### Filters

This is the largest feature surface and lowest priority. `usvg` resolves the filter graph but does not execute it.

Strategy: implement filters **on demand**, starting with the most common:

1. `feGaussianBlur` — AGG's stack blur is a fast, visually acceptable approximation. True Gaussian if needed.
2. `feColorMatrix` — straightforward per-pixel transform.
3. `feOffset`, `feFlood`, `feComposite`, `feMerge` — needed for drop shadows, which account for the majority of real-world filter usage.
4. Everything else — defer until a real document needs it.

Acceptance: drop-shadow filter chains render correctly. Other filters degrade gracefully until implemented.

---

## 5. Mapping Reference

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

## 6. Coordinate System and Numeric Type Notes

- The bridge and widget system use **Y-up** coordinates: origin at bottom-left, positive Y upward. SVG uses Y-down document coordinates. The SVG renderer owns this conversion at its root transform, so backends do not special-case SVG orientation.
- AGG 2.6 (and our port) uses `f64` throughout. `usvg` returns coordinates as `f32` (it builds on `tiny-skia-path`). Cast at the bridge boundary; no precision concerns for typical SVG content.
- `usvg` resolves the SVG `viewBox` and outer transform into the root node's transform. Compose that with the SVG-to-Y-up transform for the chosen viewport before emitting bridge calls.
- Keep target dimensions in physical pixels at the target factory boundary. The SVG walker should operate in logical SVG/user units and let the target context's transform map those units to the backing store.

---

## 7. Testing Strategy

We use a single source of test SVGs — the **`resvg` test suite** — for both automated regression testing and a developer-facing visual demo. One corpus, two consumers. This keeps maintenance low and ensures the demo always reflects what CI is actually testing.

### 7.1 Test corpus: `resvg-test-suite`

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
├── tests/            ← SVG files and paired PNG references, organized by feature
├── images/           ← raster assets referenced by SVGs
└── fonts/            ← bundled fonts for text tests
```

**Vendoring:** Add `resvg-test-suite` as a git submodule under `tests/resvg-test-suite/`. Pin to a specific commit hash so reference renderings don't drift under us. Update the pin deliberately when we want to pull in new tests.

### 7.2 Integration tests (CI)

**Location:** `agg-gui/tests/svg_regression.rs`

**Mechanism:** A single opt-in integration test walks `tests/resvg-test-suite/tests/`, imports the SVG renderer from the `agg-gui` library crate, renders each SVG through that shared library API into available targets, and diffs against the paired PNG next to the SVG. The first landed harness covers RGBA report generation; LCD and headless/recorded hardware reporting should be layered in after the report format stabilizes.

The harness is environment-driven so normal `cargo test` stays fast:

- `AGG_GUI_SVG_REGRESSION=1` enables the suite.
- `AGG_GUI_SVG_FILTER=<substring>` runs a feature/path subset.
- `AGG_GUI_SVG_LIMIT=<n>` caps the number of cases for smoke runs.
- `AGG_GUI_SVG_SHARD=<index>/<count>` splits the suite across jobs/agents.
- `AGG_GUI_SVG_RENDER_ONLY=1` checks parse/render success without pixel diffing.
- `AGG_GUI_SVG_STRICT=1` makes failures fail the test; omit it for report-only triage.
- `AGG_GUI_SVG_REPORT=<path>` overrides the JSON report path.
- `AGG_GUI_SVG_KNOWN_DIFFS=<path>` overrides the known-diffs policy file. The default is `tests/svg_known_diffs.txt`.

Strict mode still passes accepted known diffs, but keeps them visible in the report under `known_failures`. Each policy entry is specific and threshold-bounded so unexpected regressions remain failures.

```rust
// agg-gui/tests/svg_regression.rs
use std::path::PathBuf;

#[test]
fn resvg_test_suite() {
    let suite_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resvg-test-suite");
    let tests_dir = suite_root.join("tests");

    let mut failures = Vec::new();
    let mut total = 0;

    for entry in walk_svg_files(&tests_dir) {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        total += 1;

        let rel = entry.path().strip_prefix(&tests_dir).unwrap();
        let expected_png = entry.path().with_extension("png");
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

### 7.3 Visual demo rows

The in-app `SVG Test` window is a curated capability gallery, not a dump of every suite case. It should show roughly **40-50 attractive, differentiated rows** selected from the ~1,500 `resvg-test-suite` examples.

Each row must demonstrate a meaningfully distinct renderer capability. Avoid adding near-duplicates just because a test exists. The full suite belongs in automated regression tests; the demo rows are for fast visual understanding of progress and breadth.

Candidate row categories as features land:

- Basic fills and simple paths.
- Fill rules and self-intersections.
- Transforms and viewBox scaling.
- Stroke joins, caps, miter limits, and dashes.
- Group opacity and paint opacity.
- Embedded and external raster images.
- Linear gradients, radial gradients, spread modes, and gradient transforms.
- Patterns.
- Clip paths and masks.
- Text converted to paths, including fallback fonts and positioning.
- Filters, starting with drop shadows.
- Stress/complex illustration cases once performance work begins.

**Layout:** when the demo window is open it always shows exactly four render columns in this order:

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

**Four columns per row:**
1. **`reference.png`** — the PNG from `resvg-test-suite/png/`, displayed as-is.
2. **`agg-rgba-bitmap render`** — the SVG rendered through the shared walker into the RGBA bitmap target.
3. **`agg-lcd-bitmap render`** — the same SVG rendered through the shared walker into the LCD coverage bitmap target, then composited for display.
4. **`hardware render`** — the same SVG rendered through the shared walker into the hardware target.

Diff overlays, when added, are per target and compare each rendered output to `reference.png`. They are controls/overlays on the corresponding render column, not a replacement for any of the four required columns.

**Viewer controls:**
- Fixed column headers.
- Bidirectional scrolling.
- Default 50% zoom.
- `50%`, `100%`, and `Custom` zoom state buttons.
- Ctrl+wheel zooms around the mouse position while normal wheel scrolling still scrolls.

### 7.4 Performance benchmarks

Separate from correctness. A small set of large SVGs (city maps, complex illustrations) timed via Criterion. Compare wall-clock render time against `resvg`/`tiny-skia` rendering the same files. We expect AGG to be in the same ballpark; if we're significantly slower, profile and optimize. Located in `benches/svg_render.rs`.

---

## 8. Open Questions for the Team

1. **Color management.** SVG 2 specifies linear-light blending for many operations. Do we need correct color-space handling now, or is sRGB-naive acceptable for v1? (resvg is sRGB-naive in many places too.)
2. **Text fallback policy.** Which fonts do we bundle? Do we surface "font not found" warnings to callers, or silently substitute?
3. **Filter scope for v1.** Is drop-shadow enough, or do we have known documents that use more exotic filters?
4. **Threading.** AGG renders single-threaded. For very large SVGs, do we want to tile the framebuffer and render tiles in parallel? (This is a sizable architectural decision; defer unless benchmarks show we need it.)

---

## 9. References

- `usvg` on crates.io: https://crates.io/crates/usvg
- `resvg` source (reference implementation): https://github.com/linebender/resvg
- `resvg-test-suite` (our test corpus): https://github.com/RazrFalcon/resvg-test-suite
- `resvg-test-suite` support table (example dashboard): https://razrfalcon.github.io/resvg-test-suite/svg-support-table.html
- AGG 2.6 documentation (C++): https://agg.sourceforge.net/antigrain.com/doc/index.html
- `agg-rust` interactive demos: https://larsbrubaker.github.io/agg-rust/
- `clipper2-rust`: https://github.com/larsbrubaker/clipper2-rust
