//! Headless timing probe for the Code Editor demo's scroll performance.
//!
//! Background: the Code Editor (see the parent `code_editor_demo.rs`) is a
//! `TextArea` with a monospace font, per-line Rust syntax highlighting, and an
//! LCD-subpixel CPU backbuffer. A Backend-panel reading showed ~70 ms/frame
//! while scrolling (target: <10 ms). This module is a MEASUREMENT harness only
//! — it changes no production code and asserts no timing threshold. It exists
//! to locate the bottleneck and to keep a reusable, CI-safe probe in the tree.
//!
//! It reuses the demo's real building blocks (`code_font`, `rust_highlighter`,
//! `EDITOR_FONT_SIZE`, `EDITOR_PADDING`, and the `SAMPLE` text the demo loads)
//! via `super::` so the numbers reflect the actual painting pipeline, not a
//! copy of it. The full frame is painted through the public `App` paint path
//! into a software `GfxCtx`, exactly the traversal the real app uses; the
//! `TextArea` internally allocates its `LcdBuffer` backbuffer on the way.
//!
//! Ignored in default test sweeps (slow). Run explicitly:
//! `cargo test -p demo-ui --release code_editor_scroll_probe -- --ignored --nocapture`
//! Numbers come from whatever profile the test was built in — see the printed
//! `profile=` field (debug is what `cargo dev` users feel; deps are opt-level 2
//! but demo-ui / agg-gui own code is unoptimized in debug).

use std::cell::RefCell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    measure_text_metrics, App, Framebuffer, GfxCtx, Size, TextArea, TextEditState, Widget,
};

use super::{code_font, rust_highlighter, EDITOR_FONT_SIZE, EDITOR_PADDING, SAMPLE};

/// Repeat the demo's `SAMPLE` until the document is `copies` paragraphs long,
/// producing a buffer far taller than the viewport so wheel scrolling actually
/// moves (the 14-line `SAMPLE` alone fits a tall window and never scrolls).
fn big_document(copies: usize) -> String {
    let mut s = String::new();
    for i in 0..copies {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(SAMPLE);
    }
    s
}

/// Build a `TextArea` configured exactly like the demo's editor (same font,
/// size, padding, highlighter) and seeded with `text`, returning both the bare
/// widget and the shared `TextEditState` handle. The bare widget is what the
/// strip-engagement probe drives through `paint_subtree` directly (App does not
/// expose a typed handle to its root, so the strip counters can only be read off
/// a `TextArea` we still own).
fn build_editor(text: &str) -> (TextArea, Rc<RefCell<TextEditState>>) {
    let code_font = code_font();
    let state = Rc::new(RefCell::new(TextEditState {
        text: text.to_string(),
        cursor: 0,
        anchor: 0,
        epoch: 0,
    }));
    let editor = TextArea::new(Arc::clone(&code_font))
        .with_font_size(EDITOR_FONT_SIZE)
        .with_padding(EDITOR_PADDING)
        .with_edit_state(Rc::clone(&state))
        .with_highlighter(rust_highlighter);
    (editor, state)
}

/// Build an `App` whose root is a `TextArea` configured exactly like the demo's
/// editor (same font, size, padding, highlighter) and seeded with `text`,
/// returning the shared `TextEditState` handle so a benchmark can drive edits.
fn editor_app_with_state(text: &str) -> (App, Rc<RefCell<TextEditState>>) {
    let (editor, state) = build_editor(text);
    (App::new(Box::new(editor)), state)
}

/// Apply one single-character edit through the shared `TextEditState` exactly as
/// the demo's KeyDown funnels do: insert `x` at the caret, advance the collapsed
/// caret past it, and bump the edit epoch. This is the mutation the felt edit
/// latency follows; the following `layout` + `paint` is what actually costs.
fn apply_edit(state: &Rc<RefCell<TextEditState>>) {
    let mut st = state.borrow_mut();
    let at = st.cursor.min(st.text.len());
    st.text.insert(at, 'x');
    st.cursor = at + 1;
    st.anchor = st.cursor;
    st.epoch = st.epoch.wrapping_add(1);
}

/// Build an `App` whose root is a `TextArea` configured exactly like the demo's
/// editor (same font, size, padding, highlighter) and seeded with `text`.
fn editor_app(text: &str) -> App {
    editor_app_with_state(text).0
}

/// Mean wall-clock time per closure invocation, in milliseconds, after
/// discarding `warmup` iterations.
fn mean_ms<F: FnMut()>(warmup: usize, iters: usize, mut f: F) -> f64 {
    for _ in 0..warmup {
        f();
    }
    let t = Instant::now();
    for _ in 0..iters {
        f();
    }
    t.elapsed().as_secs_f64() * 1000.0 / iters as f64
}

/// Paint one frame, forcing a backbuffer RE-RASTER by nudging the viewport
/// height ±1 px each call. That flips the `TextArea`'s cache signature
/// (`h_bits`), which is exactly what a scroll-offset change does — so this
/// isolates the per-frame re-raster cost without needing a scrollable document.
/// The wrap cache is unaffected (width + epoch unchanged), so only the paint /
/// rasterize / blit path is re-run.
fn reraster_frame(app: &mut App, fb: &mut Framebuffer, w: f64, base_h: f64, i: usize) {
    let h = base_h + (i % 2) as f64;
    app.layout(Size::new(w, h));
    let mut ctx = GfxCtx::new(fb);
    app.paint(&mut ctx);
}

/// True if any pixel in the framebuffer is non-zero (smoke check that painting
/// actually produced output).
fn any_painted(fb: &Framebuffer) -> bool {
    fb.pixels().iter().any(|&b| b != 0)
}

/// Ignored by default: this is a measurement probe, not a gate — it takes
/// ~17.5 minutes in a debug-profile `cargo test -p demo-ui` sweep and only
/// smoke-asserts. Run it explicitly for perf work:
/// `cargo test -p demo-ui --release code_editor_scroll_probe -- --ignored --nocapture`
#[test]
#[ignore = "slow measurement probe; run explicitly with --release ... -- --ignored --nocapture"]
fn code_editor_scroll_probe() {
    // ── Setup ────────────────────────────────────────────────────────────────
    let profile = if cfg!(debug_assertions) {
        "debug (cargo dev — own code unoptimized)"
    } else {
        "release"
    };
    let lcd = agg_gui::font_settings::lcd_enabled();

    let big = big_document(30); // ~420 lines
    let big_lines: Vec<&str> = big.split('\n').collect();
    let half = big_document(15); // ~210 lines
    let half_lines: Vec<&str> = half.split('\n').collect();
    let sample_lines: Vec<&str> = SAMPLE.split('\n').collect();

    let w = 1400.0_f64;
    let h_large = 1000.0_f64;
    let h_small = 600.0_f64;
    let font = code_font();

    // ── (c) Component isolation: highlighter alone ───────────────────────────
    let hl_big_ms = mean_ms(2, 20, || {
        for line in &big_lines {
            black_box(rust_highlighter(black_box(line)));
        }
    });
    let hl_per_line_us = hl_big_ms * 1000.0 / big_lines.len() as f64;

    // ── (c) Component isolation: text shaping / measurement ──────────────────
    // measure_text_metrics -> measure_advance -> shape_glyphs, which caches
    // rustybuzz output in a thread-local keyed by (font ptr, text, size). The
    // FIRST pass over these strings on this test thread is a cold cache; the
    // second is warm. This exposes what the shape cache already saves us.
    let shape_cold_ms = mean_ms(0, 1, || {
        for line in &big_lines {
            black_box(measure_text_metrics(&font, black_box(line), EDITOR_FONT_SIZE).width);
        }
    });
    let shape_warm_ms = mean_ms(2, 20, || {
        for line in &big_lines {
            black_box(measure_text_metrics(&font, black_box(line), EDITOR_FONT_SIZE).width);
        }
    });

    // ── (c) Component isolation: TextArea word-wrap / layout pass ─────────────
    // measure_min_height re-wraps the whole document from scratch every call
    // (it deliberately does not touch the layout cache), so this is a clean
    // read of the wrap cost for the full document at the editor's inner width.
    let inner_w = w - EDITOR_PADDING * 2.0;
    let wrap_probe = TextArea::new(Arc::clone(&font))
        .with_font_size(EDITOR_FONT_SIZE)
        .with_padding(EDITOR_PADDING)
        .with_text(big.clone());
    let wrap_ms = mean_ms(2, 20, || {
        black_box(wrap_probe.measure_min_height(black_box(inner_w)));
    });

    // ── (a) Whole-frame paint: cold vs warm ──────────────────────────────────
    // Cold = first paint of a fresh app (backbuffer must rasterize). Warm = a
    // repeat paint with nothing changed (backbuffer clean → blit only).
    let mut fb = Framebuffer::new(w as u32, (h_large + 1.0) as u32);

    let cold_ms = {
        let mut app = editor_app(&big);
        app.layout(Size::new(w, h_large));
        let t = Instant::now();
        {
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx);
        }
        t.elapsed().as_secs_f64() * 1000.0
    };
    let painted = any_painted(&fb);

    let warm_ms = {
        let mut app = editor_app(&big);
        app.layout(Size::new(w, h_large));
        {
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx); // prime the backbuffer cache
        }
        mean_ms(3, 30, || {
            // No layout call → sig unchanged → cache stays clean → blit only.
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx);
        })
    };

    // ── (b) Scroll frame the user feels: wheel event + layout + paint ─────────
    let scroll_wheel_ms = {
        let mut app = editor_app(&big);
        app.layout(Size::new(w, h_large));
        let (cx, cy) = (w * 0.5, h_large * 0.5);
        let mut i = 0usize;
        mean_ms(4, 40, || {
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            i += 1;
            app.on_mouse_wheel(cx, cy, dir);
            app.layout(Size::new(w, h_large));
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx);
        })
    };

    // ── (b') Single-character EDIT frame the user feels: edit + layout + paint ─
    // Drive a real edit through the shared `TextEditState` (insert one char at
    // the caret + advance the edit epoch) exactly as the KeyDown funnels do,
    // then time the following layout + paint. The KeyDown dispatch itself is
    // negligible; the felt latency is this re-wrap + re-raster, which is what we
    // measure. With the over-scan band a naive edit re-rasters the whole band
    // buffer; the dirty-line-strip path (if present) confines it to the edited
    // line strip, which is what should bring this near the warm blit floor.
    let edit_ms = {
        let (mut app, state) = editor_app_with_state(&big);
        app.layout(Size::new(w, h_large));
        {
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx); // prime the backbuffer cache
        }
        mean_ms(4, 40, || {
            apply_edit(&state);
            app.layout(Size::new(w, h_large));
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx);
        })
    };

    // ── EDIT DECOMPOSITION: split the felt edit frame into layout vs paint ─────
    // `edit_ms` above lumps `app.layout` and `app.paint` together. To locate the
    // ~58 ms excess over the warm blit floor, time the two calls separately from
    // the SAME primed loop (one edit per iteration, then layout, then paint), so
    // the split is over identical frames. Reported as means over 40 iters after
    // 4 warmup — same shape as `edit_ms`.
    let edit_iters = 40usize;
    let (edit_layout_ms, edit_paint_ms) = {
        let (mut app, state) = editor_app_with_state(&big);
        app.layout(Size::new(w, h_large));
        {
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx); // prime the backbuffer cache
        }
        for _ in 0..4 {
            apply_edit(&state);
            app.layout(Size::new(w, h_large));
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx);
        }
        let mut layout_acc = 0.0_f64;
        let mut paint_acc = 0.0_f64;
        for _ in 0..edit_iters {
            apply_edit(&state);
            let t = Instant::now();
            app.layout(Size::new(w, h_large));
            layout_acc += t.elapsed().as_secs_f64();
            let t = Instant::now();
            {
                let mut ctx = GfxCtx::new(&mut fb);
                app.paint(&mut ctx);
            }
            paint_acc += t.elapsed().as_secs_f64();
        }
        (
            layout_acc * 1000.0 / edit_iters as f64,
            paint_acc * 1000.0 / edit_iters as f64,
        )
    };

    // Control: edit + layout only (no paint). Cross-checks `edit_layout_ms` —
    // it should land near it (paint is what a control leaves out), confirming the
    // layout number isn't distorted by loop bookkeeping.
    let edit_nopaint_control_ms = {
        let (mut app, state) = editor_app_with_state(&big);
        app.layout(Size::new(w, h_large));
        {
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx); // prime the backbuffer cache
        }
        mean_ms(4, edit_iters, || {
            apply_edit(&state);
            app.layout(Size::new(w, h_large));
        })
    };

    // Strip engagement: verify EVERY edit frame took the dirty-line-strip path
    // (not a full band re-raster). The strip counters live on the `TextArea`, and
    // App exposes only a `&dyn Widget` root with no downcast — so this runs a bare
    // `TextArea` through the production `paint_subtree` path (the same path
    // `App::paint` funnels its root through) with the identical per-iteration work
    // as `edit_ms`: mutate the shared state, `layout`, `paint`. No `mark_dirty` —
    // exactly like `edit_ms`, which relies on `layout`'s sig change to invalidate.
    // Every timed iteration bumps `raster_count` (each strip re-raster still calls
    // `paint` once) AND `strip_raster_count`; so both deltas should equal
    // `edit_iters`. A strip delta below that means the probe's edit is NOT taking
    // the strip path (it fell back to a full band raster).
    // Also time the bare `TextArea` + `paint_subtree` loop (mean ms/iteration
    // over the same 40 timed edits). This isolates the TextArea+paint_subtree
    // cost from App-level extras at zero extra cost — the loop already runs the
    // exact per-iteration work `edit_ms` does, minus the `App` wrapper.
    let (edit_strip_delta, edit_raster_delta, edit_bare_paint_ms) = {
        let (mut editor, state) = build_editor(&big);
        editor.layout(Size::new(w, h_large));
        {
            let mut ctx = GfxCtx::new(&mut fb);
            paint_subtree(&mut editor, &mut ctx); // prime → 1 full raster, 0 strips
        }
        // Warm up the same 4 frames the timed loops discard.
        for _ in 0..4 {
            apply_edit(&state);
            editor.layout(Size::new(w, h_large));
            let mut ctx = GfxCtx::new(&mut fb);
            paint_subtree(&mut editor, &mut ctx);
        }
        let raster_before = editor.debug_raster_count();
        let strip_before = editor.debug_strip_raster_count();
        let mut bare_acc = 0.0_f64;
        for _ in 0..edit_iters {
            apply_edit(&state);
            editor.layout(Size::new(w, h_large));
            let t = Instant::now();
            {
                let mut ctx = GfxCtx::new(&mut fb);
                paint_subtree(&mut editor, &mut ctx);
            }
            bare_acc += t.elapsed().as_secs_f64();
        }
        (
            editor.debug_strip_raster_count() - strip_before,
            editor.debug_raster_count() - raster_before,
            bare_acc * 1000.0 / edit_iters as f64,
        )
    };

    // Re-raster frame via ±1px height (same cost driver as a scroll: full
    // backbuffer re-raster). Cross-check against the wheel number above.
    let reraster_full_large_ms = {
        let mut app = editor_app(&big);
        let mut i = 0usize;
        mean_ms(4, 40, || {
            reraster_frame(&mut app, &mut fb, w, h_large, i);
            i += 1;
        })
    };

    // ── (c) LCD backbuffer machinery overhead (no glyphs) ────────────────────
    // Empty document: re-raster still allocates + clears a viewport-sized
    // LcdBuffer, paints the bg + border, flips the colour/alpha planes and
    // blits. This is the fixed per-frame cost independent of text volume.
    let reraster_empty_large_ms = {
        let mut app = editor_app("");
        let mut i = 0usize;
        mean_ms(4, 40, || {
            reraster_frame(&mut app, &mut fb, w, h_large, i);
            i += 1;
        })
    };

    // ── (3) Scaling: DOCUMENT size (half vs full) at a fixed viewport ─────────
    let reraster_half_large_ms = {
        let mut app = editor_app(&half);
        let mut i = 0usize;
        mean_ms(4, 40, || {
            reraster_frame(&mut app, &mut fb, w, h_large, i);
            i += 1;
        })
    };

    // ── (3) Scaling: VIEWPORT size (small vs large) at a fixed document ───────
    let mut fb_small = Framebuffer::new(w as u32, (h_small + 1.0) as u32);
    let reraster_full_small_ms = {
        let mut app = editor_app(&big);
        let mut i = 0usize;
        mean_ms(4, 40, || {
            reraster_frame(&mut app, &mut fb_small, w, h_small, i);
            i += 1;
        })
    };

    // ── Reference: the REAL demo content (14-line SAMPLE) ─────────────────────
    let reraster_sample_large_ms = {
        let mut app = editor_app(SAMPLE);
        let mut i = 0usize;
        mean_ms(4, 40, || {
            reraster_frame(&mut app, &mut fb, w, h_large, i);
            i += 1;
        })
    };

    // ── Report ───────────────────────────────────────────────────────────────
    eprintln!("\n================ Code Editor scroll timing probe ================");
    eprintln!("profile      : {profile}");
    eprintln!("lcd_enabled  : {lcd}  (LcdCoverage backbuffer when true)");
    eprintln!(
        "viewport     : {}x{} logical (large), {}x{} (small)",
        w as u32, h_large as u32, w as u32, h_small as u32
    );
    eprintln!(
        "documents    : full={} lines, half={} lines, SAMPLE={} lines",
        big_lines.len(),
        half_lines.len(),
        sample_lines.len()
    );
    eprintln!("-----------------------------------------------------------------");
    eprintln!("WHOLE FRAME (App::paint, full doc, large viewport)");
    eprintln!("  cold  (first paint, backbuffer raster)      : {cold_ms:8.3} ms");
    eprintln!("  warm  (no change, cache blit only)          : {warm_ms:8.3} ms");
    eprintln!("  scroll (wheel + layout + paint)             : {scroll_wheel_ms:8.3} ms  <- user-felt");
    eprintln!("  edit   (1 char + layout + paint)            : {edit_ms:8.3} ms  <- user-felt");
    eprintln!("  scroll (±1px height re-raster, cross-check) : {reraster_full_large_ms:8.3} ms");
    eprintln!("-----------------------------------------------------------------");
    eprintln!("EDIT DECOMPOSITION (1 char/frame, full doc, large viewport)");
    eprintln!("  edit: app.layout only                       : {edit_layout_ms:8.3} ms");
    eprintln!("  edit: app.paint only                        : {edit_paint_ms:8.3} ms");
    eprintln!("  edit: layout+paint (sum cross-check)        : {:8.3} ms", edit_layout_ms + edit_paint_ms);
    eprintln!("  edit: edit+layout, NO paint (control)       : {edit_nopaint_control_ms:8.3} ms");
    eprintln!("  edit: bare paint_subtree loop               : {edit_bare_paint_ms:8.3} ms");
    eprintln!(
        "  strip engagement (over {edit_iters} timed edits)     : strip_delta={edit_strip_delta}, raster_delta={edit_raster_delta}"
    );
    if edit_strip_delta < edit_iters as u64 {
        eprintln!(
            "  *** WARNING: strip_delta {edit_strip_delta} < {edit_iters} iters — the strip path is NOT"
        );
        eprintln!("      fully engaging; the probe's edit is falling back to a full band raster.");
    } else {
        eprintln!("  (strip path engaged on every timed edit, as expected)");
    }
    eprintln!("-----------------------------------------------------------------");
    eprintln!("COMPONENTS (full doc)");
    eprintln!(
        "  highlighter over all lines                  : {hl_big_ms:8.3} ms  ({hl_per_line_us:.2} us/line)"
    );
    eprintln!("  shaping/measure all lines (cold cache)      : {shape_cold_ms:8.3} ms");
    eprintln!("  shaping/measure all lines (warm cache)      : {shape_warm_ms:8.3} ms");
    eprintln!("  word-wrap whole doc (measure_min_height)    : {wrap_ms:8.3} ms");
    eprintln!("  LCD backbuffer machinery (empty doc raster) : {reraster_empty_large_ms:8.3} ms");
    eprintln!("-----------------------------------------------------------------");
    eprintln!("SCALING (per re-raster frame)");
    eprintln!("  DOCUMENT: half doc,  large vp               : {reraster_half_large_ms:8.3} ms");
    eprintln!("  DOCUMENT: full doc,  large vp               : {reraster_full_large_ms:8.3} ms");
    eprintln!("  VIEWPORT: full doc,  small vp               : {reraster_full_small_ms:8.3} ms");
    eprintln!("  VIEWPORT: full doc,  large vp               : {reraster_full_large_ms:8.3} ms");
    eprintln!("  REFERENCE: real SAMPLE (14 lines), large vp : {reraster_sample_large_ms:8.3} ms");
    eprintln!("=================================================================\n");

    // ── Smoke assertion (CI gate, NOT a perf gate) ───────────────────────────
    assert!(painted, "App::paint must produce non-empty output");
    assert!(
        cold_ms.is_finite() && reraster_full_large_ms.is_finite() && edit_ms.is_finite(),
        "timings must be finite"
    );
}
