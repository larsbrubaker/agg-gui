//! Text-related and layout demo windows: scrolling rows, strip layout, table,
//! text layout showcase, undo/redo, window options, modals, and multi-touch info.
//!
//! Most demos here are purely compositional — they build a widget tree from
//! `FlexColumn`, `FlexRow`, `Container`, `Label`, etc. without custom painting.

#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    measure_text_metrics, Button, Checkbox, Color, Container, DragValue, DrawCtx, Event,
    EventResult, FlexColumn, FlexRow, Font, Label, LabelAlign, MouseButton, Point, Rect,
    ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

// ---------------------------------------------------------------------------
// Strip demo
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct StripRegion {
    rect: Rect,
    label: &'static str,
    bg: Color,
}

fn faded_strip_color(color: Color) -> Color {
    Color::rgba(color.r, color.g, color.b, 0.22)
}

fn strip_demo_regions(width: f64, height: f64, body_text_size: f64) -> Vec<StripRegion> {
    // Egui's `StripBuilder` exact sizes and `at_least` constraints impose
    // lower bounds even when the surrounding window gets small. Preserve those
    // content constraints in our hand-rolled Y-up layout.
    const MIN_WIDTH_FROM_EXACT_RECTS: f64 = 120.0 + 70.0;
    let footer_min = body_text_size.max(12.0);
    let w = width.max(MIN_WIDTH_FROM_EXACT_RECTS);
    let h = height.max(50.0 + 60.0 + footer_min);
    let footer_h = body_text_size.max(12.0).min(h);
    let top_h = 50.0_f64.min((h - footer_h).max(0.0));
    let remaining = (h - footer_h - top_h).max(0.0);
    let lower_h = (remaining * 0.5).max(60.0).min(remaining);
    let middle_h = (remaining - lower_h).max(0.0);
    let lower_y = footer_h;
    let middle_y = lower_y + lower_h;
    let top_y = middle_y + middle_h;

    let middle_half_w = w * 0.5;
    let yellow_h = middle_h / 3.0;
    let fixed_w = MIN_WIDTH_FROM_EXACT_RECTS;
    let lower_gap_w = (w - fixed_w) * 0.5;
    let gold_x = lower_gap_w;
    let green_x = (w - 70.0).max(0.0);
    let gold_y = lower_y + (lower_h - 60.0).max(0.0) * 0.5;
    let green_h = (lower_h * 0.5).max(60.0).min(lower_h);
    let green_y = lower_y + (lower_h - green_h) * 0.5;

    vec![
        StripRegion {
            rect: Rect::new(0.0, top_y, w, top_h),
            label: "width: 100%\nheight: 50px",
            bg: faded_strip_color(Color::rgb(0.0, 0.0, 1.0)),
        },
        StripRegion {
            rect: Rect::new(0.0, middle_y, middle_half_w, middle_h),
            label: "width: 50%\nheight: remaining",
            bg: faded_strip_color(Color::rgb(1.0, 0.0, 0.0)),
        },
        StripRegion {
            rect: Rect::new(middle_half_w, middle_y + yellow_h, middle_half_w, yellow_h),
            label: "width: 50%\nheight: 1/3 of the red region",
            bg: faded_strip_color(Color::rgb(1.0, 1.0, 0.0)),
        },
        StripRegion {
            rect: Rect::new(gold_x, gold_y, 120.0_f64.min(w), 60.0_f64.min(lower_h)),
            label: "width: 120px\nheight: 60px",
            bg: faded_strip_color(Color::rgb(1.0, 0.84, 0.0)),
        },
        StripRegion {
            rect: Rect::new(green_x, green_y, 70.0_f64.min(w), green_h),
            label: "width: 70px\n\nheight: 50%, but at least 60px.",
            bg: faded_strip_color(Color::rgb(0.0, 1.0, 0.0)),
        },
    ]
}

/// Custom painter for the egui Strip demo's exact/remainder/relative regions.
struct StripDemoCanvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
}

impl StripDemoCanvas {
    fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
        }
    }
}

impl Widget for StripDemoCanvas {
    fn type_name(&self) -> &'static str {
        "StripDemoCanvas"
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
        let w = if available.width.is_finite() {
            available.width.max(360.0)
        } else {
            400.0
        };
        let h = if available.height.is_finite() {
            available.height.max(260.0)
        } else {
            320.0
        };
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(11.0);
        for region in strip_demo_regions(self.bounds.width, self.bounds.height, 14.0) {
            let r = region.rect;
            ctx.set_fill_color(region.bg);
            ctx.begin_path();
            ctx.rect(r.x, r.y, r.width, r.height);
            ctx.fill();
            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rect(r.x, r.y, r.width, r.height);
            ctx.stroke();

            ctx.set_fill_color(v.text_color);
            for (line_i, line) in region.label.lines().enumerate() {
                let y = r.y + r.height - 15.0 - line_i as f64 * 13.0;
                if y >= r.y {
                    ctx.fill_text(line, r.x + 6.0, y);
                }
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Strip demo — nested exact/remainder/relative strips matching egui.
pub fn strip_demo(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(StripDemoCanvas::new(font))
}

#[cfg(test)]
mod strip_tests {
    use super::strip_demo_regions;

    #[test]
    fn strip_regions_match_reference_shape() {
        let regions = strip_demo_regions(400.0, 300.0, 14.0);

        assert_eq!(regions.len(), 5);
        assert_rect(&regions[0].rect, 0.0, 250.0, 400.0, 50.0);
        assert_rect(&regions[1].rect, 0.0, 132.0, 200.0, 118.0);
        assert_rect(&regions[2].rect, 200.0, 171.333, 200.0, 39.333);
        assert_rect(&regions[3].rect, 105.0, 43.0, 120.0, 60.0);
        assert_rect(&regions[4].rect, 330.0, 43.0, 70.0, 60.0);
    }

    #[test]
    fn strip_regions_respect_exact_and_at_least_lower_limits() {
        let regions = strip_demo_regions(120.0, 80.0, 14.0);

        // Exact 120px + 70px horizontal regions must keep their required width
        // instead of collapsing/overlapping when the host asks for too little.
        assert!(regions.iter().all(|r| r.rect.width <= 190.0));
        assert_rect(&regions[3].rect, 0.0, 14.0, 120.0, 60.0);
        assert_rect(&regions[4].rect, 120.0, 14.0, 70.0, 60.0);
        assert!(
            regions[0].rect.height >= 50.0 && regions[4].rect.height >= 60.0,
            "exact and at-least strip heights must be preserved"
        );
    }

    fn assert_rect(rect: &agg_gui::Rect, x: f64, y: f64, w: f64, h: f64) {
        assert!((rect.x - x).abs() < 0.001, "x: {} != {}", rect.x, x);
        assert!((rect.y - y).abs() < 0.001, "y: {} != {}", rect.y, y);
        assert!((rect.width - w).abs() < 0.001, "w: {} != {}", rect.width, w);
        assert!(
            (rect.height - h).abs() < 0.001,
            "h: {} != {}",
            rect.height,
            h
        );
    }
}

// ---------------------------------------------------------------------------
// Table demo
// ---------------------------------------------------------------------------

/// Build the Table demo — a header row and 8 data rows with alternating colors.
pub fn table_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    outer.push(
        Box::new(Label::new("Simple data table", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    // Column widths.
    let col_w = [55.0_f64, 90.0, 70.0, 55.0];
    let headers = ["#", "Name", "Value", "Status"];

    // Header row.
    let mut header_row = FlexRow::new().with_gap(0.0);
    for (i, &hdr) in headers.iter().enumerate() {
        let cell = Container::new()
            .with_background(Color::rgba(0.0, 0.0, 0.0, 0.10))
            .with_border(Color::rgba(0.0, 0.0, 0.0, 0.15), 1.0)
            .with_padding(5.0)
            .add(Box::new(SizedBox::new().with_width(col_w[i]).with_child(
                Box::new(Label::new(hdr, Arc::clone(&font)).with_font_size(11.5)),
            )));
        header_row.push(Box::new(cell), 0.0);
    }
    outer.push(Box::new(header_row), 0.0);

    // Data rows.
    let data = [
        ("1", "Alpha", "0.92", "OK"),
        ("2", "Beta", "1.44", "OK"),
        ("3", "Gamma", "0.07", "Warn"),
        ("4", "Delta", "3.14", "OK"),
        ("5", "Epsilon", "2.72", "OK"),
        ("6", "Zeta", "0.00", "Error"),
        ("7", "Eta", "9.81", "OK"),
        ("8", "Theta", "1.618", "OK"),
    ];
    for (row_i, &(n, name, val, status)) in data.iter().enumerate() {
        let bg = if row_i % 2 == 0 {
            Color::rgba(0.0, 0.0, 0.0, 0.03)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.0)
        };
        let cells_text = [n, name, val, status];
        let mut data_row = FlexRow::new().with_gap(0.0);
        for (ci, &text) in cells_text.iter().enumerate() {
            let cell = Container::new()
                .with_background(bg)
                .with_border(Color::rgba(0.0, 0.0, 0.0, 0.08), 1.0)
                .with_padding(5.0)
                .add(Box::new(SizedBox::new().with_width(col_w[ci]).with_child(
                    Box::new(Label::new(text, Arc::clone(&font)).with_font_size(12.0)),
                )));
            data_row.push(Box::new(cell), 0.0);
        }
        outer.push(Box::new(data_row), 0.0);
    }

    outer.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(ScrollView::new(Box::new(outer)))
}
