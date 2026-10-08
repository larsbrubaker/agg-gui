//! `TextEditTests.TextEditingSpecialKeysWork`: the word keys (C#'s
//! `IndexOfNextToken`/`IndexOfPreviousToken`, agg-gui's
//! `widgets::text_caret_navigation`), word selection with Shift+Control,
//! Delete, and copy and paste through a simulated clipboard.
//!
//! C#'s `TextEditWidget("some starting text")` sizes itself to its text;
//! here the field is as wide as that text at 12 points. C#'s
//! `TopLeftOffset.Y == 0` (the field has not scrolled vertically) has no
//! counterpart: an agg-gui `TextField` scrolls only horizontally.
//! `SendKeyDown(Keys.Control)`/`SendKeyDown(Keys.Shift)` press a modifier on
//! its own, which agg-gui receives as a modifier change.

use std::sync::Arc;

use agg_gui::clipboard;
use agg_gui::event::{Key, Modifiers};
use agg_gui::widgets::text_caret_navigation::{index_of_next_token, index_of_previous_token};

use super::{
    container_with, control, selection, send_key, set_text, text, text_edit_widget, POINT_SIZE,
};

fn shift_control() -> Modifiers {
    Modifiers {
        shift: true,
        ctrl: true,
        ..Modifiers::default()
    }
}

#[test]
fn text_editing_special_keys_work() {
    // C#'s WindowsKeyBindings scope: Control+Arrow moves by word.
    let edit = "textEdit";
    let start = "some starting text";
    let font = Arc::new(agg_gui::fonts::standard_ui_font());
    let width = agg_gui::text::measure_advance(&font, start, POINT_SIZE);
    let field = text_edit_widget(edit, start, 0.0, 0.0, width);
    let height = agg_gui::Widget::bounds(&field).height;
    let mut container = container_with(200.0, 200.0, vec![field]);
    let c = &mut container;

    super::down(c, 1, 1.0, height - 1.0);
    super::up(c, 1.0, height - 1.0);

    assert!(super::char_index_to_insert_before(c, edit) == 0);

    // test that we move to the next character correctly
    assert_eq!(index_of_next_token("235 12/6", 0), 4);
    assert_eq!(index_of_next_token("235   12/6", 0), 6);
    assert_eq!(index_of_next_token("235\n   12/6", 0), 3);
    assert_eq!(index_of_next_token("235\n   12/6", 3), 7);
    assert_eq!(index_of_next_token("235\n\n   12/6", 3), 4);
    assert_eq!(index_of_next_token("235\n\n   12/6", 4), 8);
    assert_eq!(index_of_next_token("123+ 235   12/6", 0), 3);
    assert_eq!(index_of_next_token("235+12/6", 0), 3);
    assert_eq!(index_of_next_token("+++++235   12/6", 0), 5);
    assert_eq!(index_of_next_token("+++++235   12/6", 0), 5);

    // test that we move to the previous character correctly
    assert_eq!(index_of_previous_token("=35+12/6", 8), 7);
    assert_eq!(index_of_previous_token("35556+68384734", 10), 6);
    assert_eq!(index_of_previous_token("35556+68384734", 6), 5);
    assert_eq!(index_of_previous_token("35556+68384734", 5), 0);

    assert_eq!(index_of_previous_token("235\n\n   12/6", 12), 11);
    assert_eq!(index_of_previous_token("235\n\n   12/6", 11), 10);
    assert_eq!(index_of_previous_token("235\n\n   12/6", 10), 8);
    assert_eq!(index_of_previous_token("235\n\n   12/6", 8), 5);
    assert_eq!(index_of_previous_token("235\n\n   12/6", 5), 4);
    assert_eq!(index_of_previous_token("235\n\n   12/6", 4), 0);
    assert_eq!(index_of_previous_token("some starting text", 5), 0);

    let run_with_specific_char = |c: &mut agg_gui_automation::HeadlessWindow,
                                  sep: &str,
                                  first: &str,
                                  second: &str,
                                  third: &str| {
        let start_text = format!("{first}{sep}{second}{sep}{third}");
        assert!(text(c, edit) == start_text);
        // this is to select some text
        send_key(c, Key::ArrowRight, shift_control());
        assert!(selection(c, edit) == format!("{first}{sep}"));
        assert!(text(c, edit) == start_text);
        // this is to prove that we don't loose the selection when pressing Control
        c.set_modifiers(control());
        assert!(selection(c, edit) == format!("{first}{sep}"));
        assert!(text(c, edit) == start_text);
        // this is to prove that we don't loose the selection when pressing Shift
        c.set_modifiers(super::shift());
        assert!(text(c, edit) == start_text);
        assert!(selection(c, edit) == format!("{first}{sep}"));
        c.set_modifiers(Modifiers::default());
        send_key(c, Key::ArrowRight, Modifiers::default());
        assert!(selection(c, edit).is_empty());
        send_key(c, Key::ArrowLeft, shift_control());
        assert!(selection(c, edit) == format!("{first}{sep}"));
        send_key(c, Key::Delete, Modifiers::default());
        assert!(text(c, edit) == format!("{second}{sep}{third}"));
        send_key(c, Key::ArrowRight, shift_control());
        assert!(selection(c, edit) == format!("{second}{sep}"));

        // Copy and paste go through whatever clipboard the process installed, and this test must not
        // leave the real one holding its scratch text - so it swaps in a simulated one and puts back
        // exactly what was there.
        let installed_clipboard = clipboard::simulate();

        c.on_key_down(Key::Char('c'), control());
        assert!(selection(c, edit) == format!("{second}{sep}"));
        assert!(text(c, edit) == format!("{second}{sep}{third}"));
        send_key(c, Key::ArrowRight, Modifiers::default()); // move to the right
        c.on_key_down(Key::Char('v'), control());
        assert!(text(c, edit) == format!("{second}{sep}{second}{sep}{third}"));

        drop(installed_clipboard);
    };

    let check_char = |c: &mut agg_gui_automation::HeadlessWindow, sep: &str| {
        set_text(c, edit, &format!("some{sep}starting{sep}text"));
        // spaces work as expected
        run_with_specific_char(c, sep, "some", "starting", "text");
        set_text(c, edit, &format!("123{sep}is{sep}number"));
        run_with_specific_char(c, sep, "123", "is", "number");
        set_text(c, edit, &format!("123_1{sep}456_2{sep}789_3"));
        run_with_specific_char(c, sep, "123_1", "456_2", "789_3");
    };

    check_char(c, " ");
}
