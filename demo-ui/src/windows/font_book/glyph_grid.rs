//! Virtualized glyph grid for the Font Book demo.
//!
//! [`GlyphGrid`] enumerates the *real* code points of the currently selected
//! font via `agg_gui::Font::characters()` (see `agg-gui/src/text.rs`) and lays
//! them out as a wrapping grid of clickable cells. Only the rows inside the
//! `ScrollView` viewport allocate/paint widgets — the grid reads the shared
//! viewport cell each layout the same way `windows::scrolling::helpers::RowList`
//! does, so scrolling thousands of glyphs stays cheap.
//!
//! Each cell ([`GlyphCell`]) is wrapped in a framework [`Tooltip`] showing the
//! char, its hex code point, and its advance width; clicking a cell copies the
//! character to the clipboard through `agg_gui::clipboard`.
//!
//! Relationship to the rest of the demo: `mod.rs` builds the surrounding
//! window chrome (source link, count label, filter field, font picker) and
//! drops this grid inside a `ScrollView`. The live font is pulled from
//! `font_settings::current_system_font()` so picking a font — here or in the
//! System window — re-enumerates the grid on the next layout.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    clipboard, font_settings, measure_text_metrics, DrawCtx, Event, EventResult, Font, Label,
    MouseButton, Rect, Size, Tooltip, Widget,
};

/// Edge length of a single glyph cell, in pixels.
const CELL: f64 = 26.0;
/// Gap between adjacent cells, in pixels.
const GAP: f64 = 3.0;
/// Point size the glyph is rendered at inside a cell.
const GLYPH_SIZE: f64 = 18.0;

// ---------------------------------------------------------------------------
// GlyphCell — a single clickable glyph
// ---------------------------------------------------------------------------

/// One glyph cell: draws the character centered, highlights on hover, and
/// copies the character to the clipboard when clicked.
pub(super) struct GlyphCell {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    ch: char,
    font: Arc<Font>,
    hovered: bool,
}

impl GlyphCell {
    pub(super) fn new(ch: char, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            ch,
            font,
            hovered: false,
        }
    }
}

impl Widget for GlyphCell {
    fn type_name(&self) -> &'static str {
        "GlyphCell"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // egui renders frameless buttons; we mirror that by only painting a
        // surface on hover so the resting grid stays flat.
        if self.hovered {
            ctx.set_fill_color(v.widget_bg_hovered);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
            ctx.fill();
        }

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(GLYPH_SIZE);
        ctx.set_fill_color(v.text_color);
        let glyph = self.ch.to_string();
        if let Some(m) = ctx.measure_text(&glyph) {
            let gx = (w - m.width) * 0.5;
            let gy = m.centered_baseline_y(h);
            ctx.fill_text(&glyph, gx, gy);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let now = self.hit_test(*pos);
                if now != self.hovered {
                    self.hovered = now;
                    agg_gui::animation::request_draw();
                }
                EventResult::Ignored
            }
            // Consume the press so the framework captures this cell and
            // routes the matching release back to it.
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } if self.hit_test(*pos) => EventResult::Consumed,
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } if self.hit_test(*pos) => {
                clipboard::set_text(&self.ch.to_string());
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// Tooltip text for a glyph: char, hex code point, and advance width.
///
/// Glyph *names* are intentionally omitted — egui sources them from the
/// `unicode_names2` crate, which we deliberately do not pull in. Hex + advance
/// are computed from data we already have.
fn glyph_tooltip(ch: char, font: &Font) -> String {
    let advance = measure_text_metrics(font, &ch.to_string(), GLYPH_SIZE).width;
    format!("{ch}\nHex: U+{:04X}\nAdvance: {advance:.1} px", ch as u32)
}

// ---------------------------------------------------------------------------
// GlyphGrid — virtualized wrapping grid
// ---------------------------------------------------------------------------

pub(super) struct GlyphGrid {
    bounds: Rect,
    /// Pool of `Tooltip`-wrapped `GlyphCell`s covering the visible rows, plus
    /// (when the filter matches nothing) a single explanatory `Label`.
    children: Vec<Box<dyn Widget>>,
    /// Char currently mapped to each pool slot — lets scrolling reuse slots
    /// without rebuilding tooltips whose glyph didn't change.
    slot_chars: Vec<char>,

    /// Fallback font used only until the live system font resolves.
    fallback_font: Arc<Font>,
    filter: Rc<RefCell<String>>,
    /// Total supported-character count, published for the header label.
    count_cell: Rc<Cell<usize>>,
    /// Content-space viewport rect (top-down) shared with the `ScrollView`.
    viewport: Rc<Cell<Rect>>,

    /// Every renderable code point of the active font (whitespace/control
    /// stripped), re-enumerated whenever the font changes.
    all_chars: Vec<char>,
    /// `all_chars` narrowed by the current filter text.
    filtered: Vec<char>,
    /// Guards re-filtering: `None` forces a rebuild (font just changed).
    last_filter: Option<String>,
    /// Identity of the font `all_chars` was enumerated from.
    last_font_ptr: usize,
    /// Whether the empty-filter placeholder label currently occupies the pool.
    showing_empty: bool,
}

impl GlyphGrid {
    pub(super) fn new(
        font: Arc<Font>,
        filter: Rc<RefCell<String>>,
        count_cell: Rc<Cell<usize>>,
        viewport: Rc<Cell<Rect>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            slot_chars: Vec::new(),
            fallback_font: font,
            filter,
            count_cell,
            viewport,
            all_chars: Vec::new(),
            filtered: Vec::new(),
            last_filter: None,
            last_font_ptr: 0,
            showing_empty: false,
        }
    }

    /// The font the grid should currently reflect: the live system override
    /// if set, otherwise the fallback captured at construction.
    fn active_font(&self) -> Arc<Font> {
        font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.fallback_font))
    }

    /// Does `ch` pass the current filter? Matches the character itself or its
    /// hex code point (e.g. "20AC" or "20ac"). Name matching is not supported
    /// — see [`glyph_tooltip`].
    fn matches(ch: char, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        let lower = filter.to_lowercase();
        if ch.to_string().to_lowercase().contains(&lower) {
            return true;
        }
        format!("{:04X}", ch as u32).contains(&filter.to_uppercase())
    }

    /// Re-enumerate `all_chars` when the active font changed. Returns the
    /// active font so the caller can reuse it without a second lookup.
    fn refresh_font(&mut self) -> Arc<Font> {
        let font = self.active_font();
        let ptr = Arc::as_ptr(&font) as usize;
        if ptr != self.last_font_ptr {
            self.last_font_ptr = ptr;
            self.all_chars = font
                .characters()
                .into_iter()
                .filter(|c| !c.is_whitespace() && !c.is_control())
                .collect();
            self.count_cell.set(self.all_chars.len());
            // Force a re-filter against the new glyph set.
            self.last_filter = None;
        }
        font
    }

    /// Recompute `filtered` if the filter text (or the font) changed.
    fn refresh_filter(&mut self) {
        let filter = self.filter.borrow().trim().to_string();
        if self.last_filter.as_deref() == Some(filter.as_str()) {
            return;
        }
        self.filtered = self
            .all_chars
            .iter()
            .copied()
            .filter(|&c| Self::matches(c, &filter))
            .collect();
        self.last_filter = Some(filter);
    }

    /// Visible row range `[first, last)` from the shared viewport, in the
    /// same top-down content coordinates `RowList` uses.
    ///
    /// No special case for an unpublished viewport: `ScrollView::layout`
    /// runs the child's layout *before* writing the viewport cell, so on the
    /// first frame the cell is still `Rect::default()`. Treating that as a
    /// zero-height viewport naturally yields ~1 row (RowList does the same),
    /// instead of materializing every glyph cell in one layout pass.
    fn visible_rows(&self, rows: usize) -> (usize, usize) {
        let pitch = CELL + GAP;
        let vp = self.viewport.get();
        let first = (vp.y / pitch).floor().max(0.0) as usize;
        let last = ((vp.y + vp.height) / pitch).ceil() as usize + 1;
        (first.min(rows), last.min(rows))
    }

    fn show_empty_placeholder(&mut self, available: Size) {
        if !self.showing_empty {
            self.children.clear();
            self.slot_chars.clear();
            self.children.push(Box::new(
                Label::new(
                    "No glyphs match the current filter.",
                    Arc::clone(&self.fallback_font),
                )
                .with_font_size(12.0),
            ));
            self.showing_empty = true;
        }
        let h = CELL;
        if let Some(child) = self.children.first_mut() {
            let s = child.layout(Size::new(available.width, h));
            child.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        }
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
    }
}

impl Widget for GlyphGrid {
    fn type_name(&self) -> &'static str {
        "GlyphGrid"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let font = self.refresh_font();
        self.refresh_filter();

        let n = self.filtered.len();
        if n == 0 {
            self.show_empty_placeholder(available);
            return Size::new(available.width, self.bounds.height);
        }
        if self.showing_empty {
            // Leaving the empty state — drop the placeholder label.
            self.children.clear();
            self.slot_chars.clear();
            self.showing_empty = false;
        }

        let pitch = CELL + GAP;
        let cols = (((available.width + GAP) / pitch).floor() as usize).max(1);
        let rows = n.div_ceil(cols);
        let total_h = rows as f64 * pitch;

        let (first_row, last_row) = self.visible_rows(rows);

        // Collect the visible (char_index, char) pairs in row-major order.
        let mut visible: Vec<(usize, char)> = Vec::new();
        for row in first_row..last_row {
            for col in 0..cols {
                let idx = row * cols + col;
                if idx >= n {
                    break;
                }
                visible.push((idx, self.filtered[idx]));
            }
        }

        // Resize the pool to the visible count, reusing slots. A slot is only
        // rebuilt when the glyph mapped to it changed, so an idle grid keeps
        // each Tooltip's hover timer intact.
        self.slot_chars.resize(visible.len(), '\0');
        while self.children.len() < visible.len() {
            self.children.push(Self::build_cell('\0', &self.fallback_font));
            self.slot_chars[self.children.len() - 1] = '\0';
        }
        self.children.truncate(visible.len());

        for (slot, &(idx, ch)) in visible.iter().enumerate() {
            if self.slot_chars[slot] != ch {
                self.children[slot] = Self::build_cell(ch, &font);
                self.slot_chars[slot] = ch;
            }
            let row = idx / cols;
            let col = idx % cols;
            let x = col as f64 * pitch;
            // Y-up: row 0 sits at the top of the content rect.
            let y = total_h - row as f64 * pitch - CELL;
            let child = &mut self.children[slot];
            child.layout(Size::new(CELL, CELL));
            child.set_bounds(Rect::new(x, y, CELL, CELL));
        }

        self.bounds = Rect::new(0.0, 0.0, available.width, total_h);
        Size::new(available.width, total_h)
    }

    fn paint(&mut self, _: &mut dyn DrawCtx) {
        // Cells (and their tooltips) paint themselves via the tree walk.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

impl GlyphGrid {
    /// Build a tooltip-wrapped cell for `ch`. The tooltip text is baked from
    /// `font` so hex/advance reflect the glyph as it will render.
    fn build_cell(ch: char, font: &Arc<Font>) -> Box<dyn Widget> {
        let cell = GlyphCell::new(ch, Arc::clone(font));
        Box::new(Tooltip::new(
            Box::new(cell),
            glyph_tooltip(ch, font),
            Arc::clone(font),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter matches on the character itself (case-insensitively) or on
    /// its hex code point, in either case. An empty filter matches everything.
    #[test]
    fn filter_matches_char_and_hex() {
        // Empty filter — everything passes.
        assert!(GlyphGrid::matches('A', ""));

        // Character match, case-insensitive.
        assert!(GlyphGrid::matches('A', "a"));
        assert!(GlyphGrid::matches('€', "€"));

        // Hex code-point match, either case (U+20AC = €, U+0041 = A).
        assert!(GlyphGrid::matches('€', "20ac"));
        assert!(GlyphGrid::matches('€', "20AC"));
        assert!(GlyphGrid::matches('A', "0041"));
        // Partial hex substrings match too.
        assert!(GlyphGrid::matches('€', "AC"));

        // Non-matches.
        assert!(!GlyphGrid::matches('A', "z"));
        assert!(!GlyphGrid::matches('€', "0041"));
    }

    /// The tooltip carries the char, its hex code point, and an advance width.
    #[test]
    fn tooltip_reports_hex_and_advance() {
        const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
        let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
        let tip = glyph_tooltip('A', &font);
        assert!(tip.starts_with("A\n"), "first line is the glyph: {tip:?}");
        assert!(tip.contains("Hex: U+0041"), "hex code point: {tip:?}");
        assert!(tip.contains("Advance:"), "advance width line: {tip:?}");
    }
}
