//! Miscellaneous demo windows: Interactive Container, Font Book, and Misc Demos.
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

mod misc_demos;
pub use misc_demos::misc_demos;

// ---------------------------------------------------------------------------
// Interactive Container demo
// ---------------------------------------------------------------------------

/// A widget that changes its appearance on hover and click.
///
/// Text is rendered through a backbuffered Label child.  Because the click
/// count changes rarely (only on mouse-up), the label cache stays warm most
/// frames and avoids unnecessary glyph rasterization.
struct InteractiveBox {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    hovered: bool,
    pressed: bool,
    clicks: u32,
    /// Backbuffered label for the centered text.
    label_widget: Label,
}

impl InteractiveBox {
    fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            hovered: false,
            pressed: false,
            clicks: 0,
            label_widget: Label::new("Click me!", font).with_font_size(13.0),
        }
    }
}

impl Widget for InteractiveBox {
    fn type_name(&self) -> &'static str {
        "InteractiveBox"
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
        let w = available.width.min(200.0);
        let h = 60.0_f64;
        self.bounds = Rect::new(0.0, 0.0, w, h);

        // Update label text from click count.
        let text = if self.clicks == 0 {
            "Click me!".to_string()
        } else {
            format!(
                "Clicked {} time{}",
                self.clicks,
                if self.clicks == 1 { "" } else { "s" }
            )
        };
        self.label_widget.set_text(text);

        // Center the label within the box.
        let ls = self.label_widget.layout(Size::new(w, h));
        let lx = (w - ls.width) * 0.5;
        let ly = (h - ls.height) * 0.5;
        self.label_widget
            .set_bounds(Rect::new(lx, ly, ls.width, ls.height));

        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        let bg = if self.pressed {
            v.accent_pressed
        } else if self.hovered {
            v.accent_hovered
        } else {
            v.widget_bg
        };

        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 8.0);
        ctx.fill();

        ctx.set_stroke_color(if self.hovered {
            v.accent
        } else {
            v.widget_stroke
        });
        ctx.set_line_width(if self.hovered { 2.0 } else { 1.0 });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 8.0);
        ctx.stroke();

        // Paint label via backbuffered child.
        let text_color = if self.pressed {
            Color::white()
        } else {
            v.text_color
        };
        self.label_widget.set_color(text_color);
        let lb = self.label_widget.bounds();
        ctx.save();
        ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label_widget, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was_hovered = self.hovered;
                self.hovered = pos.x >= 0.0
                    && pos.x <= self.bounds.width
                    && pos.y >= 0.0
                    && pos.y <= self.bounds.height;
                if self.hovered != was_hovered {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                if self.hovered {
                    self.pressed = true;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.pressed {
                    self.pressed = false;
                    if self.hovered {
                        self.clicks += 1;
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

/// Build the Interactive Container demo — a box that responds to hover and click.
pub fn interactive_container(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Hover and click the box", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    col.push(Box::new(InteractiveBox::new(Arc::clone(&font))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "Background, border, and label change on hover / press.",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// font_book is in the sibling module font_book.rs (re-exported from windows.rs).
