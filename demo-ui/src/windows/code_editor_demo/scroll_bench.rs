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
//! Run: `cargo test -p demo-ui code_editor_scroll_probe -- --nocapture`
//! Numbers come from whatever profile the test was built in — see the printed
//! `profile=` field (debug is what `cargo dev` users feel; deps are opt-level 2
//! but demo-ui / agg-gui own code is unoptimized in debug).

use std::cell::RefCell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

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

/// Build an `App` whose root is a `TextArea` configured exactly like the demo's
/// editor (same font, size, padding, highlighter) and seeded with `text`.
fn editor_app(text: &str) -> App {
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
        .with_edit_state(state)
        .with_highlighter(rust_highlighter);
    App::new(Box::new(editor))
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

#[test]
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
    eprintln!("  scroll (±1px height re-raster, cross-check) : {reraster_full_large_ms:8.3} ms");
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
        cold_ms.is_finite() && reraster_full_large_ms.is_finite(),
        "timings must be finite"
    );
}
