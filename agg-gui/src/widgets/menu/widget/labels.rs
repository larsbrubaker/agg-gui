//! Label caches for the menu bar and popup.
//!
//! Menus compose their text rendering out of [`Label`] widgets so glyphs flow
//! through the framework's standard backbuffer + LCD subpixel path.  Each
//! cache stores its `Label`s by stable identity (bar-button index, or
//! `(layout_index, row_index)` for popup rows) and rebuilds them lazily when
//! the source text changes.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::label::{Label, LabelAlign};

use super::super::geometry::PopupLayout;
use super::super::model::MenuEntry;
use super::super::paint::popup_row_text_color;

/// Per-bar-button `Label` cache.  Keyed by index into the bar's
/// `menus: Vec<TopMenu>`; rebuilt when the menu list changes
/// (`sync_to(...)`).
pub struct BarLabels {
    labels: Vec<Label>,
}

impl BarLabels {
    pub fn new() -> Self {
        Self { labels: Vec::new() }
    }

    /// Ensure there's one `Label` per `menu`, matching label text.
    /// Rebuilds entries whose text changed; leaves the rest in place so
    /// their backbuffer caches keep their pre-rasterised glyphs.
    pub fn sync_to(&mut self, font: &Arc<Font>, font_size: f64, labels: &[&str]) {
        if self.labels.len() != labels.len() {
            // Resize first (cheap to recreate; bar entries rarely change).
            self.labels = labels
                .iter()
                .map(|text| make_bar_label(text, font, font_size))
                .collect();
            return;
        }
        for (slot, text) in self.labels.iter_mut().zip(labels.iter()) {
            if slot.text_str() != *text {
                slot.set_text(*text);
            }
        }
    }

    /// Layout the label inside `slot` (the bar item's rect) and paint
    /// it through `paint_subtree` so glyphs flow through Label's own
    /// backbuffer.  `color` is applied before paint so an open / hover
    /// state change retints without a Label rebuild.
    pub fn paint_in(&mut self, ctx: &mut dyn DrawCtx, idx: usize, slot: Rect, color: Color) {
        let Some(label) = self.labels.get_mut(idx) else {
            return;
        };
        label.set_color(color);
        // Lay out the label so it knows its measured width (used by the
        // backbuffer-cache size).  Bar buttons paint their text with a
        // small left inset matching the historical 9-px gutter.
        let avail_h = slot.height.max(1.0);
        let size = label.layout(Size::new(slot.width, avail_h));
        let x = slot.x + 9.0;
        let y = slot.y + (slot.height - size.height) * 0.5;
        label.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        ctx.save();
        ctx.translate(x, y);
        paint_subtree(label, ctx);
        ctx.restore();
    }
}

impl Default for BarLabels {
    fn default() -> Self {
        Self::new()
    }
}

/// `Label`s the popup needs for a single row: the item's display text
/// and the optional shortcut on the right.
pub struct PopupRowLabels {
    pub label: Label,
    pub shortcut: Option<Label>,
}

/// Cache of `Label`s for every visible popup row, keyed by
/// `(layout_index, row_index)`.  Rebuilt whenever the visible set
/// changes (`sync_to(...)` is cheap when nothing differs).
pub struct PopupLabels {
    /// Outer vec: one entry per layout level (root popup, submenu,
    /// nested submenu, …).  Inner vec: one entry per row at that
    /// level.  `Option` because separator rows have no labels but we
    /// keep the slot to preserve `row_index` alignment.
    rows: Vec<Vec<Option<PopupRowLabels>>>,
    /// Font + size we last built with.  Rebuilds happen on change so a
    /// system-font swap propagates through every cached label.
    last_font_ptr: *const Font,
    last_font_size: f64,
}

impl PopupLabels {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            last_font_ptr: std::ptr::null(),
            last_font_size: 0.0,
        }
    }

    /// Build or refresh the row label cache for the current popup
    /// layout tree.  Walks `layouts` and `items` together, creating a
    /// `PopupRowLabels` for every item row.  Existing labels are
    /// preserved (and only `set_text`-ed when the text changed) so
    /// their backbuffer cache stays warm across opens / hovers.
    pub fn sync_to(
        &mut self,
        font: &Arc<Font>,
        font_size: f64,
        items: &[MenuEntry],
        layouts: &[PopupLayout],
    ) {
        let font_ptr = Arc::as_ptr(font);
        let font_changed =
            font_ptr != self.last_font_ptr || (self.last_font_size - font_size).abs() > 0.01;
        if font_changed {
            self.rows.clear();
            self.last_font_ptr = font_ptr;
            self.last_font_size = font_size;
        }
        // Resize outer vec to one entry per layout.
        if self.rows.len() != layouts.len() {
            self.rows.resize_with(layouts.len(), Vec::new);
        }
        for (level_idx, layout) in layouts.iter().enumerate() {
            let level_items = items_for_layout(items, &layout.path_prefix);
            let level = &mut self.rows[level_idx];
            if level.len() != layout.rows.len() {
                level.clear();
                level.resize_with(layout.rows.len(), || None);
            }
            for (row_idx, row) in layout.rows.iter().enumerate() {
                let Some(item_idx) = row.item_index else {
                    // Separator: no label needed.  Clear any stale slot.
                    level[row_idx] = None;
                    continue;
                };
                let Some(MenuEntry::Item(item)) = level_items.get(item_idx) else {
                    level[row_idx] = None;
                    continue;
                };
                match &mut level[row_idx] {
                    Some(existing) => {
                        if existing.label.text_str() != item.label {
                            existing.label.set_text(&item.label);
                        }
                        match (&mut existing.shortcut, item.shortcut.as_deref()) {
                            (Some(slot), Some(text)) => {
                                if slot.text_str() != text {
                                    slot.set_text(text);
                                }
                            }
                            (slot @ Some(_), None) => *slot = None,
                            (slot @ None, Some(text)) => {
                                *slot = Some(make_shortcut_label(text, font, font_size));
                            }
                            (None, None) => {}
                        }
                    }
                    slot @ None => {
                        *slot = Some(PopupRowLabels {
                            label: make_label(&item.label, font, font_size),
                            shortcut: item
                                .shortcut
                                .as_deref()
                                .map(|s| make_shortcut_label(s, font, font_size)),
                        });
                    }
                }
            }
        }
    }

    /// Paint a single popup row's text (label + optional shortcut).
    /// Background, icon, check / radio glyph, and submenu chevron are
    /// painted by the caller.  `color` is applied to BOTH the label and
    /// the shortcut so a hover / open state change retints in one shot.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_row(
        &mut self,
        ctx: &mut dyn DrawCtx,
        level_idx: usize,
        row_idx: usize,
        row_rect: Rect,
        label_x: f64,
        shortcut_right_pad: f64,
        color: Color,
    ) {
        let Some(level) = self.rows.get_mut(level_idx) else {
            return;
        };
        let Some(Some(row)) = level.get_mut(row_idx) else {
            return;
        };

        // Main label.
        row.label.set_color(color);
        let inner_w = (row_rect.width - label_x).max(0.0);
        let size = row.label.layout(Size::new(inner_w, row_rect.height));
        row.label
            .set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        let lx = row_rect.x + label_x;
        let ly = row_rect.y + (row_rect.height - size.height) * 0.5;
        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut row.label, ctx);
        ctx.restore();

        // Optional shortcut, right-aligned.
        if let Some(shortcut) = row.shortcut.as_mut() {
            shortcut.set_color(color);
            let s_size = shortcut.layout(Size::new(row_rect.width, row_rect.height));
            shortcut.set_bounds(Rect::new(0.0, 0.0, s_size.width, s_size.height));
            let sx = row_rect.x + row_rect.width - shortcut_right_pad - s_size.width;
            let sy = row_rect.y + (row_rect.height - s_size.height) * 0.5;
            ctx.save();
            ctx.translate(sx, sy);
            paint_subtree(shortcut, ctx);
            ctx.restore();
        }
    }

    /// Convenience: re-resolve the per-row text colour from the
    /// item's enabled/open state and paint the row's text.  Used by
    /// [`PopupMenu::paint`] to keep the colour-resolution logic in one
    /// place.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_row_with_state(
        &mut self,
        ctx: &mut dyn DrawCtx,
        level_idx: usize,
        row_idx: usize,
        row_rect: Rect,
        label_x: f64,
        shortcut_right_pad: f64,
        enabled: bool,
        open: bool,
    ) {
        let color = popup_row_text_color(ctx, enabled, open);
        self.paint_row(
            ctx,
            level_idx,
            row_idx,
            row_rect,
            label_x,
            shortcut_right_pad,
            color,
        );
    }
}

impl Default for PopupLabels {
    fn default() -> Self {
        Self::new()
    }
}

// Note: Clone is implemented so `PopupMenu` (and its host `MenuBar`) can stay
// `Clone`.  Cloning drops the cache and starts fresh — labels rebuild on the
// next `sync_to`.  Cheap because the cache is empty most of the time.
impl Clone for PopupLabels {
    fn clone(&self) -> Self {
        Self::new()
    }
}

fn make_label(text: &str, font: &Arc<Font>, font_size: f64) -> Label {
    Label::new(text, Arc::clone(font))
        .with_font_size(font_size)
        .with_align(LabelAlign::Left)
}

/// Build a menu-**bar** button label.
///
/// Identical to [`make_label`] except the Label is created *non-buffered*
/// (`with_has_backbuffer(false)`).  The `MenuBar` is itself a backbuffered
/// widget that rasterises into an `LcdGfxCtx` (when LCD is on); a *buffered*
/// child Label would rasterise into its own `LcdCoverage` cache and then
/// composite back through the default `draw_lcd_backbuffer_arc`, which
/// collapses per-channel coverage to a single alpha — making the bar text
/// grayscale while the popup (painted onto the real ctx) stays subpixel.
/// Painting the bar label directly via `LcdGfxCtx::fill_text` preserves LCD
/// chroma.  No caching is lost: the bar's own backbuffer holds the composited
/// pixels, so the label only re-rasterises when the bar does.
fn make_bar_label(text: &str, font: &Arc<Font>, font_size: f64) -> Label {
    make_label(text, font, font_size).with_has_backbuffer(false)
}

fn make_shortcut_label(text: &str, font: &Arc<Font>, font_size: f64) -> Label {
    Label::new(text, Arc::clone(font))
        .with_font_size(font_size)
        .with_align(LabelAlign::Left)
}

fn items_for_layout<'a>(items: &'a [MenuEntry], path: &[usize]) -> &'a [MenuEntry] {
    let mut current = items;
    for &idx in path {
        let Some(MenuEntry::Item(item)) = current.get(idx) else {
            return current;
        };
        current = &item.submenu;
    }
    current
}

#[cfg(test)]
mod lcd_tests {
    use super::BarLabels;
    use crate::color::Color;
    use crate::draw_ctx::DrawCtx;
    use crate::font_settings::{clear_lcd_enabled_override, set_lcd_enabled};
    use crate::geometry::Rect;
    use crate::lcd_coverage::LcdBuffer;
    use crate::lcd_gfx_ctx::LcdGfxCtx;
    use crate::text::Font;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");

    fn font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    /// True when any pixel's R/G/B coverage channels differ — the
    /// signature of LCD subpixel rendering.  A grayscale (collapsed)
    /// raster has `R == G == B` for every pixel.
    fn has_subpixel(buf: &LcdBuffer) -> bool {
        buf.color_plane()
            .chunks_exact(3)
            .any(|p| p[0] != p[1] || p[1] != p[2])
    }

    /// Regression: a menu-bar button label, painted into the bar's own
    /// `LcdCoverage` backbuffer (an [`LcdGfxCtx`]), must keep its LCD
    /// subpixel coverage.
    ///
    /// The bug: bar labels were *buffered*, so each rasterised into its
    /// own `LcdCoverage` cache and was then composited into the bar's
    /// `LcdGfxCtx` via the default `draw_lcd_backbuffer_arc`, which
    /// collapses per-channel coverage to a single alpha
    /// (`a = ra.max(ga).max(ba)`).  That left the bar text grayscale
    /// while every other (real-ctx) label rendered LCD — exactly the
    /// "top menu is blurry / not subpixel" report.  Painting the label
    /// *directly* (non-buffered) routes through `LcdGfxCtx::fill_text`,
    /// preserving subpixel chroma.
    #[test]
    fn bar_label_keeps_lcd_subpixel_inside_backbuffer_ctx() {
        set_lcd_enabled(true);
        let font = font();
        let mut bar = BarLabels::new();
        bar.sync_to(&font, 14.0, &["File"]);

        let mut buf = LcdBuffer::new(120, 28);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            bar.paint_in(&mut ctx, 0, Rect::new(0.0, 0.0, 80.0, 28.0), Color::black());
        }
        let subpixel = has_subpixel(&buf);
        clear_lcd_enabled_override();

        assert!(
            subpixel,
            "menu-bar label lost LCD subpixel coverage when painted into the \
             bar's LcdCoverage backbuffer (nested-backbuffer collapse regression)"
        );
    }
}
