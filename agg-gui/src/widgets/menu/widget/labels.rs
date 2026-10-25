//! Label caches for the menu bar and popup.
//!
//! Menus compose their text rendering out of [`Label`] widgets so glyphs flow
//! through the framework's standard backbuffer + LCD subpixel path.  A popup
//! row's icon glyph (`MenuItem::icon`) is a `Label` too, laid out in the same
//! line box as the row's text, so the two share a baseline pixel row under
//! every line box and device scale.  Each
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

use super::super::fit_width::{menu_text_width, SHORTCUT_GAP};
use super::super::geometry::PopupLayout;
use super::super::model::MenuEntry;
use super::super::paint::popup_row_text_color;
use super::super::style::ShortcutFormat;

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
    /// The row's icon glyph, when it has one.
    pub icon: Option<Label>,
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
    /// Font (its [`Font::id`], 0 before the first build) + size we last
    /// built with.  Rebuilds happen on change so a system-font swap
    /// propagates through every cached label.  The id, not the font's
    /// address: a freed font's address can be handed to the next font
    /// (the cached labels hold their font alive today, so the key must not
    /// depend on that to stay correct).
    last_font_id: u64,
    last_font_size: f64,
    /// The font the labels were last built with, for measuring a shortcut's
    /// column the way a fitted row was sized (`fit_width::menu_text_width`).
    font: Option<Arc<Font>>,
}

impl PopupLabels {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            last_font_id: 0,
            last_font_size: 0.0,
            font: None,
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
        shortcut_format: ShortcutFormat,
    ) {
        let font_id = font.id();
        let font_changed =
            font_id != self.last_font_id || (self.last_font_size - font_size).abs() > 0.01;
        if font_changed {
            self.rows.clear();
            self.last_font_id = font_id;
            self.last_font_size = font_size;
            self.font = Some(Arc::clone(font));
        }
        // Resize outer vec to one entry per layout.
        if self.rows.len() != layouts.len() {
            self.rows.resize_with(layouts.len(), Vec::new);
        }
        let platform = crate::platform::current_platform();
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
                // Formatted per the popup's `ShortcutFormat` (by default
                // `⌘X` on a Mac, `Ctrl+X` elsewhere), never the raw
                // declaration string of a parsed shortcut.
                let shortcut_text = item.shortcut_text_formatted(platform, font, shortcut_format);
                let icon_text = item.icon.map(String::from);
                match &mut level[row_idx] {
                    Some(existing) => {
                        if existing.label.text_str() != item.label {
                            existing.label.set_text(&item.label);
                        }
                        match (&mut existing.shortcut, shortcut_text.as_deref()) {
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
                        match (&mut existing.icon, icon_text.as_deref()) {
                            (Some(slot), Some(text)) => {
                                if slot.text_str() != text {
                                    slot.set_text(text);
                                }
                            }
                            (slot, text) => {
                                *slot = text.map(|t| make_label(t, font, font_size));
                            }
                        }
                    }
                    slot @ None => {
                        *slot = Some(PopupRowLabels {
                            label: make_row_label(&item.label, font, font_size),
                            shortcut: shortcut_text
                                .as_deref()
                                .map(|s| make_shortcut_label(s, font, font_size)),
                            icon: icon_text.as_deref().map(|t| make_label(t, font, font_size)),
                        });
                    }
                }
            }
        }
    }

    /// The main label of row `row_idx` in popup level `level_idx`, once
    /// built.
    #[cfg(test)]
    pub(super) fn row_label(&self, level_idx: usize, row_idx: usize) -> Option<&Label> {
        self.rows
            .get(level_idx)?
            .get(row_idx)?
            .as_ref()
            .map(|row| &row.label)
    }

    /// The shortcut label of row `row_idx` in popup level `level_idx`, once
    /// built and when the row has one.
    #[cfg(test)]
    pub(super) fn row_shortcut(&self, level_idx: usize, row_idx: usize) -> Option<&Label> {
        self.rows
            .get(level_idx)?
            .get(row_idx)?
            .as_ref()
            .and_then(|row| row.shortcut.as_ref())
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
        let (font, font_size) = (self.font.clone(), self.last_font_size);
        let Some(level) = self.rows.get_mut(level_idx) else {
            return;
        };
        let Some(Some(row)) = level.get_mut(row_idx) else {
            return;
        };

        // The shortcut first: the label gets what the row has left beside
        // it, the gap a fitted popup keeps (`fit_width::SHORTCUT_GAP`)
        // included, and the right-hand column the chevron and right-edge
        // checks sit in.  On a window too narrow for the row's natural width
        // (a phone) the label then ends in an ellipsis instead of running
        // under the shortcut or off the panel.  The column is the shortcut's
        // unsnapped advance, the width a fitted row was sized with, not its
        // laid-out width (rounded up to device pixels under
        // `LineBox::EmSnapped`), so a fitted row never elides.
        let shortcut_size = row.shortcut.as_mut().map(|shortcut| {
            shortcut.set_color(color);
            shortcut.layout(Size::new(row_rect.width, row_rect.height))
        });
        let shortcut_w = match (&row.shortcut, shortcut_size, &font) {
            (Some(shortcut), Some(_), Some(font)) => {
                menu_text_width(font, font_size, shortcut.text_str()) + SHORTCUT_GAP
            }
            (_, Some(size), None) => size.width + SHORTCUT_GAP,
            _ => 0.0,
        };

        // Main label.
        row.label.set_color(color);
        let inner_w = (row_rect.width - label_x - shortcut_right_pad - shortcut_w).max(0.0);
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
        if let (Some(shortcut), Some(s_size)) = (row.shortcut.as_mut(), shortcut_size) {
            shortcut.set_bounds(Rect::new(0.0, 0.0, s_size.width, s_size.height));
            let sx = row_rect.x + row_rect.width - shortcut_right_pad - s_size.width;
            let sy = row_rect.y + (row_rect.height - s_size.height) * 0.5;
            ctx.save();
            ctx.translate(sx, sy);
            paint_subtree(shortcut, ctx);
            ctx.restore();
        }
    }

    /// Paint row `row_idx`'s icon glyph in popup level `level_idx` with its
    /// left edge `icon_x` into `row_rect`, in `color`.  Its line box is
    /// centred on the row exactly as the row's label's is, so both stand on
    /// one baseline.  `false` when the row has no icon label.
    pub fn paint_icon(
        &mut self,
        ctx: &mut dyn DrawCtx,
        level_idx: usize,
        row_idx: usize,
        row_rect: Rect,
        icon_x: f64,
        color: Color,
    ) -> bool {
        let Some(icon) = self
            .rows
            .get_mut(level_idx)
            .and_then(|level| level.get_mut(row_idx))
            .and_then(|row| row.as_mut())
            .and_then(|row| row.icon.as_mut())
        else {
            return false;
        };
        icon.set_color(color);
        let size = icon.layout(Size::new(row_rect.width, row_rect.height));
        icon.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        ctx.save();
        ctx.translate(
            row_rect.x + icon_x,
            row_rect.y + (row_rect.height - size.height) * 0.5,
        );
        paint_subtree(icon, ctx);
        ctx.restore();
        true
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

/// A popup row's label: ends in an ellipsis when its row is too narrow for
/// it ([`PopupLabels::paint_row`]).  Bar titles keep [`make_bar_label`]'s
/// clipping; the bar scrolls its titles instead.
fn make_row_label(text: &str, font: &Arc<Font>, font_size: f64) -> Label {
    make_label(text, font, font_size).with_ellipsis_if_clipped(true)
}

/// Build a menu-**bar** button label.
///
/// Identical to [`make_label`] except the Label is created *non-buffered*
/// (`with_has_backbuffer(false)`).  The `MenuBar` is itself a backbuffered
/// widget that rasterises into an `LcdGfxCtx` (when LCD is on), so a second
/// cache per label would only duplicate it: painting the bar label directly
/// via `LcdGfxCtx::fill_text` keeps its LCD chroma, and no caching is lost —
/// the bar's own backbuffer holds the composited pixels, so the label only
/// re-rasterises when the bar does.  (`LcdGfxCtx::draw_lcd_backbuffer_arc`
/// merges a buffered child's planes per channel too, so either way the bar
/// text stays subpixel.)
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
    /// Grayscale bar text while every other (real-ctx) label rendered
    /// LCD was the "top menu is blurry / not subpixel" report.  The bar
    /// label paints *directly* (non-buffered) through
    /// `LcdGfxCtx::fill_text`, preserving subpixel chroma.
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
