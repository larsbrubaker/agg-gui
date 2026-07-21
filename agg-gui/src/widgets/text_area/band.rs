//! Over-scan band planning for [`TextArea`]'s scroll backbuffer.
//!
//! Scrolling a `TextArea` used to change its backbuffer-cache signature (the raw
//! scroll offset was part of the sig), so every wheel notch re-rastered the whole
//! LCD backbuffer — ~120 ms at 1400×1000. This module implements the *over-scan
//! band* model that decouples scrolling from rastering: the widget rasters a band
//! covering the viewport plus a margin of content above and below, anchored in
//! content space. As long as the live scroll offset stays inside the band,
//! scrolling only changes the blit offset the framework applies to the cached
//! band (see [`crate::widget::BackbufferBand`]) — no re-raster. Leaving the band
//! re-anchors it around the new offset and rasters once.
//!
//! The functions here are the pure band math (kept side-effect free so they are
//! unit-testable in isolation); the `impl TextArea` block wires them into the
//! widget's layout / paint / `backbuffer_band` path.

use super::*;
use crate::widget::BackbufferBand;

/// Find the visual-line index range that differs between two wraps, so an
/// in-place edit can re-raster just that strip. Returns `(first, last,
/// count_changed)` in NEW-line coordinates, where `first..=last` is the
/// smallest range covering every changed line (a common prefix and suffix of
/// identically-rendered lines is trimmed by comparing the rendered `text`).
/// `count_changed` is true when the edit added or removed visual lines, in
/// which case every line from `first` down shifts position and the caller must
/// repaint through the band bottom. `None` when the two wraps render identically.
pub(super) fn diff_line_range(
    old: &[WrappedLine],
    new: &[WrappedLine],
) -> Option<(usize, usize, bool)> {
    let count_changed = old.len() != new.len();
    let min_len = old.len().min(new.len());
    let mut first = 0;
    while first < min_len && old[first].text == new[first].text {
        first += 1;
    }
    if !count_changed && first == new.len() {
        return None; // identical
    }
    let mut back = 0;
    while back < min_len - first
        && old[old.len() - 1 - back].text == new[new.len() - 1 - back].text
    {
        back += 1;
    }
    let last = new.len().saturating_sub(1).saturating_sub(back);
    Some((first, last.max(first), count_changed))
}

/// Over-scan margin as a fraction of the viewport height, applied to *each*
/// side of the band. `0.5` gives one whole viewport of over-scan in total (half
/// above, half below), so — for the code editor's ~17.5 px line pitch and a
/// ~1000 px viewport — roughly nine 3-line wheel notches fit before the band
/// must re-anchor. Larger values re-anchor less often but make each re-anchor
/// raster (and the buffer) proportionally bigger; one viewport total is a good
/// balance and matches the design brief.
pub(crate) const BAND_OVERSCAN_FRACTION: f64 = 0.5;

/// Anchor + over-scan extents of the currently rastered band. Recomputed each
/// `layout`. `active` is false when the content fits (nothing to scroll), which
/// keeps the widget on the byte-identical bounds-sized backbuffer path.
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct BandState {
    pub active: bool,
    /// Scroll offset (content space) the band was rastered at.
    pub anchor: f64,
    /// Extra logical px of content rastered above the viewport top.
    pub over_top: f64,
    /// Extra logical px of content rastered below the viewport bottom.
    pub over_bottom: f64,
}

/// Over-scan extents around `anchor`, clamped to the content actually available
/// on each side. `margin` is the desired extent per side (logical px).
///
/// Scrolling up consumes `over_top` (content above the viewport, of which there
/// is `anchor` px); scrolling down consumes `over_bottom` (content below, of
/// which there is `max_scroll - anchor` px). Clamping to the available content
/// keeps the band from wasting buffer on regions that can never be scrolled into
/// view, and guarantees the in-band range never extends past the scroll limits.
pub(crate) fn plan_overscan(anchor: f64, max_scroll: f64, margin: f64) -> (f64, f64) {
    let margin = margin.max(0.0);
    let over_top = margin.min(anchor.max(0.0));
    let over_bottom = margin.min((max_scroll - anchor).max(0.0));
    (over_top, over_bottom)
}

/// Is the live scroll `offset` still inside the band anchored at `anchor`?
///
/// The band covers `[anchor - over_top, anchor + over_bottom]`; a small epsilon
/// absorbs float wobble at the exact edge so we don't re-anchor spuriously.
pub(crate) fn offset_in_band(offset: f64, anchor: f64, over_top: f64, over_bottom: f64) -> bool {
    const EPS: f64 = 1e-6;
    offset >= anchor - over_top - EPS && offset <= anchor + over_bottom + EPS
}

impl TextArea {
    /// Recompute the over-scan band after the wrap cache + scroll offset are up
    /// to date (called from `layout`, after `sync_scroll`). Re-anchors — and
    /// thereby changes the cache signature, forcing one re-raster — only when
    /// the live offset has left the current band, the band is inactive, or the
    /// content shrank below the anchor. Ordinary in-band scrolling leaves the
    /// band untouched, so the sig is stable and no re-raster happens.
    pub(crate) fn recompute_band(&mut self) {
        let max_scroll = self.max_scroll_y();
        if max_scroll <= 0.0 || self.cached_line_h <= 0.0 {
            // Everything fits: no scrolling, so keep the plain bounds-sized path.
            self.band.active = false;
            return;
        }
        let vp = self.inner_height();
        let margin = vp * BAND_OVERSCAN_FRACTION;
        let live = self.vbar.offset;
        let in_band = self.band.active
            && self.band.anchor <= max_scroll
            && offset_in_band(live, self.band.anchor, self.band.over_top, self.band.over_bottom);
        if !in_band {
            let anchor = live.clamp(0.0, max_scroll);
            let (over_top, over_bottom) = plan_overscan(anchor, max_scroll, margin);
            self.band = BandState {
                active: true,
                anchor,
                over_top,
                over_bottom,
            };
        }
    }

    /// The [`BackbufferBand`] the framework reads to size the taller buffer and
    /// compute the blit offset. `None` when the content fits (byte-identical
    /// bounds-sized path). `blit_dy` is the residual between the live offset and
    /// the band anchor — the only thing that changes as the user scrolls within
    /// the band, and it flows purely into the blit, never the raster.
    pub(crate) fn backbuffer_band_impl(&self) -> Option<BackbufferBand> {
        if !self.band.active {
            return None;
        }
        Some(BackbufferBand {
            overscan_top: self.band.over_top,
            overscan_bottom: self.band.over_bottom,
            blit_dy: self.vbar.offset - self.band.anchor,
        })
    }

    /// Vertical extent (local Y-up) of the band being painted into the buffer:
    /// the padded inner rect expanded by the over-scan margins. Used to expand
    /// the paint clip and to cull lines that fall entirely outside the band.
    /// Collapses to the plain inner rect when the band is inactive.
    pub(crate) fn band_clip_y(&self) -> (f64, f64) {
        let lo = self.padding;
        let hi = (self.bounds.height - self.padding).max(lo);
        if self.band.active {
            (lo - self.band.over_bottom, hi + self.band.over_top)
        } else {
            (lo, hi)
        }
    }

    /// Test/introspection hook: number of real backbuffer re-rasters (each
    /// `paint` into the buffer bumps it). Scrolling within a band must not
    /// increment this; re-anchoring or editing must.
    pub fn debug_raster_count(&self) -> u64 {
        self.raster_count.get()
    }

    /// Test/introspection hook: number of *partial* (dirty-line-strip)
    /// re-rasters. A localized edit within an active band bumps this; a
    /// re-anchor or a structural change (resize, wrap re-flow) does not.
    pub fn debug_strip_raster_count(&self) -> u64 {
        self.strip_raster_count.get()
    }

    /// Decide whether the pending re-raster can be confined to the edited line
    /// strip (repainting just those rows into the retained band buffer) instead
    /// of rebuilding the whole band. Sets [`Self::render_dirty_lines`]; `None`
    /// there means a full band repaint.
    ///
    /// A strip is safe only when the edit is a plain in-place text change with a
    /// collapsed caret (no selection band to update elsewhere) and nothing
    /// structural moved — same size, same band anchor + over-scan, same
    /// alignment and font size as the last raster. Anything else (a re-anchor, a
    /// resize, a selection, a width re-flow) falls back to a full repaint, which
    /// is always correct because the widget paints opaque coverage over the
    /// whole band. The framework additionally vetoes the strip (via
    /// `partial_allowed`) whenever it could not reuse the retained buffer.
    pub(crate) fn plan_dirty_strip(&mut self) {
        self.render_dirty_lines.set(None);
        if !self.band.active {
            return;
        }
        let Some((first, last, count_changed)) = self.wrap_change else {
            return;
        };
        let Some(prev) = self.last_sig.as_ref() else {
            return;
        };
        let (now_no_sel, _) = {
            let st = self.edit.borrow();
            (st.cursor == st.anchor, ())
        };
        let stable = now_no_sel
            && prev.cursor == prev.anchor
            && prev.band_active
            && prev.band_anchor_bits == self.band.anchor.to_bits()
            && prev.band_over_top_bits == self.band.over_top.to_bits()
            && prev.band_over_bottom_bits == self.band.over_bottom.to_bits()
            && prev.w_bits == self.bounds.width.to_bits()
            && prev.h_bits == self.bounds.height.to_bits()
            && prev.h_align == self.resolved_h_align()
            && prev.v_align == self.resolved_v_align()
            && prev.font_size_bits == self.font_size.to_bits();
        if !stable || self.cached_lines.is_empty() {
            return;
        }
        let n = self.cached_lines.len();
        // A wrap-count change shifts every line from `first` down, so repaint
        // through the last line (the band clip + per-line culling keep the
        // actual work to the band-visible rows).
        let end = if count_changed { n - 1 } else { last.min(n - 1) };
        let first = first.min(n - 1);
        self.render_dirty_lines.set(Some((first, end.max(first))));
    }

    /// Y-up strip extent (clamped to the band clip) covering visual lines
    /// `first..=last`, used by `paint` to fill the strip background and clip the
    /// strip re-raster. Uses the same `line_top_y` the glyph loop uses, so the
    /// strip lands exactly on the rows those lines paint into.
    pub(crate) fn dirty_strip_y(&self, first: usize, last: usize) -> (f64, f64) {
        let (clip_lo, clip_hi) = self.band_clip_y();
        // Line cells stack top-to-bottom in Y-up: `first` (smaller index) is
        // higher (larger Y). Use exact cell boundaries — the 1.35× line pitch
        // leaves a gap between glyph runs, so an opaque strip fill at the cell
        // edges covers the changed lines' glyphs without erasing the un-
        // repainted neighbours above/below (which the retained buffer keeps).
        let hi = self.line_top_y(first).min(clip_hi);
        let lo = (self.line_top_y(last) - self.cached_line_h).max(clip_lo);
        (lo, hi.max(lo))
    }

    /// Stroke the rounded border in the current focus/hover colour. Shared by
    /// the cached (non-band) paint and the fixed band-mode overlay so the frame
    /// looks identical either way.
    pub(crate) fn paint_border(&self, ctx: &mut dyn DrawCtx, v: &crate::theme::Visuals) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let border = if self.focused {
            v.accent
        } else if self.hovered {
            v.widget_stroke_active
        } else {
            v.widget_stroke
        };
        let line_width = if self.focused { 2.0 } else { 1.0 };
        ctx.set_stroke_color(border);
        ctx.set_line_width(line_width);
        ctx.begin_path();
        let inset = line_width * 0.5;
        ctx.rounded_rect(
            inset,
            inset,
            (w - line_width).max(0.0),
            (h - line_width).max(0.0),
            4.0,
        );
        ctx.stroke();
    }

    /// Re-establish the fixed frame over a scrolled band blit: fill the padding
    /// ring (rounded outer edge minus the sharp inner content rect, even-odd) in
    /// the widget bg so text that bled into the padding while scrolling is
    /// covered and the rounded corners are restored, then stroke the border.
    /// Drawn in `paint_overlay` so it never scrolls with the cached content.
    pub(crate) fn paint_band_frame(&self, ctx: &mut dyn DrawCtx, v: &crate::theme::Visuals) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let inner_w = (w - self.padding * 2.0).max(0.0);
        let inner_h = (h - self.padding * 2.0).max(0.0);
        ctx.set_fill_color(v.widget_bg);
        ctx.set_fill_rule(crate::draw_ctx::FillRule::EvenOdd);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.rect(self.padding, self.padding, inner_w, inner_h);
        ctx.fill();
        ctx.set_fill_rule(crate::draw_ctx::FillRule::NonZero);
        self.paint_border(ctx, v);
    }
}

#[cfg(test)]
mod tests {
    use super::{offset_in_band, plan_overscan};

    #[test]
    fn overscan_is_symmetric_in_the_interior() {
        // Anchored mid-document with plenty of content both sides: full margin.
        let (top, bottom) = plan_overscan(1000.0, 2000.0, 400.0);
        assert_eq!(top, 400.0);
        assert_eq!(bottom, 400.0);
    }

    #[test]
    fn overscan_clamps_to_available_content_at_the_top() {
        // Near the top there are only 100 px of content above the viewport, so
        // the top over-scan can't exceed that (nothing to raster above it).
        let (top, bottom) = plan_overscan(100.0, 2000.0, 400.0);
        assert_eq!(top, 100.0);
        assert_eq!(bottom, 400.0);
    }

    #[test]
    fn overscan_clamps_to_available_content_at_the_bottom() {
        let (top, bottom) = plan_overscan(1950.0, 2000.0, 400.0);
        assert_eq!(top, 400.0);
        assert_eq!(bottom, 50.0);
    }

    #[test]
    fn in_band_covers_anchor_plus_minus_overscan() {
        let (anchor, over_top, over_bottom) = (1000.0, 400.0, 400.0);
        assert!(offset_in_band(1000.0, anchor, over_top, over_bottom));
        assert!(offset_in_band(600.0, anchor, over_top, over_bottom)); // top edge
        assert!(offset_in_band(1400.0, anchor, over_top, over_bottom)); // bottom edge
        assert!(!offset_in_band(599.0, anchor, over_top, over_bottom));
        assert!(!offset_in_band(1401.0, anchor, over_top, over_bottom));
    }

    #[test]
    fn planned_band_never_extends_past_scroll_limits() {
        // Whatever the anchor, anchor - over_top >= 0 and
        // anchor + over_bottom <= max_scroll, so the in-band range is always a
        // reachable sub-range of [0, max_scroll].
        let max_scroll = 2000.0;
        let margin = 700.0;
        for &anchor in &[0.0, 5.0, 350.0, 1000.0, 1990.0, 2000.0] {
            let (top, bottom) = plan_overscan(anchor, max_scroll, margin);
            assert!(anchor - top >= -1e-9, "top edge below 0 at anchor {anchor}");
            assert!(
                anchor + bottom <= max_scroll + 1e-9,
                "bottom edge past max at anchor {anchor}"
            );
        }
    }
}
