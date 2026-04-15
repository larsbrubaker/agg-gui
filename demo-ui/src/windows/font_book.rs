//! Font Book demo — scrollable grid of Unicode glyphs.
//!
//! Shows common Unicode ranges (Latin, digits, Greek, math symbols) as
//! individual glyph cells, each displaying the character and its hex codepoint.

use std::sync::Arc;

use agg_gui::{DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Label,
              Rect, ScrollView, Separator, Size, SizedBox, Widget};
use agg_gui::widget::paint_subtree;

// ---------------------------------------------------------------------------
// GlyphCell
// ---------------------------------------------------------------------------

/// A single-glyph cell: shows the character large, with a tiny codepoint label.
pub(super) struct GlyphCell {
    bounds:      Rect,
    children:    Vec<Box<dyn Widget>>,
    glyph_label: Label,
    cp_label:    Label,
}

impl GlyphCell {
    pub(super) fn new(ch: char, font: Arc<Font>) -> Self {
        let glyph = ch.to_string();
        let cp = format!("{:04X}", ch as u32);
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            glyph_label: Label::new(glyph, Arc::clone(&font)).with_font_size(18.0),
            cp_label: Label::new(cp, Arc::clone(&font)).with_font_size(7.5),
        }
    }
}

impl Widget for GlyphCell {
    fn type_name(&self) -> &'static str { "GlyphCell" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, _available: Size) -> Size {
        const W: f64 = 38.0;
        const H: f64 = 44.0;
        self.bounds = Rect::new(0.0, 0.0, W, H);

        let gs = self.glyph_label.layout(Size::new(W, H * 0.75));
        let gx = (W - gs.width) * 0.5;
        let gy = (H * 0.75 - gs.height) * 0.5 + H * 0.05;
        self.glyph_label.set_bounds(Rect::new(gx, gy, gs.width, gs.height));

        let cs = self.cp_label.layout(Size::new(W, H * 0.25));
        let cx = (W - cs.width) * 0.5;
        self.cp_label.set_bounds(Rect::new(cx, 3.0, cs.width, cs.height));

        Size::new(W, H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.fill();

        self.glyph_label.set_color(v.text_color);
        let gb = self.glyph_label.bounds();
        ctx.save(); ctx.translate(gb.x, gb.y);
        paint_subtree(&mut self.glyph_label, ctx);
        ctx.restore();

        self.cp_label.set_color(v.text_dim);
        let cb = self.cp_label.bounds();
        ctx.save(); ctx.translate(cb.x, cb.y);
        paint_subtree(&mut self.cp_label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// font_book builder
// ---------------------------------------------------------------------------

/// Build the Font Book demo — a scrollable grid of Unicode glyphs grouped by
/// category (Latin uppercase, digits, Greek lowercase, math symbols).
pub fn font_book(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .with_panel_bg();

    let header = |text: &str, font: &Arc<Font>| -> Box<dyn Widget> {
        Box::new(Label::new(text, Arc::clone(font)).with_font_size(11.0))
    };

    let glyph_row = |chars: &[char], font: &Arc<Font>| -> Box<dyn Widget> {
        let mut row = FlexRow::new().with_gap(4.0);
        for &ch in chars {
            row.push(Box::new(GlyphCell::new(ch, Arc::clone(font))), 0.0);
        }
        Box::new(row)
    };

    col.push(header("Basic Latin — Uppercase", &font), 0.0);
    col.push(glyph_row(&('A'..='Z').collect::<Vec<_>>(), &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(header("Digits", &font), 0.0);
    col.push(glyph_row(&('0'..='9').collect::<Vec<_>>(), &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(header("Greek lowercase", &font), 0.0);
    col.push(glyph_row(&('α'..='ω').collect::<Vec<_>>(), &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    let math_syms: &[char] = &[
        '∑', '∏', '∫', '√', '∂', '∞', '≈', '≠', '≤', '≥',
        '±', '×', '÷', '∈', '∉', '⊂', '⊃', '∩', '∪', '∅',
    ];
    col.push(header("Math symbols", &font), 0.0);
    col.push(glyph_row(math_syms, &font), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(ScrollView::new(Box::new(col)))
}
