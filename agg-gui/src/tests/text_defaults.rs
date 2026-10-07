//! App-wide text defaults: `font_settings::set_default_font_size` and the
//! one-em `LineBox::Em` line box.
//!
//! Default behaviour must stay exactly as before (Label 14 px at 1.5 em per
//! line, TextField 2.4 em / 28 px, menus 14 px) so other consumers are
//! unaffected; opting in gives agg-sharp's sizing (`TextWidget` one em tall,
//! `TextEditWidget` one em of content plus its padding).

use super::*;

use crate::font_settings::{clear_default_font_size, set_default_font_size, set_line_box, LineBox};
use crate::text::Font;
use crate::widgets::menu::effective_metrics;
use crate::widgets::Label;
use std::sync::Arc;

fn font() -> Arc<Font> {
    Arc::new(Font::from_bytes(TEST_FONT.to_vec()).expect("test font"))
}

fn prop(w: &dyn Widget, name: &str) -> String {
    w.properties()
        .into_iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

/// Big enough that no widget is clamped by the available space.
const ROOM: Size = Size {
    width: 400.0,
    height: 400.0,
};

#[test]
fn defaults_unchanged_without_overrides() {
    clear_default_font_size();
    set_line_box(LineBox::Standard);
    let f = font();

    let mut label = Label::new("Ag", Arc::clone(&f));
    assert_eq!(prop(&label, "font_size"), "14.0");
    assert_eq!(label.layout(ROOM).height, 14.0 * 1.5);

    let button = Button::new("OK", Arc::clone(&f));
    assert_eq!(prop(&button, "font_size"), "14.0");

    let mut field = TextField::new(Arc::clone(&f));
    assert_eq!(field.layout(ROOM).height, (14.0_f64 * 2.4).max(28.0));

    assert_eq!(effective_metrics().default_font_size, 14.0);
}

#[test]
fn default_font_size_applies_to_widgets_built_after_it() {
    let f = font();
    set_default_font_size(12.0);

    let mut label = Label::new("Ag", Arc::clone(&f));
    assert_eq!(prop(&label, "font_size"), "12.0");
    assert_eq!(label.layout(ROOM).height, 12.0 * 1.5);

    let button = Button::new("OK", Arc::clone(&f));
    assert_eq!(prop(&button, "font_size"), "12.0");

    let mut field = TextField::new(Arc::clone(&f));
    assert_eq!(field.layout(ROOM).height, (12.0_f64 * 2.4).max(28.0));

    assert_eq!(effective_metrics().default_font_size, 12.0);

    // An explicit size still wins over the app-wide default.
    let explicit = Label::new("Ag", Arc::clone(&f)).with_font_size(20.0);
    assert_eq!(prop(&explicit, "font_size"), "20.0");

    clear_default_font_size();
    let after = Label::new("Ag", f);
    assert_eq!(prop(&after, "font_size"), "14.0");
}

#[test]
fn em_line_box_makes_label_one_em_tall() {
    clear_default_font_size();
    let f = font();
    set_line_box(LineBox::Em);
    let mut label = Label::new("Ag", Arc::clone(&f)).with_font_size(16.0);
    assert_eq!(label.layout(ROOM).height, 16.0);
    assert_eq!(label.measure_min_height(ROOM.width), 16.0);

    // Wrapped: one em per line (agg-sharp's line spacing is 1).
    let mut wrapped = Label::new("one two three four five six", Arc::clone(&f))
        .with_font_size(16.0)
        .with_wrap(true);
    let lines = (wrapped.layout(Size::new(60.0, 400.0)).height / 16.0).round();
    assert!(lines >= 2.0, "expected wrapping, got {lines} lines");
    assert_eq!(wrapped.layout(Size::new(60.0, 400.0)).height, lines * 16.0);

    // A per-widget line box overrides the global one.
    let mut pinned = Label::new("Ag", Arc::clone(&f))
        .with_font_size(16.0)
        .with_line_box(LineBox::Standard);
    assert_eq!(pinned.layout(ROOM).height, 16.0 * 1.5);

    set_line_box(LineBox::Standard);
    let mut standard = Label::new("Ag", Arc::clone(&f)).with_font_size(16.0);
    assert_eq!(standard.layout(ROOM).height, 16.0 * 1.5);
    let mut opted = Label::new("Ag", f)
        .with_font_size(16.0)
        .with_line_box(LineBox::Em);
    assert_eq!(opted.layout(ROOM).height, 16.0);
}

#[test]
fn em_line_box_makes_text_field_one_em_plus_padding() {
    clear_default_font_size();
    let f = font();
    set_line_box(LineBox::Em);
    let mut field = TextField::new(Arc::clone(&f))
        .with_font_size(12.0)
        .with_padding(3.0);
    assert_eq!(field.layout(ROOM).height, 12.0 + 2.0 * 3.0);

    let mut pinned = TextField::new(Arc::clone(&f))
        .with_font_size(12.0)
        .with_line_box(LineBox::Standard);
    assert_eq!(pinned.layout(ROOM).height, (12.0_f64 * 2.4).max(28.0));

    set_line_box(LineBox::Standard);
    let mut opted = TextField::new(f)
        .with_font_size(12.0)
        .with_padding(3.0)
        .with_line_box(LineBox::Em);
    assert_eq!(opted.layout(ROOM).height, 18.0);
}
