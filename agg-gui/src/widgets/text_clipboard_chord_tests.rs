//! Guard: clipboard chords follow agg-sharp, not egui. agg-sharp's
//! `InternalTextEditWidget.OnKeyDown` (`Gui/TextWidgets/
//! InternalTextEditWidget.cs`, the `Keys.X` / `Keys.C` / `Keys.V` cases)
//! cuts, copies and pastes on any Control chord, whatever Shift / Alt
//! are doing, and marks the key handled. egui #8668 instead lets
//! Cmd+Shift+V etc. through, which in MatterCAD would let Ctrl+Shift+X in a
//! text field reach the design's own key commands and cut the selected
//! part. These tests pin the agg-sharp behaviour so a future egui re-port
//! can't silently drop it.
//!
//! Drives real `KeyDown` events through `TextField` and `TextArea` against a
//! simulated clipboard, with the thread pinned to the Mac and then the
//! Windows platform. A child module of `text_key_bindings`.

use std::sync::Arc;

use crate::clipboard;
use crate::event::{Event, EventResult, Key, Modifiers};
use crate::geometry::Size;
use crate::platform::{command_modifiers, override_platform_for_thread, Platform};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::{TextArea, TextField};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn chord(w: &mut dyn Widget, c: char, modifiers: Modifiers) -> EventResult {
    w.on_event(&Event::KeyDown {
        key: Key::Char(c),
        modifiers,
    })
}

/// With the editor holding `"ab"`, all selected: command+Shift+C copies it
/// and command+Shift+V pastes over it, each consuming the key.
fn check_shifted_copy_and_paste<W: Widget>(make: impl Fn() -> W, text: impl Fn(&W) -> String) {
    for platform in [Platform::MacOS, Platform::Windows] {
        let _platform = override_platform_for_thread(platform);
        let _clipboard = clipboard::simulate();
        let cmd = command_modifiers();
        let shifted = Modifiers { shift: true, ..cmd };
        let mut w = make();
        chord(&mut w, 'a', cmd);

        clipboard::set_text("clip");
        assert!(
            chord(&mut w, 'c', shifted).is_consumed(),
            "{platform:?}: command+Shift+C is consumed"
        );
        assert_eq!(
            clipboard::get_text().as_deref(),
            Some("ab"),
            "{platform:?}: command+Shift+C copies"
        );

        clipboard::set_text("clip");
        assert!(
            chord(&mut w, 'v', shifted).is_consumed(),
            "{platform:?}: command+Shift+V is consumed"
        );
        assert_eq!(text(&w), "clip", "{platform:?}: command+Shift+V pastes");
    }
}

#[test]
fn text_field_shifted_clipboard_chords_copy_and_paste() {
    check_shifted_copy_and_paste(
        || {
            let mut f = TextField::new(font()).with_text("ab");
            f.layout(Size::new(400.0, 32.0));
            f.on_event(&Event::FocusGained);
            f
        },
        TextField::text,
    );
}

#[test]
fn text_area_shifted_clipboard_chords_copy_and_paste() {
    check_shifted_copy_and_paste(
        || {
            let mut ta = TextArea::new(font()).with_text("ab");
            ta.layout(Size::new(400.0, 120.0));
            ta.on_event(&Event::FocusGained);
            ta
        },
        TextArea::text,
    );
}
