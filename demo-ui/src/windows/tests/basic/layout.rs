#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    set_cursor_icon, Color, Container, CursorIcon, DrawCtx, Event, EventResult, FlexColumn,
    FlexRow, Font, Label, Point, Rect, Separator, Size, SizedBox, TextField, Widget,
};

// ---------------------------------------------------------------------------
// Flex Layout Test
// ---------------------------------------------------------------------------
//
// The Input Test moved to `input_probe.rs` — it is now an interaction probe
// (custom Widgets with different `on_event`/`EventResult` consumption profiles)
// rather than a key/mouse status box.

/// Build the Flex Layout Test — colored boxes with alignment labels.
/// (Internal fn name stays `layout_test` for dispatch stability.)
pub fn layout_test(font: Arc<Font>) -> Box<dyn Widget> {
    let labels = ["Left", "Center", "Right", "Stretch"];
    let colors = [
        Color::rgba(0.22, 0.45, 0.88, 0.25),
        Color::rgba(0.18, 0.72, 0.42, 0.25),
        Color::rgba(0.88, 0.25, 0.18, 0.25),
        Color::rgba(0.86, 0.78, 0.40, 0.25),
    ];

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Alignment examples", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    for (i, (&lbl, &bg)) in labels.iter().zip(colors.iter()).enumerate() {
        let box_w = match i {
            0 => 80.0,
            1 => 120.0,
            2 => 100.0,
            _ => 0.0, // stretch — use flex
        };

        let cell = Container::new()
            .with_background(bg)
            .with_border(Color::rgba(0.0, 0.0, 0.0, 0.15), 1.0)
            .with_padding(6.0)
            .add(Box::new(
                Label::new(lbl, Arc::clone(&font)).with_font_size(12.0),
            ));

        if i == 3 {
            // Stretch row.
            let row = FlexRow::new().add_flex(Box::new(cell), 1.0);
            col.push(Box::new(row), 0.0);
        } else {
            let row = FlexRow::new().add(Box::new(
                SizedBox::new().with_width(box_w).with_child(Box::new(cell)),
            ));
            col.push(Box::new(row), 0.0);
        }
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "FlexRow / FlexColumn control alignment.\n\
         add() = fixed-size child, add_flex() = fills remaining space.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Manual Layout Test
// ---------------------------------------------------------------------------
//
// Mirrors egui's `tests/manual_layout_test.rs`: pick a widget type, then drive
// its position and size with sliders. A real, interactive widget of that type
// is placed at the chosen rect inside a bounded canvas via the `AbsolutePlace`
// container (see `absolute_place.rs`).

use std::cell::RefCell;

use agg_gui::{Button, RadioGroup, Rebuilder, Slider, TextArea};

use super::absolute_place::AbsolutePlace;

/// Build one labelled slider row: `label` on the left, two sliders (0..=400)
/// bound to `a`/`b` filling the rest — matching egui's two-slider Grid rows.
fn slider_pair_row(
    font: &Arc<Font>,
    label: &str,
    a: &Rc<Cell<f64>>,
    b: &Rc<Cell<f64>>,
) -> Box<dyn Widget> {
    let slider = |cell: &Rc<Cell<f64>>| -> Box<dyn Widget> {
        Box::new(
            SizedBox::new().with_height(28.0).with_child(Box::new(
                Slider::new(cell.get(), 0.0, 400.0, Arc::clone(font))
                    .with_step(1.0)
                    .with_decimals(0)
                    .with_value_cell(Rc::clone(cell)),
            )),
        )
    };
    Box::new(
        FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                SizedBox::new().with_width(110.0).with_child(Box::new(
                    Label::new(label, Arc::clone(font)).with_font_size(12.0),
                )),
            ))
            .add_flex(slider(a), 1.0)
            .add_flex(slider(b), 1.0),
    )
}

/// Build the placed widget for the current type selection.
/// 0 = Button, 1 = Label, 2 = TextEdit (matching the radio order).
fn build_placed_widget(
    font: &Arc<Font>,
    widget_type: usize,
    text: &Rc<RefCell<String>>,
) -> Box<dyn Widget> {
    match widget_type {
        1 => Box::new(Label::new("Example label", Arc::clone(font)).with_font_size(13.0)),
        2 => Box::new(
            TextArea::new(Arc::clone(font))
                .with_font_size(12.5)
                .with_text(text.borrow().clone())
                // Capture edits back into the shared cell so switching the
                // widget type away and back re-seeds the editor with the
                // user's mid-session text instead of the original default.
                .on_change({
                    let text = Rc::clone(text);
                    move |s| *text.borrow_mut() = s.to_string()
                }),
        ),
        _ => Box::new(
            Button::new("Example button", Arc::clone(font))
                .with_font_size(13.0)
                .on_click(|| agg_gui::animation::request_draw()),
        ),
    }
}

/// Build the Manual Layout Test — a real widget placed at an arbitrary rect.
pub fn manual_layout_test(font: Arc<Font>) -> Box<dyn Widget> {
    // Defaults mirror egui: offset (150,150), size (200,100), type = Button.
    let widget_type = Rc::new(Cell::new(0_usize)); // 0=Button 1=Label 2=TextEdit
    let x = Rc::new(Cell::new(150.0_f64));
    let y = Rc::new(Cell::new(150.0_f64));
    let w = Rc::new(Cell::new(200.0_f64));
    let h = Rc::new(Cell::new(100.0_f64));
    let text = Rc::new(RefCell::new(
        "Editable text — this widget is placed manually.".to_string(),
    ));

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg();

    // Reset — restores every control to its default (egui's reset_button).
    {
        let (rt, rwt, rx, ry, rw, rh) = (
            Rc::clone(&text),
            Rc::clone(&widget_type),
            Rc::clone(&x),
            Rc::clone(&y),
            Rc::clone(&w),
            Rc::clone(&h),
        );
        col.push(
            Box::new(
                SizedBox::new()
                    .with_width(80.0)
                    .with_height(28.0)
                    .with_child(Box::new(
                        Button::new("Reset", Arc::clone(&font))
                            .with_font_size(12.0)
                            .on_click(move || {
                                rwt.set(0);
                                rx.set(150.0);
                                ry.set(150.0);
                                rw.set(200.0);
                                rh.set(100.0);
                                *rt.borrow_mut() =
                                    "Editable text — this widget is placed manually.".to_string();
                                agg_gui::animation::request_draw();
                            }),
                    )),
            ),
            0.0,
        );
    }

    // Widget-type radio.
    {
        let wt = Rc::clone(&widget_type);
        let radio_row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                SizedBox::new().with_height(66.0).with_child(Box::new(
                    Label::new("Test widget:", Arc::clone(&font)).with_font_size(12.0),
                )),
            ))
            .add(Box::new(
                RadioGroup::new(vec!["Button", "Label", "TextEdit"], wt.get(), Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_selected_cell(Rc::clone(&widget_type))
                    .on_change(move |i| wt.set(i)),
            ));
        col.push(Box::new(radio_row), 0.0);
    }

    // Position + size slider rows.
    col.push(slider_pair_row(&font, "Widget position:", &x, &y), 0.0);
    col.push(slider_pair_row(&font, "Widget size:", &w, &h), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Bounded canvas holding the placed widget. The Rebuilder swaps the child
    // when the widget type changes; position/size are read live each frame so
    // dragging the sliders re-places the child without a rebuild.
    let version = {
        let widget_type = Rc::clone(&widget_type);
        move || widget_type.get() as u64
    };
    let builder = {
        let font = Arc::clone(&font);
        let widget_type = Rc::clone(&widget_type);
        let text = Rc::clone(&text);
        let (x, y, w, h) = (Rc::clone(&x), Rc::clone(&y), Rc::clone(&w), Rc::clone(&h));
        move || {
            let child = build_placed_widget(&font, widget_type.get(), &text);
            Box::new(AbsolutePlace::new(
                child,
                Rc::clone(&x),
                Rc::clone(&y),
                Rc::clone(&w),
                Rc::clone(&h),
            )) as Box<dyn Widget>
        }
    };
    col.push(Box::new(Rebuilder::new(version, builder)), 1.0);

    Box::new(col)
}
