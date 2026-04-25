//! Frame Demo — mirrors egui `FrameDemo`.
//!
//! Reproduces egui's Frame inspector:
//!   - `Inner margin`  : "same" checkbox + DragValue, expands to L/R/T/B when off
//!   - `Outer margin`  : same
//!   - `Corner radius` : "same" checkbox + DragValue, expands to NW/NE/SW/SE
//!   - `Shadow`        : x / y drag values (row 1), blur / spread (row 2), colour picker
//!   - `Fill`          : colour picker (with "No Color (Pass Through)")
//!   - `Stroke`        : width DragValue + colour picker
//!   - `Reset`         : restores every value to egui defaults.
//!
//! Layout:
//!   ┌──── controls ────┐  ┌── preview ──┐
//!   │ …control rows…   │  │  [frame]    │
//!   └──────────────────┘  └─────────────┘
//!
//! The parent `Window` is built with `.with_auto_size(true)` so expanding a
//! "same" checkbox or opening a colour picker grows the window downward.

#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::layout_props::{HAnchor, VAnchor};
use agg_gui::widget::paint_subtree;
use agg_gui::{
    Button, Checkbox, Color, ColorPicker, DragValue, DrawCtx, Event, EventResult, FlexColumn,
    FlexRow, Font, Label, Rect, Separator, Size, Widget,
};

mod core;
use core::{FourValueField, FramePreview, FrameState, IntrinsicRow, CONTROLS_W, FIELD_W, LABEL_W};

// ── Shadow editor — mini grid of 4 DragValues + colour picker ────────────────

fn shadow_editor(st: &Rc<FrameState>, font: Arc<Font>) -> Box<dyn Widget> {
    let dx_c = Rc::clone(&st.shadow_dx);
    let dy_c = Rc::clone(&st.shadow_dy);
    let bl_c = Rc::clone(&st.shadow_blur);
    let sp_c = Rc::clone(&st.shadow_spread);

    let dx = labeled_drag("x:", dx_c.clone(), -100.0, 100.0, 1.0, 0, Arc::clone(&font));
    let dy = labeled_drag("y:", dy_c.clone(), -100.0, 100.0, 1.0, 0, Arc::clone(&font));
    let bl = labeled_drag("blur:", bl_c.clone(), 0.0, 100.0, 1.0, 0, Arc::clone(&font));
    let sp = labeled_drag(
        "spread:",
        sp_c.clone(),
        0.0,
        100.0,
        1.0,
        0,
        Arc::clone(&font),
    );

    let col_cell = Rc::clone(&st.shadow_col);
    let col_pick = ColorPicker::new(col_cell, Arc::clone(&font))
        .with_allow_none(false)
        .with_font_size(12.0);

    // `add_flex(1.0)` so x & y (and blur & spread) share the row's width
    // equally instead of each one claiming the full width.
    let row1 = FlexRow::new()
        .with_gap(6.0)
        .add_flex(dx, 1.0)
        .add_flex(dy, 1.0);
    let row2 = FlexRow::new()
        .with_gap(6.0)
        .add_flex(bl, 1.0)
        .add_flex(sp, 1.0);
    let col = FlexColumn::new()
        .with_gap(4.0)
        .add(Box::new(row1))
        .add(Box::new(row2))
        .add(Box::new(col_pick));

    Box::new(col)
}

fn labeled_drag(
    prefix: &'static str,
    cell: Rc<Cell<f64>>,
    min: f64,
    max: f64,
    speed: f64,
    decimals: usize,
    font: Arc<Font>,
) -> Box<dyn Widget> {
    let c = Rc::clone(&cell);
    let row = FlexRow::new()
        .with_gap(4.0)
        .add(Box::new(
            Label::new(prefix, Arc::clone(&font))
                .with_font_size(12.0)
                .with_min_size(Size::new(40.0, 0.0))
                .with_max_size(Size::new(40.0, f64::MAX)),
        ))
        .add_flex(
            Box::new(
                DragValue::new(cell.get(), min, max, font)
                    .with_speed(speed)
                    .with_decimals(decimals)
                    .with_min_size(Size::new(60.0, 22.0))
                    .on_change(move |v| c.set(v)),
            ),
            1.0,
        );
    Box::new(row)
}

// ── Stroke editor: width + colour picker ─────────────────────────────────────

fn stroke_editor(st: &Rc<FrameState>, font: Arc<Font>) -> Box<dyn Widget> {
    let w_cell = Rc::clone(&st.stroke_w);
    let col_cell = Rc::clone(&st.stroke_col);
    let w_c = Rc::clone(&w_cell);
    // Width DragValue — max_size caps the preferred width so the FlexRow's
    // clamped_w math gives it a sane size instead of gobbling the full row.
    let dv = DragValue::new(w_cell.get(), 0.0, 20.0, Arc::clone(&font))
        .with_speed(0.1)
        .with_decimals(1)
        .with_min_size(Size::new(60.0, 22.0))
        .with_max_size(Size::new(70.0, f64::MAX))
        .on_change(move |v| w_c.set(v));
    let cp = ColorPicker::new(col_cell, Arc::clone(&font))
        .with_allow_none(false)
        .with_font_size(12.0);
    // Width on the left, colour picker (flex) takes the rest of the row.
    Box::new(
        FlexRow::new()
            .with_gap(6.0)
            .add(Box::new(dv))
            .add_flex(Box::new(cp), 1.0),
    )
}

// ── Full row: [label][field] ────────────────────────────────────────────────

fn labeled_row(label: &'static str, field: Box<dyn Widget>, font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(
        FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                Label::new(label, Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_min_size(Size::new(LABEL_W, 0.0))
                    .with_max_size(Size::new(LABEL_W, f64::MAX)),
            ))
            .add_flex(field, 1.0),
    )
}

fn field_row(f: Box<dyn Widget>) -> Box<dyn Widget> {
    Box::new(
        FlexRow::new()
            .add_flex(f, 1.0)
            .with_min_size(Size::new(FIELD_W, 0.0)),
    )
}

// ── Main builder ─────────────────────────────────────────────────────────────

/// Build the Frame demo content widget (public entry point).
pub fn frame_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let st = Rc::new(FrameState::defaults());

    // ── Left column: controls ───────────────────────────────────────────────
    let inner = FourValueField::new(
        &st.inner_m,
        ["Left", "Right", "Top", "Bottom"],
        Arc::clone(&font),
        0.0,
        100.0,
        1.0,
    );
    let outer = FourValueField::new(
        &st.outer_m,
        ["Left", "Right", "Top", "Bottom"],
        Arc::clone(&font),
        0.0,
        100.0,
        1.0,
    );
    let radius = FourValueField::new(
        &st.corner_r,
        ["NW", "NE", "SW", "SE"],
        Arc::clone(&font),
        0.0,
        100.0,
        1.0,
    );

    let fill_pick = ColorPicker::new(Rc::clone(&st.fill), Arc::clone(&font)).with_font_size(12.0);

    let st_reset = Rc::clone(&st);
    let reset = Button::new("Reset", Arc::clone(&font)).on_click(move || st_reset.reset());

    let controls = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_min_size(Size::new(CONTROLS_W, 0.0))
        .with_max_size(Size::new(CONTROLS_W, f64::MAX))
        .with_v_anchor(VAnchor::FIT)
        .add(labeled_row(
            "Inner margin",
            field_row(Box::new(inner)),
            Arc::clone(&font),
        ))
        .add(labeled_row(
            "Outer margin",
            field_row(Box::new(outer)),
            Arc::clone(&font),
        ))
        .add(labeled_row(
            "Corner radius",
            field_row(Box::new(radius)),
            Arc::clone(&font),
        ))
        .add(labeled_row(
            "Shadow",
            field_row(shadow_editor(&st, Arc::clone(&font))),
            Arc::clone(&font),
        ))
        .add(labeled_row(
            "Fill",
            field_row(Box::new(fill_pick)),
            Arc::clone(&font),
        ))
        .add(labeled_row(
            "Stroke",
            field_row(stroke_editor(&st, Arc::clone(&font))),
            Arc::clone(&font),
        ))
        .add(Box::new(reset));

    // ── Right column: live preview ──────────────────────────────────────────
    let preview = FramePreview {
        bounds: Rect::default(),
        children: Vec::new(),
        st: Rc::clone(&st),
        content: Label::new("Content", Arc::clone(&font))
            .with_font_size(13.0)
            .with_color(Color::white()),
    };

    // ── Main row ────────────────────────────────────────────────────────────
    //
    // `IntrinsicRow` reports the SUM of its children widths (not `available`),
    // so `Window::with_auto_size` can grow / shrink the window to match the
    // preview's outer_margin size.
    Box::new(IntrinsicRow::new(
        8.0,
        8.0,
        vec![
            Box::new(controls),
            Box::new(
                Separator::vertical()
                    .with_line_inset(0.0)
                    .with_min_size(Size::new(1.0, 0.0))
                    .with_max_size(Size::new(1.0, f64::MAX)),
            ),
            Box::new(preview),
        ],
    ))
}
