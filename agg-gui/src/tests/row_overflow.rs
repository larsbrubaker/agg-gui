//! Widgets in a row never overlap or run out of the row: a `FlexRow` whose
//! fixed children are wider than it shrinks the ones that can shrink (an
//! ellipsizing `Label`), each child paints clipped to its own column, and a
//! content-fitted `ComboBox` (agg-sharp `DropDownList`, `HAnchor.Fit`) takes
//! only its widest option's width. Also: an ellipsized label tips its full
//! text through the App's real hover path, and a `ModalSheet` panel never
//! shrinks below its minimum size.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::widgets::tooltip::controller;
use crate::widgets::tooltip::{reset_tooltip_test_state, tooltip_timings};
use crate::widgets::{Label, ModalSheet};
use crate::{DrawCtx, Event, EventResult};

const LONG: &str = "A rather long design name that cannot fit.mcx";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn assert_in_order_inside(row: &dyn Widget, width: f64) {
    let mut right = 0.0;
    for (i, child) in row.children().iter().enumerate() {
        let b = child.bounds();
        assert!(
            b.x >= right - 0.5,
            "child {i} at {b:?} overlaps the one before (ends {right})"
        );
        right = b.x + b.width;
    }
    assert!(
        right <= width + 0.5,
        "the last child ends at {right}, past the {width} row"
    );
}

#[test]
fn a_crowded_row_shrinks_its_ellipsis_label_so_nothing_overlaps() {
    let mut row = FlexRow::new()
        .with_gap(4.0)
        .add(Box::new(
            Label::new(LONG, font()).with_ellipsis_if_clipped(true),
        ))
        .add(Box::new(SizedBox::fixed(40.0, 20.0)))
        .add(Box::new(SizedBox::fixed(40.0, 20.0)));
    row.set_bounds(Rect::new(0.0, 0.0, 200.0, 24.0));
    row.layout(Size::new(200.0, 24.0));
    assert_in_order_inside(&row, 200.0);
    // The label took the overflow: the buttons keep their size.
    assert_eq!(row.children()[1].bounds().width, 40.0);
    assert_eq!(row.children()[2].bounds().width, 40.0);
    assert_eq!(row.children()[0].bounds().width, 200.0 - 80.0 - 8.0);
    let label = row.children()[0]
        .as_any()
        .unwrap()
        .downcast_ref::<Label>()
        .unwrap();
    assert!(label.ellipsis_active());
    assert!(label.shown_text().ends_with("..."));
}

#[test]
fn shrinking_stops_at_the_label_min_width() {
    let mut row = FlexRow::new()
        .with_gap(0.0)
        .add(Box::new(
            Label::new(LONG, font())
                .with_ellipsis_if_clipped(true)
                .with_min_size(Size::new(50.0, 0.0)),
        ))
        .add(Box::new(SizedBox::fixed(120.0, 20.0)));
    row.layout(Size::new(100.0, 24.0));
    assert_eq!(row.children()[0].bounds().width, 50.0);
    // The rigid box still starts after the label; the row clips what's past it.
    assert_eq!(row.children()[1].bounds().x, 50.0);
}

#[test]
fn a_row_that_fits_does_not_shrink_anything() {
    let mut row = FlexRow::new()
        .with_gap(4.0)
        .add(Box::new(
            Label::new("Cube", font()).with_ellipsis_if_clipped(true),
        ))
        .add(Box::new(SizedBox::fixed(40.0, 20.0)));
    row.layout(Size::new(400.0, 24.0));
    let label = row.children()[0]
        .as_any()
        .unwrap()
        .downcast_ref::<Label>()
        .unwrap();
    assert!(!label.ellipsis_active());
    assert_eq!(label.shown_text(), "Cube");
}

#[test]
fn a_label_without_ellipsis_does_not_shrink() {
    let mut label = Label::new(LONG, font());
    assert_eq!(label.shrink_min_width(), None);
    label = label.with_ellipsis_if_clipped(true);
    assert_eq!(label.shrink_min_width(), Some(0.0));
}

/// A leaf that paints a red block `overhang` px wider than its own bounds.
struct Overhang {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    overhang: f64,
    color: Color,
}

impl Widget for Overhang {
    fn type_name(&self) -> &'static str {
        "Overhang"
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
        Size::new(20.0, 20.0)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rect(
            0.0,
            0.0,
            self.bounds.width + self.overhang,
            self.bounds.height,
        );
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn overhang_row(clip: bool) -> FlexRow {
    let red = Overhang {
        bounds: Rect::default(),
        children: Vec::new(),
        overhang: 40.0,
        color: Color::rgb(1.0, 0.0, 0.0),
    };
    let white = Overhang {
        bounds: Rect::default(),
        children: Vec::new(),
        overhang: 0.0,
        color: Color::rgb(1.0, 1.0, 1.0),
    };
    FlexRow::new()
        .with_gap(10.0)
        .with_clip_children_to_slots(clip)
        .add(Box::new(red))
        .add(Box::new(SizedBox::fixed(20.0, 20.0)))
        .add(Box::new(white))
}

fn paint_row(mut row: FlexRow) -> Framebuffer {
    row.layout(Size::new(100.0, 20.0));
    row.set_bounds(Rect::new(0.0, 0.0, 100.0, 20.0));
    let mut fb = Framebuffer::new(100, 20);
    let mut ctx = GfxCtx::new(&mut fb);
    crate::widget::paint_subtree(&mut row, &mut ctx);
    drop(ctx);
    fb
}

#[test]
fn a_child_that_paints_past_its_box_does_not_paint_over_its_sibling() {
    let fb = paint_row(overhang_row(true));
    // The red child spans 0..20; its 40 px overhang would reach the gap and
    // the next slot (30..50). Only its own column is red.
    assert!(is_red(sample(&fb, 10, 10)));
    assert!(
        !is_red(sample(&fb, 25, 10)),
        "the overhang painted into the gap"
    );
    assert!(
        !is_red(sample(&fb, 40, 10)),
        "the overhang painted over its sibling"
    );
    assert!(is_white(sample(&fb, 70, 10)));
}

#[test]
fn a_row_can_opt_out_of_slot_clipping() {
    let fb = paint_row(overhang_row(false));
    assert!(is_red(sample(&fb, 40, 10)));
}

#[test]
fn a_fit_width_combo_box_is_as_wide_as_its_widest_option() {
    let options = vec!["Release", "Pre-Release", "Development"];
    let mut fitted = ComboBox::new(options.clone(), 0, font())
        .with_font_size(14.0)
        .with_fit_width(true);
    let mut full = ComboBox::new(options, 0, font()).with_font_size(14.0);
    let fitted_w = fitted.layout(Size::new(600.0, 30.0)).width;
    assert_eq!(full.layout(Size::new(600.0, 30.0)).width, 600.0);
    assert!(fitted_w < 600.0);
    // Widest text plus the 8 px side padding and the 20 px arrow.
    let widest = Label::new("Development", font())
        .with_font_size(14.0)
        .layout(Size::new(f64::MAX, 30.0))
        .width;
    assert_eq!(fitted_w, (widest + 36.0).ceil());
    assert_eq!(fitted.bounds().width, fitted_w);
    // Never wider than offered.
    assert_eq!(fitted.layout(Size::new(50.0, 30.0)).width, 50.0);
}

#[test]
fn a_fit_width_combo_box_leaves_room_for_its_row_neighbours() {
    let mut row = FlexRow::new()
        .with_gap(4.0)
        .add(Box::new(Label::new("Update Channel", font())))
        .add_flex(Box::new(SizedBox::new()), 1.0)
        .add(Box::new(
            ComboBox::new(vec!["Release", "Pre-Release", "Development"], 0, font())
                .with_fit_width(true),
        ));
    row.layout(Size::new(400.0, 30.0));
    assert_in_order_inside(&row, 400.0);
    let combo = row.children()[2].bounds();
    assert!(
        (combo.x + combo.width - 400.0).abs() < 1.0,
        "{combo:?} not at the row's end"
    );
}

#[test]
fn an_ellipsized_label_tips_its_full_text() {
    let mut label = Label::new(LONG, font()).with_ellipsis_if_clipped(true);
    let size = label.layout(Size::new(60.0, 20.0));
    label.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    assert_eq!(label.tooltip_text(), Some(LONG));
    // Its own tip wins.
    label.set_tooltip_text(Some("Rename".into()));
    assert_eq!(label.tooltip_text(), Some("Rename"));
    // A label that fits has no tip.
    let mut short = Label::new("Cube", font()).with_ellipsis_if_clipped(true);
    let size = short.layout(Size::new(600.0, 20.0));
    short.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    assert_eq!(short.tooltip_text(), None);
}

#[test]
fn hovering_an_ellipsized_label_in_a_row_shows_its_full_text() {
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
    reset_tooltip_test_state();
    controller::reset();
    crate::clock::start_virtual();
    crate::font_settings::set_system_font(Some(font()));
    let row = FlexRow::new()
        .add(Box::new(
            Label::new(LONG, font()).with_ellipsis_if_clipped(true),
        ))
        .add(Box::new(SizedBox::fixed(40.0, 20.0)));
    let mut app = App::new(Box::new(row));
    app.layout(Size::new(150.0, 30.0));
    let label = app.root().children()[0].bounds();
    let p = Point::new(label.x + 10.0, label.y + label.height * 0.5);
    app.on_mouse_move(p.x, 30.0 - p.y);
    app.update_tooltips_for_test();
    crate::clock::advance(tooltip_timings().initial_delay);
    app.update_tooltips_for_test();
    let shown = controller::visible_text();
    reset_tooltip_test_state();
    controller::reset();
    crate::font_settings::set_system_font(None);
    assert_eq!(shown.as_deref(), Some(LONG));
}

#[test]
fn a_modal_sheet_panel_never_shrinks_below_its_minimum() {
    let visible = Rc::new(Cell::new(true));
    let mut sheet = ModalSheet::new(Rc::clone(&visible), Box::new(SizedBox::new()))
        .with_panel_size(Size::new(600.0, 400.0))
        .with_min_panel_size(Size::new(500.0, 350.0));
    sheet.layout(Size::new(300.0, 200.0));
    // Centred on the host and overhanging it, at its minimum size.
    assert_eq!(
        sheet.children()[0].bounds(),
        Rect::new(-100.0, -75.0, 500.0, 350.0)
    );
    // With room, the panel keeps its desired size.
    sheet.layout(Size::new(1000.0, 800.0));
    assert_eq!(sheet.children()[0].bounds().width, 600.0);
    // The host can raise the minimum after building the sheet.
    sheet.set_min_panel_size(Size::new(700.0, 0.0));
    sheet.layout(Size::new(1000.0, 800.0));
    assert_eq!(sheet.children()[0].bounds().width, 700.0);
}
