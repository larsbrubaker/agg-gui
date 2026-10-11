//! agg-sharp `Tests/Agg.Tests/Agg/TypeFacePrinterLineSpacingTests.cs`:
//! `WrappedTextWidgetKeepsItsSpacingAcrossTextChanges`, against agg-gui's
//! wrapped `Label` (C# `WrappedTextWidget`, whose `TextWidget` rebuilds its
//! printer on every text change). The C# `{ Width = 60 }` is a layout at 60
//! logical units; the label uses the one-em line box of agg-sharp's
//! `TextWidget`. The class's two `TypeFacePrinter` tests cover the C# glyph
//! printer, which agg-gui has no counterpart of, and are not in this batch.

use std::sync::Arc;

use crate::font_settings::LineBox;
use crate::text::Font;
use crate::widgets::Label;
use crate::{Size, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn wrapped(text: &str) -> Label {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    Label::new(text, font)
        .with_font_size(12.0)
        .with_line_box(LineBox::Em)
        .with_wrap(true)
}

#[test]
fn wrapped_text_widget_keeps_its_spacing_across_text_changes() {
    // TextWidget rebuilds its printer on every text change, so the spacing has to be carried across.
    let mut single = wrapped("one two three four five six seven eight nine ten");
    let mut spaced = wrapped("one two three four five six seven eight nine ten");
    spaced.set_line_spacing(1.5);
    spaced.set_text("ten nine eight seven six five four three two one");

    let single_height = single.layout(Size::new(60.0, 1000.0)).height;
    let spaced_height = spaced.layout(Size::new(60.0, 1000.0)).height;

    assert_eq!(single.line_spacing(), 1.0);
    assert_eq!(spaced.line_spacing(), 1.5);
    assert!(
        spaced_height > single_height * 1.3,
        "spaced {spaced_height} vs single {single_height}"
    );
}
