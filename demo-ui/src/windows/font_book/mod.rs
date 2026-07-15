//! Font Book demo — a scrollable grid of every glyph the selected font
//! actually contains.
//!
//! Ported from egui's `font_book.rs`
//! (`egui-reference/crates/egui_demo_lib/src/demo/font_book.rs`): the glyph set
//! is enumerated from the live font's `cmap` (via `agg_gui::Font::characters()`)
//! rather than hard-coded, a filter narrows the grid by character or hex code
//! point, clicking a glyph copies it to the clipboard, and hovering shows the
//! char / hex / advance width.
//!
//! This module builds the surrounding window chrome; [`glyph_grid::GlyphGrid`]
//! does the enumeration, virtualization, click-to-copy, and tooltips. A
//! [`font_picker`](crate::font_picker) is included so the font can be switched
//! from within the window — the grid reflects the change on the next layout
//! because it reads `font_settings::current_system_font()` live.

mod glyph_grid;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Hyperlink, Label, Rect,
    ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

use glyph_grid::GlyphGrid;

const FONT_BOOK_SOURCE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows/font_book/mod.rs";

// ---------------------------------------------------------------------------
// CountLabel — live "supports N characters" readout
// ---------------------------------------------------------------------------

/// A label that reports the glyph count the grid publishes.
///
/// The count is only known after the grid has enumerated the font, which
/// happens later in the same frame's layout pass. Drawing the text directly in
/// `paint` (rather than through a child `Label` sized at layout time) keeps the
/// readout correct even on the very first frame and after a font swap, with no
/// width-clipping surprises — the same approach `scrolling::helpers::LiveLabel`
/// uses.
struct CountLabel {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    count: Rc<Cell<usize>>,
}

impl CountLabel {
    const FONT_SIZE: f64 = 12.0;

    fn new(font: Arc<Font>, count: Rc<Cell<usize>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            count,
        }
    }
}

impl Widget for CountLabel {
    fn type_name(&self) -> &'static str {
        "CountLabel"
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
        let h = Self::FONT_SIZE + 6.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(Self::FONT_SIZE);
        ctx.set_fill_color(v.text_color);
        let text = format!("The selected font supports {} characters.", self.count.get());
        let y = (self.bounds.height - Self::FONT_SIZE) * 0.5;
        ctx.fill_text(&text, 0.0, y);
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// font_book builder
// ---------------------------------------------------------------------------

/// Build the Font Book demo.
pub fn font_book(font: Arc<Font>) -> Box<dyn Widget> {
    let filter = Rc::new(RefCell::new(String::new()));
    let count = Rc::new(Cell::new(0usize));
    let viewport = Rc::new(Cell::new(Rect::default()));

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
        Box::new(CountLabel::new(Arc::clone(&font), Rc::clone(&count))),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "Click a glyph to copy it. You can add more characters by \
                 selecting a different font below or in the System window.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    // Font selector — reuses the shared picker; the grid tracks the resulting
    // system-font change live.
    col.push(
        Box::new(
            FlexRow::new()
                .with_gap(8.0)
                .add(Box::new(
                    Label::new("Font:", Arc::clone(&font)).with_font_size(13.0),
                ))
                .add(crate::font_picker::font_picker_with_size(
                    Arc::clone(&font),
                    13.0,
                )),
        ),
        0.0,
    );

    // Filter row: character/hex text field plus a clear button.
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
                        .with_placeholder("char or hex, e.g. 20AC")
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

    let grid = GlyphGrid::new(
        Arc::clone(&font),
        Rc::clone(&filter),
        Rc::clone(&count),
        Rc::clone(&viewport),
    );
    let scroll = ScrollView::new(Box::new(grid)).with_viewport_cell(Rc::clone(&viewport));
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}
