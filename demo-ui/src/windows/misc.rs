//! Miscellaneous demo windows: Frame, Extra Viewport, Highlighting,
//! Interactive Container, Font Book, and Misc Demos.
//!
//! These demos showcase layout containers, custom painting, and Unicode glyph
//! display without requiring external state or animation.

#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Checkbox, CollapsingHeader, Color, DragValue, DrawCtx, Event, EventResult, FlexColumn, FlexRow,
    Font, Label, MouseButton, Point, RadioGroup, Rect, ScrollView, Separator, Size, SizedBox,
    Slider, Widget,
};

mod interactive_container;
mod misc_demos;
mod tree_section;
pub use interactive_container::interactive_container;
pub use misc_demos::misc_demos;
// ---------------------------------------------------------------------------
// Extra Viewport demo
// ---------------------------------------------------------------------------

/// Build the Extra Viewport demo — informational placeholder.
pub fn extra_viewport(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "Extra viewports are not supported on this platform.",
                Arc::clone(&font),
            )
            .with_font_size(13.0),
        ),
        0.0,
    );

    Box::new(col)
}

// ---------------------------------------------------------------------------
// Highlighting demo
// ---------------------------------------------------------------------------

/// A widget that draws colored highlight boxes behind individual words.
///
/// This simulates syntax highlighting without a real text-layout engine:
/// each word is measured, a highlight rect is drawn behind it, and then the
/// word is drawn on top.
struct HighlightWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    /// (word, highlight_color, text_color).
    words: Vec<(&'static str, Color, Color)>,
}

impl Widget for HighlightWidget {
    fn type_name(&self) -> &'static str {
        "HighlightWidget"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, 36.0);
        Size::new(available.width, 36.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(14.0);

        let pad = 4.0;
        let h = self.bounds.height;
        let mut x = pad;
        let baseline = h * 0.35; // Y-up: baseline in lower portion

        for (word, bg, fg) in &self.words {
            if let Some(m) = ctx.measure_text(word) {
                let word_w = m.width;
                let box_h = m.ascent - m.descent + 4.0;
                let box_y = baseline + m.descent - 2.0;

                // Highlight box.
                ctx.set_fill_color(*bg);
                ctx.begin_path();
                ctx.rounded_rect(x - 2.0, box_y, word_w + 4.0, box_h, 3.0);
                ctx.fill();

                // Word text.
                ctx.set_fill_color(*fg);
                ctx.fill_text(word, x, baseline);

                x += word_w + 8.0; // gap between words
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Highlighting demo — several highlighted word spans demonstrating
/// per-glyph color control.
pub fn highlighting(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Colored text segments", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    col.push(
        Box::new(HighlightWidget {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            words: vec![
                (
                    "fn",
                    Color::rgba(0.22, 0.45, 0.88, 0.30),
                    Color::rgb(0.22, 0.45, 0.88),
                ),
                (
                    "main",
                    Color::rgba(0.86, 0.78, 0.40, 0.30),
                    Color::rgb(0.86, 0.78, 0.40),
                ),
                (
                    "()",
                    Color::rgba(0.90, 0.90, 0.90, 0.10),
                    Color::rgb(0.70, 0.70, 0.70),
                ),
                (
                    "{",
                    Color::rgba(0.90, 0.90, 0.90, 0.10),
                    Color::rgb(0.90, 0.90, 0.90),
                ),
            ],
        }),
        0.0,
    );

    col.push(
        Box::new(HighlightWidget {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            words: vec![
                (
                    "let",
                    Color::rgba(0.22, 0.45, 0.88, 0.30),
                    Color::rgb(0.22, 0.45, 0.88),
                ),
                (
                    "x",
                    Color::rgba(0.90, 0.90, 0.90, 0.10),
                    Color::rgb(0.90, 0.90, 0.90),
                ),
                (
                    "=",
                    Color::rgba(0.90, 0.90, 0.90, 0.10),
                    Color::rgb(0.60, 0.60, 0.60),
                ),
                (
                    "42;",
                    Color::rgba(0.82, 0.60, 0.45, 0.30),
                    Color::rgb(0.82, 0.60, 0.45),
                ),
            ],
        }),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "Each token is measured, a highlight rect is drawn, then the text.",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// The Interactive Container demo lives in the `interactive_container`
// submodule (re-exported above) to keep this file within the line limit.

// font_book is in the sibling module font_book.rs (re-exported from windows.rs).
