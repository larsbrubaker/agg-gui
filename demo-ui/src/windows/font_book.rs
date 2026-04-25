//! Font Book demo — scrollable grid of Unicode glyphs.
//!
//! Shows common Unicode ranges (Latin, digits, Greek, math symbols) as
//! individual glyph cells, each displaying the character and its hex codepoint.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Hyperlink, Label, Rect,
    ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

const FONT_BOOK_SOURCE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows/font_book.rs";

// ---------------------------------------------------------------------------
// GlyphCell
// ---------------------------------------------------------------------------

/// A single-glyph cell: shows the character large, with a tiny codepoint label.
pub(super) struct GlyphCell {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    glyph_label: Label,
    cp_label: Label,
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

    fn layout(&mut self, _available: Size) -> Size {
        const W: f64 = 38.0;
        const H: f64 = 44.0;
        self.bounds = Rect::new(0.0, 0.0, W, H);

        let gs = self.glyph_label.layout(Size::new(W, H * 0.75));
        let gx = (W - gs.width) * 0.5;
        let gy = (H * 0.75 - gs.height) * 0.5 + H * 0.05;
        self.glyph_label
            .set_bounds(Rect::new(gx, gy, gs.width, gs.height));

        let cs = self.cp_label.layout(Size::new(W, H * 0.25));
        let cx = (W - cs.width) * 0.5;
        self.cp_label
            .set_bounds(Rect::new(cx, 3.0, cs.width, cs.height));

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
        ctx.save();
        ctx.translate(gb.x, gb.y);
        paint_subtree(&mut self.glyph_label, ctx);
        ctx.restore();

        self.cp_label.set_color(v.text_dim);
        let cb = self.cp_label.bounds();
        ctx.save();
        ctx.translate(cb.x, cb.y);
        paint_subtree(&mut self.cp_label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

struct FilteredGlyphSections {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    filter: Rc<RefCell<String>>,
    last_filter: String,
}

impl FilteredGlyphSections {
    fn new(font: Arc<Font>, filter: Rc<RefCell<String>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            filter,
            last_filter: String::new(),
        }
    }

    fn matches_filter(ch: char, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        let lower = filter.to_lowercase();
        ch.to_string().to_lowercase().contains(&lower)
            || format!("{:04X}", ch as u32).contains(&filter.to_uppercase())
    }

    fn rebuild(&mut self) {
        let filter = self.filter.borrow().trim().to_string();
        if filter == self.last_filter && !self.children.is_empty() {
            return;
        }
        self.last_filter = filter.clone();
        self.children.clear();

        let sections: [(&str, Vec<char>); 4] = [
            ("Basic Latin — Uppercase", ('A'..='Z').collect()),
            ("Digits", ('0'..='9').collect()),
            ("Greek lowercase", ('α'..='ω').collect()),
            (
                "Math symbols",
                vec![
                    '∑', '∏', '∫', '√', '∂', '∞', '≈', '≠', '≤', '≥', '±', '×', '÷', '∈', '∉', '⊂',
                    '⊃', '∩', '∪', '∅',
                ],
            ),
        ];

        let mut any = false;
        for (title, chars) in sections {
            let filtered = chars
                .into_iter()
                .filter(|&ch| Self::matches_filter(ch, &filter))
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                continue;
            }
            any = true;
            self.children.push(Box::new(
                Label::new(title, Arc::clone(&self.font)).with_font_size(11.0),
            ));
            let mut row = FlexRow::new().with_gap(4.0);
            for ch in filtered {
                row.push(Box::new(GlyphCell::new(ch, Arc::clone(&self.font))), 0.0);
            }
            self.children.push(Box::new(row));
            self.children.push(Box::new(Separator::horizontal()));
        }

        if !any {
            self.children.push(Box::new(
                Label::new(
                    "No glyphs match the current filter.",
                    Arc::clone(&self.font),
                )
                .with_font_size(12.0),
            ));
        }
    }
}

impl Widget for FilteredGlyphSections {
    fn type_name(&self) -> &'static str {
        "FilteredGlyphSections"
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
        self.rebuild();
        let mut y = 0.0_f64;
        for child in &mut self.children {
            let size = child.layout(Size::new(available.width, f64::MAX / 2.0));
            child.set_bounds(Rect::new(0.0, y, available.width, size.height));
            y += size.height + 8.0;
        }
        let total = y.max(1.0);
        for child in &mut self.children {
            let b = child.bounds();
            child.set_bounds(Rect::new(0.0, total - b.y - b.height, b.width, b.height));
        }
        self.bounds = Rect::new(0.0, 0.0, available.width, total);
        Size::new(available.width, total)
    }

    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// font_book builder
// ---------------------------------------------------------------------------

/// Build the Font Book demo — a scrollable grid of Unicode glyphs grouped by
/// category (Latin uppercase, digits, Greek lowercase, math symbols).
pub fn font_book(font: Arc<Font>) -> Box<dyn Widget> {
    let filter = Rc::new(RefCell::new(String::new()));
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Hyperlink::new("(source code)", Arc::clone(&font))
                .with_font_size(11.0)
                .on_click(|| crate::url::open_url(FONT_BOOK_SOURCE_URL)),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "The selected font sample supports 80 visible characters in this compact agg-gui font book.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "Install or select additional fonts in the System window to exercise other faces.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    let filter_row = FlexRow::new()
        .with_gap(8.0)
        .add(Box::new(
            Label::new("Filter:", Arc::clone(&font)).with_font_size(13.0),
        ))
        .add(Box::new(
            SizedBox::new()
                .with_width(160.0)
                .with_height(28.0)
                .with_child(Box::new(
                    TextField::new(Arc::clone(&font))
                        .with_font_size(13.0)
                        .with_placeholder("type to filter")
                        .with_text_cell(Rc::clone(&filter)),
                )),
        ))
        .add(Box::new(
            Button::new("x", Arc::clone(&font))
                .with_font_size(12.0)
                .on_click({
                    let filter = Rc::clone(&filter);
                    move || filter.borrow_mut().clear()
                }),
        ));
    col.push(Box::new(filter_row), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(FilteredGlyphSections::new(Arc::clone(&font), filter)),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(ScrollView::new(Box::new(col)))
}
