//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/MacTextEditKeyBindingTests.cs`.
//!
//! Caret motion and delete bindings differ between Mac and Windows. C#'s Mac
//! platform layer folds Command onto `Keys.Control`, so the very same key
//! event that means "word jump" on Windows means "go to start of line" on
//! Mac. agg-gui delivers Command as `meta`; these tests send it that way
//! ([`command`]) where C# sends `Keys.Control` under the Mac bindings, and
//! send `ctrl` where C# means a Windows Control chord.
//!
//! C# pins `InternalTextEditWidget.UseMacKeyBindings` (process-wide, hence
//! its `[NotInParallel]`); here [`with_key_bindings`] pins this test
//! thread's platform with `agg_gui::platform::override_platform_for_thread`,
//! which [`agg_gui::platform::use_mac_key_bindings`] follows, so the tests
//! run in parallel. C#'s `InternalTextEditWidget(text, 12, multiLine, 0)` is
//! an agg-gui [`TextArea`] (as in `text_edit/multi_line.rs`), and C#'s
//! `editWidget.OnKeyDown(new KeyEventArgs(keys))` is the area's own
//! `on_event` with that key down. C#'s `Selecting` is "anchor and caret
//! differ" (`TextArea::selection`).

use std::sync::Arc;

use agg_gui::event::{Event, Key, Modifiers};
use agg_gui::platform::{override_platform_for_thread, use_mac_key_bindings, Platform};
use agg_gui::widgets::TextArea;
use agg_gui::{Rect, Size, Widget};

const THREE_WORDS: &str = "hello big world";

const THREE_LINES: &str = "line1\nline2\nline3";

/// C#'s 12 points in pixels.
const EM_PIXELS: f64 = 12.0 / 72.0 * 96.0;

/// Runs `action` with the Mac/Windows binding seam pinned for this thread,
/// restoring it afterwards no matter how the test ends (the guard's drop).
fn with_key_bindings(use_mac_key_bindings: bool, action: impl FnOnce()) {
    let platform = if use_mac_key_bindings {
        Platform::MacOS
    } else {
        Platform::Windows
    };
    let _pinned = override_platform_for_thread(platform);
    action();
}

/// C# `new InternalTextEditWidget(text, 12, multiLine, 0) { CharIndexToInsertBefore = cursor }`.
/// The single-line editor is the same area with one line; it is laid out
/// wide enough that nothing wraps, so its visual lines are its text lines.
fn editor_at(text: &str, cursor: usize) -> TextArea {
    let mut edit_widget = TextArea::new(Arc::new(agg_gui::fonts::standard_ui_font()))
        .with_font_size(EM_PIXELS)
        .with_padding(0.0)
        .with_text(text);
    edit_widget.layout(Size::new(10_000.0, 10_000.0));
    edit_widget.set_bounds(Rect::new(0.0, 0.0, 10_000.0, 10_000.0));
    edit_widget.set_char_index_to_insert_before(cursor as i32);
    edit_widget.set_selection_index_to_start_before(cursor as i32);
    edit_widget
}

/// An editor whose caret sits at `cursor` with a live selection running back
/// to `anchor`, which is the state an unshifted motion has to collapse.
fn editor_selecting(text: &str, anchor: usize, cursor: usize) -> TextArea {
    let mut edit_widget = editor_at(text, cursor);
    edit_widget.set_selection_index_to_start_before(anchor as i32);
    assert!(selecting(&edit_widget));
    edit_widget
}

fn send_key_down(key: Key, modifiers: Modifiers, edit_widget: &mut TextArea) {
    edit_widget.on_event(&Event::KeyDown { key, modifiers });
}

fn selecting(edit_widget: &TextArea) -> bool {
    edit_widget.selection().is_some()
}

fn none() -> Modifiers {
    Modifiers::default()
}

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::default()
    }
}

/// The Mac Command key, as agg-gui delivers it.
fn command() -> Modifiers {
    Modifiers {
        meta: true,
        ..Modifiers::default()
    }
}

fn control() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

fn shift(mut modifiers: Modifiers) -> Modifiers {
    modifiers.shift = true;
    modifiers
}

#[test]
fn mac_alt_arrows_move_by_word() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_WORDS, THREE_WORDS.len());

        // option-left from the end of "hello big world" lands on the "w"
        send_key_down(Key::ArrowLeft, alt(), &mut edit_widget);
        assert_eq!(
            edit_widget.char_index_to_insert_before(),
            "hello big ".len()
        );

        send_key_down(Key::ArrowLeft, alt(), &mut edit_widget);
        assert_eq!(edit_widget.char_index_to_insert_before(), "hello ".len());

        send_key_down(Key::ArrowRight, alt(), &mut edit_widget);
        assert_eq!(
            edit_widget.char_index_to_insert_before(),
            "hello big ".len()
        );
    });
}

#[test]
fn mac_command_left_goes_to_line_start_not_word_boundary() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_WORDS, THREE_WORDS.len());

        // Before the Mac bindings existed this did a word jump and stopped at
        // "world", which is the bug this test exists for.
        send_key_down(Key::ArrowLeft, command(), &mut edit_widget);
        assert_eq!(edit_widget.char_index_to_insert_before(), 0);
    });
}

#[test]
fn mac_command_right_goes_to_line_end() {
    with_key_bindings(true, || {
        // the middle line holds two words so that a word jump (index 10) and
        // the end of the line (index 14) cannot be confused for one another
        let text = "line1\nbig word\nline3";
        let mut edit_widget = editor_at(text, "line1\n".len());

        send_key_down(Key::ArrowRight, command(), &mut edit_widget);
        assert_eq!(
            edit_widget.char_index_to_insert_before(),
            "line1\nbig word".len()
        );
    });
}

#[test]
fn mac_command_up_and_down_go_to_document_start_and_end() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_LINES, "line1\nli".len());

        send_key_down(Key::ArrowUp, command(), &mut edit_widget);
        assert_eq!(edit_widget.char_index_to_insert_before(), 0);

        send_key_down(Key::ArrowDown, command(), &mut edit_widget);
        assert_eq!(edit_widget.char_index_to_insert_before(), THREE_LINES.len());
    });
}

#[test]
fn mac_alt_backspace_deletes_previous_word() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_WORDS, THREE_WORDS.len());

        send_key_down(Key::Backspace, alt(), &mut edit_widget);
        assert_eq!(edit_widget.text(), "hello big ");
        assert_eq!(
            edit_widget.char_index_to_insert_before(),
            "hello big ".len()
        );
    });
}

#[test]
fn mac_command_backspace_deletes_to_line_start() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_LINES, "line1\nline2".len());

        send_key_down(Key::Backspace, command(), &mut edit_widget);
        assert_eq!(edit_widget.text(), "line1\n\nline3");
    });
}

#[test]
fn mac_shift_composes_with_word_and_line_motion() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_WORDS, THREE_WORDS.len());

        send_key_down(Key::ArrowLeft, shift(alt()), &mut edit_widget);
        assert_eq!(edit_widget.selected_text(), "world");

        let mut edit_widget = editor_at(THREE_WORDS, THREE_WORDS.len());

        send_key_down(Key::ArrowLeft, shift(command()), &mut edit_widget);
        assert_eq!(edit_widget.selected_text(), THREE_WORDS);
    });
}

#[test]
fn mac_unshifted_command_arrows_collapse_selection() {
    with_key_bindings(true, || {
        // An unshifted Command-arrow is a plain caret motion on Mac, so it
        // ends the selection just as an unmodified arrow does.
        let mut edit_widget = editor_selecting(THREE_WORDS, 6, THREE_WORDS.len());
        send_key_down(Key::ArrowLeft, command(), &mut edit_widget);
        assert!(!selecting(&edit_widget));
        assert_eq!(edit_widget.selected_text(), "");
        assert_eq!(edit_widget.char_index_to_insert_before(), 0);

        let mut edit_widget = editor_selecting(THREE_LINES, "line1\n".len(), "line1\nli".len());
        send_key_down(Key::ArrowRight, command(), &mut edit_widget);
        assert!(!selecting(&edit_widget));
        assert_eq!(edit_widget.selected_text(), "");
        assert_eq!(
            edit_widget.char_index_to_insert_before(),
            "line1\nline2".len()
        );

        let mut edit_widget = editor_selecting(THREE_LINES, "line1\n".len(), "line1\nli".len());
        send_key_down(Key::ArrowUp, command(), &mut edit_widget);
        assert!(!selecting(&edit_widget));
        assert_eq!(edit_widget.selected_text(), "");
        assert_eq!(edit_widget.char_index_to_insert_before(), 0);

        let mut edit_widget = editor_selecting(THREE_LINES, "line1\n".len(), "line1\nli".len());
        send_key_down(Key::ArrowDown, command(), &mut edit_widget);
        assert!(!selecting(&edit_widget));
        assert_eq!(edit_widget.selected_text(), "");
        assert_eq!(edit_widget.char_index_to_insert_before(), THREE_LINES.len());
    });
}

#[test]
fn use_mac_key_bindings_defaults_to_running_os() {
    // Every other test pins the seam, so nothing else covers the default. If
    // it were ever broken the whole Mac binding set would silently switch off
    // on Mac and the suite would stay green.
    assert_eq!(use_mac_key_bindings(), cfg!(target_os = "macos"));
}

#[test]
fn mac_home_and_end_still_go_to_line_start_and_end() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_LINES, "line1\nli".len());

        send_key_down(Key::Home, none(), &mut edit_widget);
        assert_eq!(edit_widget.char_index_to_insert_before(), "line1\n".len());

        send_key_down(Key::End, none(), &mut edit_widget);
        assert_eq!(
            edit_widget.char_index_to_insert_before(),
            "line1\nline2".len()
        );
    });
}

#[test]
fn windows_control_left_still_jumps_by_word() {
    with_key_bindings(false, || {
        let mut edit_widget = editor_at(THREE_WORDS, THREE_WORDS.len());

        send_key_down(Key::ArrowLeft, control(), &mut edit_widget);
        assert_eq!(
            edit_widget.char_index_to_insert_before(),
            "hello big ".len()
        );
    });
}

#[test]
fn windows_control_home_still_goes_to_document_start() {
    with_key_bindings(false, || {
        let mut edit_widget = editor_at(THREE_LINES, "line1\nli".len());

        send_key_down(Key::Home, control(), &mut edit_widget);
        assert_eq!(edit_widget.char_index_to_insert_before(), 0);
    });
}

/// C#'s Mac layer folds physical Control onto Command as well, so on Mac a
/// Control-arrow is the line motion too, never the Windows word jump.
#[test]
fn rust_only_mac_physical_control_is_the_command_chord() {
    with_key_bindings(true, || {
        let mut edit_widget = editor_at(THREE_WORDS, THREE_WORDS.len());
        send_key_down(Key::ArrowLeft, control(), &mut edit_widget);
        assert_eq!(edit_widget.char_index_to_insert_before(), 0);
    });
}

/// The single-line `TextField` follows the same bindings as the area.
#[test]
fn rust_only_text_field_follows_the_mac_and_windows_bindings() {
    use agg_gui::widgets::TextField;
    let field = |text: &str| {
        let mut f = TextField::new(Arc::new(agg_gui::fonts::standard_ui_font())).with_text(text);
        f.on_event(&Event::FocusGained);
        f.on_event(&Event::KeyDown {
            key: Key::End,
            modifiers: none(),
        });
        f
    };
    let key = |f: &mut TextField, key: Key, modifiers: Modifiers| {
        f.on_event(&Event::KeyDown { key, modifiers });
    };
    with_key_bindings(true, || {
        let mut f = field(THREE_WORDS);
        key(&mut f, Key::ArrowLeft, alt());
        assert_eq!(f.cursor_pos(), "hello big ".len());
        key(&mut f, Key::ArrowLeft, command());
        assert_eq!(f.cursor_pos(), 0);
        let mut f = field(THREE_WORDS);
        key(&mut f, Key::Backspace, alt());
        assert_eq!(f.text(), "hello big ");
        key(&mut f, Key::Backspace, command());
        assert_eq!(f.text(), "");
    });
    with_key_bindings(false, || {
        let mut f = field(THREE_WORDS);
        key(&mut f, Key::ArrowLeft, control());
        assert_eq!(f.cursor_pos(), "hello big ".len());
        // Alt is no word key off Mac: one character back
        key(&mut f, Key::ArrowLeft, alt());
        assert_eq!(f.cursor_pos(), "hello big".len());
        // nor is the Windows key a line key: one character on, not the end
        key(&mut f, Key::ArrowRight, command());
        assert_eq!(f.cursor_pos(), "hello big ".len());
    });
}
