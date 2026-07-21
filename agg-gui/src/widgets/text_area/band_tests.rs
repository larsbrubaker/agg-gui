//! Integration tests for [`TextArea`]'s over-scan band backbuffer and its
//! dirty-line-strip partial re-raster, exercised against the real widget
//! through the production `paint_subtree` path.
//!
//! They pin the behaviours the band + strip model promise:
//!   * scrolling *within* the band re-blits (no re-raster);
//!   * scrolling *out of* the band re-anchors and re-rasters exactly once;
//!   * an in-place edit re-rasters ONLY the changed line strip;
//!   * a strip re-raster reproduces a full re-raster pixel-for-pixel;
//! plus the mandatory guard-rail: the over-scan margins are clipped to the
//! widget bounds, so neither the band blit nor a strip re-raster ever touches
//! an adjacent sibling.
//!
//! ## Epoch-stable measurement
//!
//! The framework re-rasters any cached widget when the process-wide theme or
//! typography epoch changes (a dark/light flip, an LCD toggle). Those epochs are
//! global atomics, so a *parallel* unit test that flips the palette legitimately
//! invalidates this test's cache mid-measurement. Every raster-count assertion
//! therefore runs inside [`stable_window`], which repeats the measurement until
//! it lands in a window with no external epoch churn (single-threaded runs never
//! retry). This isolates "did scrolling/editing re-raster?" from "did a sibling
//! test flip the theme?" without weakening the invariant.

use std::sync::Arc;

use super::*;
use crate::color::Color;
use crate::font_settings::current_typography_epoch;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::theme::current_visuals_epoch;
use crate::widget::{paint_subtree, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// A document far taller than the test viewport so the band is active.
fn tall_doc() -> String {
    (0..200)
        .map(|i| format!("line number {i} with some content"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lay out to a small viewport (so content overflows and the band engages),
/// then paint once through the backbuffered path into `fb`.
fn layout_and_paint(ta: &mut TextArea, fb: &mut Framebuffer, w: f64, h: f64) {
    ta.layout(Size::new(w, h));
    let mut ctx = GfxCtx::new(fb);
    paint_subtree(ta, &mut ctx);
}

/// Run `attempt` in a window with no external theme / typography epoch change,
/// so a parallel palette-flipping test can't be mis-attributed as a scroll- or
/// edit-driven re-raster. `attempt` must rebuild any widget it measures from
/// scratch so each retry is independent. Returns the first churn-free result.
fn stable_window<T>(mut attempt: impl FnMut() -> T) -> T {
    for _ in 0..32 {
        let before = (current_visuals_epoch(), current_typography_epoch());
        let out = attempt();
        if before == (current_visuals_epoch(), current_typography_epoch()) {
            return out;
        }
    }
    // A suite this persistently churny is itself a bug; run once more so the
    // caller's assertion still executes rather than silently passing.
    attempt()
}

/// Deliver a wheel event exactly the way `dispatch_event` does for a root
/// widget: after the handler runs, if it consumed loudly OR bumped the
/// invalidation epoch, the dispatcher calls `mark_dirty()` on the widget. This
/// is the conservative auto-invalidation that would defeat the band unless the
/// widget's sig is authoritative — so the test must reproduce it, not bypass it.
fn dispatch_wheel(ta: &mut TextArea, delta_y: f64) {
    use crate::event::{Event, Modifiers};
    use crate::geometry::Point;
    let before = crate::animation::invalidation_epoch();
    let result = ta.on_event(&Event::MouseWheel {
        pos: Point::new(100.0, 75.0),
        delta_y,
        delta_x: 0.0,
        modifiers: Modifiers::default(),
    });
    if result.requests_redraw() || before != crate::animation::invalidation_epoch() {
        Widget::mark_dirty(ta);
    }
}

#[test]
fn scrolling_within_the_band_does_not_reraster() {
    let (delta, offset) = stable_window(|| {
        let mut ta = TextArea::new(font()).with_text(tall_doc());
        let mut fb = Framebuffer::new(200, 150);
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        assert!(
            ta.band.active,
            "a tall doc in a short viewport must engage the band"
        );
        let after_first = ta.debug_raster_count();
        assert_eq!(after_first, 1, "the first paint rasters once");

        // One 3-line wheel notch — well inside the half-viewport over-scan margin.
        assert!(ta.scroll_by_wheel(-1.0), "a notch should move the offset");
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        (ta.debug_raster_count() - after_first, ta.scroll_offset())
    });
    assert_eq!(delta, 0, "scrolling within the band must re-blit, not re-raster");
    assert!(offset > 0.0, "the offset actually advanced");
}

#[test]
fn wheel_event_oscillation_within_band_does_not_reraster() {
    let delta = stable_window(|| {
        let mut ta = TextArea::new(font()).with_text(tall_doc());
        let mut fb = Framebuffer::new(200, 150);
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        let baseline = ta.debug_raster_count();

        // Mimic the demo's wheel loop through the full dispatch path (which marks
        // the widget dirty on every consumed scroll). The band + authoritative
        // sig must still re-blit, not re-raster, for oscillation inside the band.
        for i in 0..12 {
            let delta_y = if i % 2 == 0 { -1.0 } else { 1.0 };
            dispatch_wheel(&mut ta, delta_y);
            layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        }
        ta.debug_raster_count() - baseline
    });
    assert_eq!(
        delta, 0,
        "oscillating wheel scrolling within the band must not re-raster"
    );
}

#[test]
fn harness_scale_wheel_oscillation_stays_in_band() {
    // Mirror the demo scroll probe: large viewport, tall doc, alternate one
    // notch up/down. Proves the band engages at 1400x1000 (raster count stable)
    // through the full dispatch path.
    let delta = stable_window(|| {
        let doc = (0..400)
            .map(|i| format!("line {i} with a reasonable amount of code-like content here"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut ta = TextArea::new(font()).with_font_size(13.0).with_text(doc);
        let mut fb = Framebuffer::new(1400, 1001);
        layout_and_paint(&mut ta, &mut fb, 1400.0, 1000.0);
        assert!(ta.band.active);
        let baseline = ta.debug_raster_count();
        for i in 0..20 {
            let delta_y = if i % 2 == 0 { -1.0 } else { 1.0 };
            dispatch_wheel(&mut ta, delta_y);
            layout_and_paint(&mut ta, &mut fb, 1400.0, 1000.0);
        }
        ta.debug_raster_count() - baseline
    });
    assert_eq!(
        delta, 0,
        "harness-scale oscillation must stay in band (no re-raster)"
    );
}

#[test]
fn leaving_the_band_reanchors_and_rerasters_once() {
    let (count_after_jump, count_after_idle, anchor_err) = stable_window(|| {
        let mut ta = TextArea::new(font()).with_text(tall_doc());
        let mut fb = Framebuffer::new(200, 150);
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        assert_eq!(ta.debug_raster_count(), 1);
        let anchor0 = ta.band.anchor;

        // Jump far past the over-scan margin so the offset leaves the band.
        let far = anchor0 + ta.band.over_bottom + 500.0;
        ta.vbar.offset = far;
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        let after_jump = ta.debug_raster_count();
        let anchor_err = (ta.band.anchor - ta.scroll_offset()).abs();

        // Painting again without moving must not re-raster.
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        (after_jump, ta.debug_raster_count(), anchor_err)
    });
    assert_eq!(
        count_after_jump, 2,
        "leaving the band must re-anchor and re-raster exactly once"
    );
    assert!(anchor_err < 1.0, "the band re-anchored around the new offset");
    assert_eq!(count_after_idle, 2, "an unchanged frame re-blits only");
}

#[test]
fn editing_still_invalidates_the_cache() {
    let delta = stable_window(|| {
        let mut ta = TextArea::new(font()).with_text(tall_doc());
        let mut fb = Framebuffer::new(200, 150);
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        let before = ta.debug_raster_count();

        // Mutate the shared edit state as a real edit would (advance the epoch),
        // then mark the wrap cache dirty like the edit funnels do.
        {
            let mut st = ta.edit.borrow_mut();
            st.text.insert_str(0, "edited ");
            st.cursor = 7;
            st.anchor = 7;
            st.epoch = st.epoch.wrapping_add(1);
        }
        ta.mark_dirty();
        layout_and_paint(&mut ta, &mut fb, 200.0, 150.0);
        ta.debug_raster_count() - before
    });
    assert_eq!(delta, 1, "an edit must re-raster the band");
}

/// Insert one character on the top (visible) line of a scrollable editor at
/// offset 0, driving the edit the way the funnels do (advance epoch + move a
/// collapsed caret). Shared by the strip-count and equivalence tests.
fn insert_char_on_top_line(ta: &mut TextArea) {
    let mut st = ta.edit.borrow_mut();
    st.text.insert(0, 'X');
    st.cursor = 1;
    st.anchor = 1;
    st.epoch = st.epoch.wrapping_add(1);
    drop(st);
    ta.mark_dirty();
}

#[test]
fn single_line_edit_rerasters_only_the_strip() {
    let (rasters, strips) = stable_window(|| {
        let mut ta = TextArea::new(font()).with_font_size(13.0).with_text(tall_doc());
        let mut fb = Framebuffer::new(400, 300);
        // Keep the view pinned at the top so the edited line 0 is on-screen.
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);
        assert!(ta.band.active, "the band must be active for a strip edit");
        let strips_before = ta.debug_strip_raster_count();
        assert_eq!(strips_before, 0, "priming is a full raster, not a strip");

        insert_char_on_top_line(&mut ta);
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);
        (ta.debug_raster_count(), ta.debug_strip_raster_count())
    });
    assert_eq!(rasters, 2, "prime + edit each reached paint once");
    assert_eq!(
        strips, 1,
        "a single-line edit must re-raster the strip, not the whole band"
    );
}

#[test]
fn reanchor_is_a_full_raster_not_a_strip() {
    let strips = stable_window(|| {
        let mut ta = TextArea::new(font()).with_font_size(13.0).with_text(tall_doc());
        let mut fb = Framebuffer::new(400, 300);
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);
        // Jump out of the band: the re-anchor must NOT be treated as a strip.
        ta.vbar.offset = ta.band.anchor + ta.band.over_bottom + 800.0;
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);
        ta.debug_strip_raster_count()
    });
    assert_eq!(strips, 0, "a re-anchor re-rasters the whole band, not a strip");
}

#[test]
fn strip_reraster_matches_a_full_reraster_pixel_for_pixel() {
    // A strip re-raster into the retained buffer must reconstruct exactly what a
    // fresh full raster of the same post-edit document produces.
    let (strip_pixels, full_pixels, did_strip) = stable_window(|| {
        let (w, h) = (400.0_f64, 300.0_f64);

        // Editor A: prime with the original text, then edit line 0 → strip path.
        // Prime into a throwaway framebuffer, then paint the measured (post-edit)
        // frame onto a FRESH zeroed framebuffer, so the comparison sees a single
        // blit + overlay pass — matching editor B and isolating the band buffer
        // content (the thing the strip re-raster must reconstruct) from
        // AA accumulation of repeated overlay compositing.
        let mut a = TextArea::new(font()).with_font_size(13.0).with_text(tall_doc());
        let mut fb_prime = Framebuffer::new(w as u32, h as u32);
        layout_and_paint(&mut a, &mut fb_prime, w, h);
        insert_char_on_top_line(&mut a);
        let mut fb_a = Framebuffer::new(w as u32, h as u32);
        layout_and_paint(&mut a, &mut fb_a, w, h);
        let did_strip = a.debug_strip_raster_count() == 1;

        // Editor B: built directly from the post-edit text → full raster only.
        let mut post = tall_doc();
        post.insert(0, 'X');
        let mut b = TextArea::new(font()).with_font_size(13.0).with_text(post);
        // Match A's caret so any (cache-external) state agrees; both sit at
        // offset 0 with the band anchored at 0, so geometry is identical.
        {
            let mut st = b.edit.borrow_mut();
            st.cursor = 1;
            st.anchor = 1;
        }
        let mut fb_b = Framebuffer::new(w as u32, h as u32);
        layout_and_paint(&mut b, &mut fb_b, w, h);

        (fb_a.pixels().to_vec(), fb_b.pixels().to_vec(), did_strip)
    });
    assert!(did_strip, "editor A must have taken the strip path");
    let w = 400u32;
    let mut coords = Vec::new();
    for p in 0..(strip_pixels.len() / 4) {
        let i = p * 4;
        if strip_pixels[i..i + 4] != full_pixels[i..i + 4] {
            coords.push((p as u32 % w, p as u32 / w));
        }
    }
    assert!(
        coords.is_empty(),
        "strip re-raster must equal a full re-raster pixel-for-pixel; {} px differ at {:?}",
        coords.len(),
        &coords[..coords.len().min(20)]
    );
}

#[test]
fn band_blit_is_clipped_to_bounds_and_spares_siblings() {
    // Paint the editor into a framebuffer with a sentinel-filled region BELOW
    // its bounds (where a sibling widget would live). The band's bottom
    // over-scan margin extends into that region inside the buffer; the framework
    // must clip the blit to the widget bounds so the sentinel survives.
    const SENTINEL: Color = Color::rgb(1.0, 0.0, 0.0);
    let widget_y = 100.0_f64; // 100 px of sentinel below the widget
    let (w, h) = (200.0_f64, 150.0_f64);

    let mut ta = TextArea::new(font()).with_text(tall_doc());
    ta.layout(Size::new(w, h));
    assert!(
        ta.band.active,
        "band must be active for this test to be meaningful"
    );
    assert!(
        ta.band.over_bottom > 40.0,
        "the bottom over-scan must reach into the sentinel region to test the clip"
    );

    let mut fb = Framebuffer::new(240, 320);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(SENTINEL);
        ctx.translate(20.0, widget_y);
        paint_subtree(&mut ta, &mut ctx);
    }

    let pixel = |x: u32, y: u32| -> (u8, u8, u8) {
        let i = ((y * fb.width() + x) * 4) as usize;
        let p = fb.pixels();
        (p[i], p[i + 1], p[i + 2])
    };
    // Below the widget bounds but inside the bottom over-scan band: must be
    // untouched sentinel (the over-scan was clipped away).
    assert_eq!(
        pixel(120, 70),
        (255, 0, 0),
        "band over-scan leaked over the region below the widget"
    );
    // Inside the widget bounds: the editor painted its opaque bg, so NOT the
    // sentinel — proves the blit happened and the check above is meaningful.
    assert_ne!(
        pixel(120, 170),
        (255, 0, 0),
        "the editor should have painted inside its own bounds"
    );
}

#[test]
fn strip_reraster_spares_a_sibling_below() {
    // A dirty-strip edit paints into the retained band buffer and re-blits it;
    // the blit is still clipped to the widget bounds, so a sibling below the
    // editor keeps its sentinel even across an edit (not just the first paint).
    const SENTINEL: Color = Color::rgb(0.0, 1.0, 0.0);
    let (w, h) = (200.0_f64, 150.0_f64);
    let widget_y = 100.0_f64;

    let mut ta = TextArea::new(font()).with_text(tall_doc());
    let mut fb = Framebuffer::new(240, 320);
    let paint_into = |ta: &mut TextArea, fb: &mut Framebuffer| {
        let mut ctx = GfxCtx::new(fb);
        ctx.clear(SENTINEL);
        ctx.translate(20.0, widget_y);
        paint_subtree(ta, &mut ctx);
    };
    ta.layout(Size::new(w, h));
    assert!(ta.band.active);
    paint_into(&mut ta, &mut fb);

    // Edit line 0, forcing a strip re-raster, then re-blit over the sentinel.
    insert_char_on_top_line(&mut ta);
    ta.layout(Size::new(w, h));
    paint_into(&mut ta, &mut fb);

    let pixel = |x: u32, y: u32| -> (u8, u8, u8) {
        let i = ((y * fb.width() + x) * 4) as usize;
        let p = fb.pixels();
        (p[i], p[i + 1], p[i + 2])
    };
    assert_eq!(
        pixel(120, 70),
        (0, 255, 0),
        "a strip re-raster's blit leaked over the region below the widget"
    );
}

#[test]
fn strip_edit_updates_published_planes_in_place() {
    // A dirty-strip edit must mutate the retained top-down planes through
    // `Arc::make_mut` (buffer address stable) rather than allocating fresh Arcs,
    // and must still bump `content_version` so GPU backends detect the change
    // despite the pointer being unchanged.
    let (same_color, same_alpha, version_grew, strips) = stable_window(|| {
        let mut ta = TextArea::new(font()).with_font_size(13.0).with_text(tall_doc());
        let mut fb = Framebuffer::new(400, 300);
        // Prime at the top so line 0 (the edited line) is on-screen.
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);
        let color_ptr0 = ta.cache.pixels.as_ref().map(|a| a.as_ref().as_ptr() as usize);
        let alpha_ptr0 = ta
            .cache
            .lcd_alpha
            .as_ref()
            .map(|a| a.as_ref().as_ptr() as usize);
        let v0 = ta.cache.content_version;

        insert_char_on_top_line(&mut ta);
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);

        let color_ptr1 = ta.cache.pixels.as_ref().map(|a| a.as_ref().as_ptr() as usize);
        let alpha_ptr1 = ta
            .cache
            .lcd_alpha
            .as_ref()
            .map(|a| a.as_ref().as_ptr() as usize);
        let v1 = ta.cache.content_version;
        (
            color_ptr0.is_some() && color_ptr0 == color_ptr1,
            alpha_ptr0.is_some() && alpha_ptr0 == alpha_ptr1,
            v1 > v0,
            ta.debug_strip_raster_count(),
        )
    });
    assert_eq!(strips, 1, "the edit must take the in-place strip path");
    assert!(
        same_color,
        "a strip edit must update the colour plane in place (stable buffer address)"
    );
    assert!(
        same_alpha,
        "a strip edit must update the alpha plane in place (stable buffer address)"
    );
    assert!(
        version_grew,
        "an in-place strip edit must still bump content_version"
    );
}

#[test]
fn full_reraster_bumps_content_version() {
    // A re-anchor (jump the scroll out of the band) is a full raster, and it too
    // must change `content_version` so backends re-upload the rebuilt planes.
    let (grew, strips) = stable_window(|| {
        let mut ta = TextArea::new(font()).with_font_size(13.0).with_text(tall_doc());
        let mut fb = Framebuffer::new(400, 300);
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);
        let v0 = ta.cache.content_version;
        ta.vbar.offset = ta.band.anchor + ta.band.over_bottom + 800.0;
        layout_and_paint(&mut ta, &mut fb, 400.0, 300.0);
        (ta.cache.content_version > v0, ta.debug_strip_raster_count())
    });
    assert_eq!(strips, 0, "a re-anchor is a full raster, not a strip");
    assert!(grew, "a full re-anchor raster must bump content_version");
}
