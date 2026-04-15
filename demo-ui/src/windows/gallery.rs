//! Widget Gallery demo window.
//!
//! Displays one representative instance of every interactive widget in the
//! agg-gui library: buttons (primary / secondary / danger), checkboxes, a
//! radio group, a combo box, slider, progress bar, toggle switches, drag
//! values, a hyperlink, and a text input field.
//!
//! Section headers use `Label` without an explicit color so they follow the
//! active theme (`ctx.visuals().text_color`) and remain readable in both dark
//! and light mode.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, Color, ComboBox, DragValue,
    FlexColumn, FlexRow, Font, Hyperlink, Label, ProgressBar, RadioGroup,
    ScrollView, Separator, SizedBox, Slider, TextField,
    ToggleSwitch, Widget,
};
use agg_gui::widgets::button::ButtonTheme;

/// Build the Widget Gallery demo — a scrollable showcase of all interactive
/// widgets with section headers.
pub fn widget_gallery(font: Arc<Font>) -> Box<dyn Widget> {
    let slider_val = Rc::new(Cell::new(0.42_f64));
    let cb1        = Rc::new(Cell::new(true));
    let cb2        = Rc::new(Cell::new(false));
    let radio_sel  = Rc::new(Cell::new(0_usize));
    let combo_sel  = Rc::new(Cell::new(0_usize));

    /// Section header — no explicit color so it inherits the active theme.
    fn section(text: &str, font: &Arc<Font>) -> Box<dyn Widget> {
        Box::new(Label::new(text, Arc::clone(font)).with_font_size(12.0))
    }

    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    // ── Buttons ───────────────────────────────────────────────────────────────
    col.push(section("Buttons", &font), 0.0);
    let row = FlexRow::new().with_gap(8.0)
        .add(Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            Button::new("Primary", Arc::clone(&font)).with_font_size(12.0).on_click(|| {})
        ))))
        .add(Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            Button::new("Secondary", Arc::clone(&font)).with_font_size(12.0)
                .with_theme(ButtonTheme {
                    background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                    background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                    background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                    label_color:        Color::rgb(0.22, 0.45, 0.88),
                    border_radius:      6.0,
                    focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                    focus_ring_width:   2.5,
                }).on_click(|| {})
        ))))
        .add(Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            Button::new("Danger", Arc::clone(&font)).with_font_size(12.0)
                .with_theme(ButtonTheme {
                    background:         Color::rgb(0.88, 0.25, 0.18),
                    background_hovered: Color::rgb(0.95, 0.32, 0.24),
                    background_pressed: Color::rgb(0.72, 0.18, 0.12),
                    label_color:        Color::white(),
                    border_radius:      6.0,
                    focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                    focus_ring_width:   2.5,
                }).on_click(|| {})
        ))));
    col.push(Box::new(row), 0.0);

    // ── Checkboxes ────────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Checkboxes", &font), 0.0);
    { let v = Rc::clone(&cb1);
      col.push(Box::new(Checkbox::new("Enable feature A", Arc::clone(&font), cb1.get())
          .with_font_size(13.0).on_change(move |v2| v.set(v2))), 0.0); }
    { let v = Rc::clone(&cb2);
      col.push(Box::new(Checkbox::new("Enable feature B", Arc::clone(&font), cb2.get())
          .with_font_size(13.0).on_change(move |v2| v.set(v2))), 0.0); }

    // ── Radio ─────────────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Radio buttons", &font), 0.0);
    { let rs = Rc::clone(&radio_sel);
      col.push(Box::new(RadioGroup::new(
          vec!["Option A", "Option B", "Option C"],
          radio_sel.get(), Arc::clone(&font),
      ).with_font_size(13.0).on_change(move |i| rs.set(i))), 0.0); }

    // ── ComboBox ──────────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Combo box", &font), 0.0);
    { let cs = Rc::clone(&combo_sel);
      col.push(Box::new(
          ComboBox::new(
              vec!["Spring", "Summer", "Autumn", "Winter"],
              combo_sel.get(),
              Arc::clone(&font),
          ).on_change(move |i| cs.set(i))
      ), 0.0); }

    // ── Slider ────────────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Slider", &font), 0.0);
    { let sv = Rc::clone(&slider_val);
      col.push(Box::new(Slider::new(slider_val.get(), 0.0, 1.0, Arc::clone(&font))
          .with_step(0.01).on_change(move |v| sv.set(v))), 0.0); }

    // ── Progress bar ──────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Progress bar (tracks slider)", &font), 0.0);
    col.push(Box::new(ProgressBar::new(slider_val.get(), Arc::clone(&font))), 0.0);

    // ── Toggle switch ─────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Toggle switch", &font), 0.0);
    {
        let ts1 = Rc::new(Cell::new(true));
        let ts2 = Rc::new(Cell::new(false));
        let row = FlexRow::new().with_gap(16.0)
            .add(Box::new(ToggleSwitch::new(ts1.get()).with_state_cell(Rc::clone(&ts1))))
            .add(Box::new(Label::new("Enabled", Arc::clone(&font)).with_font_size(13.0)))
            .add(Box::new(ToggleSwitch::new(ts2.get()).with_state_cell(Rc::clone(&ts2))))
            .add(Box::new(Label::new("Disabled", Arc::clone(&font)).with_font_size(13.0)));
        col.push(Box::new(row), 0.0);
    }

    // ── Drag value ────────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Drag value", &font), 0.0);
    {
        let dv1 = Rc::new(Cell::new(42.0_f64));
        let dv2 = Rc::new(Cell::new(3.14_f64));
        let row = FlexRow::new().with_gap(8.0)
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(28.0).with_child({
                let v = Rc::clone(&dv1);
                Box::new(DragValue::new(dv1.get(), 0.0, 100.0, Arc::clone(&font))
                    .with_decimals(0).on_change(move |x| v.set(x)))
            })))
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(28.0).with_child({
                let v = Rc::clone(&dv2);
                Box::new(DragValue::new(dv2.get(), 0.0, 10.0, Arc::clone(&font))
                    .with_decimals(2).on_change(move |x| v.set(x)))
            })));
        col.push(Box::new(row), 0.0);
    }

    // ── Hyperlink ─────────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Hyperlink", &font), 0.0);
    col.push(Box::new(
        Hyperlink::new("Visit the agg-gui repository", Arc::clone(&font))
            .with_font_size(13.0)
            .on_click(|| {})
    ), 0.0);

    // ── Text input ────────────────────────────────────────────────────────────
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(section("Text input", &font), 0.0);
    col.push(Box::new(SizedBox::new().with_height(32.0).with_child(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(13.0).with_placeholder("Type something…")
    ))), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}
