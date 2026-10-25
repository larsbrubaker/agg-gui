//! The on-screen keyboard's caret gestures (`cursor_slide.rs`) in every text
//! widget, driven through the real `App` with touch events on the virtual UI
//! clock (helpers from `on_screen_keyboard_cursor.rs`).
//!
//! `RichTextEdit` and `RichMarkdownEditWidget` report and put back their
//! selection as `TextField` and `TextArea` do, so the Backspace slide gives
//! words back exactly, and a second finger or a cancelled touch deletes
//! nothing; the markdown editor takes the trackpad's line steps. A widget that
//! can't put its selection back gets the inverse keys instead and loses no
//! text to a cancel. The Backspace slide selects words with each platform's
//! word modifier in every editor. A dictation's words are in the field before
//! a gesture snapshots it, and selections clamp to character boundaries.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use super::on_screen_keyboard_cursor::{
    android, screen, space, text_field, wait, Finger, FINGER, IPHONE,
};
use super::on_screen_keyboard_touch::{center, char_key, font, key_of, Phone};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::input_profile::InputProfile;
use crate::platform::{override_platform_for_thread, Platform};
use crate::touch_state::TouchId;
use crate::widget::TextSelection;
use crate::widgets::markdown_editing::edit_widget::RichMarkdownEditWidget;
use crate::widgets::markdown_editing::layout_style::RichLayoutFonts;
use crate::widgets::on_screen_keyboard::cursor_slide::{
    DELETE_WORD_STEP, TRACKPAD_HOLD, TRACKPAD_LINE_STEP,
};
use crate::widgets::on_screen_keyboard::test_hook;
use crate::widgets::rich_text::{uniform_resolver, Block, RichDoc, RichTextEdit};
use crate::widgets::{TextArea, TextField};
use crate::{Key, Modifiers, Widget};

/// The text widgets the gestures must work in.
#[derive(Debug, Clone, Copy)]
enum Editor {
    Field,
    Area,
    Rich,
    Markdown,
}

impl Editor {
    /// The editor holding `text` (one line), its caret at the end.
    fn build(self, text: &str) -> Box<dyn Widget> {
        match self {
            Editor::Field => Box::new(TextField::new(font()).with_text(text)),
            Editor::Area => Box::new(TextArea::new(font()).with_text(text)),
            Editor::Rich => {
                let doc = RichDoc::from_blocks(vec![Block::plain(text)]);
                Box::new(RichTextEdit::new(doc, uniform_resolver(font())))
            }
            Editor::Markdown => {
                let fonts = RichLayoutFonts {
                    regular: font(),
                    bold: None,
                    mono: None,
                };
                let mut editor = RichMarkdownEditWidget::new(fonts, None);
                editor.set_markdown(text).expect("markdown loads");
                Box::new(editor)
            }
        }
    }

    /// An Android phone typing into this editor holding `text`, its caret
    /// at the end.
    fn phone(self, text: &str) -> Phone {
        self.phone_on(text, InputProfile::MobileAndroid)
    }

    fn phone_on(self, text: &str, profile: InputProfile) -> Phone {
        let size = match profile {
            InputProfile::MobileIOS => IPHONE,
            _ => Size::new(412.0, 800.0),
        };
        let mut phone = Phone::with(self.build(text), false, profile, size);
        let end = text.len();
        set_selection(&mut phone, end, end);
        phone
    }
}

fn set_selection(phone: &mut Phone, anchor: usize, caret: usize) {
    let done = phone
        .app
        .root_mut()
        .set_text_selection(TextSelection { anchor, caret });
    assert!(done, "the editor puts its selection where it is told");
}

fn selection(phone: &Phone) -> TextSelection {
    phone
        .app
        .root()
        .text_selection()
        .expect("the editor reports its selection")
}

fn text(phone: &Phone) -> String {
    let value = phone.app.root().text_input_value();
    let root = phone.app.root().as_any();
    if let Some(md) = root.and_then(|a| a.downcast_ref::<RichMarkdownEditWidget>()) {
        return md.markdown().trim_end_matches('\n').to_string();
    }
    value.expect("the editor reports its text")
}

fn backspace() -> crate::geometry::Point {
    center(key_of("Backspace").rect)
}

const RICH_EDITORS: [Editor; 2] = [Editor::Rich, Editor::Markdown];

#[test]
fn giving_a_word_back_shrinks_the_selection_in_the_rich_editors() {
    for editor in RICH_EDITORS {
        let _clock = crate::clock::scoped_virtual(None);
        let mut phone = editor.phone("one two three");
        let start = backspace();
        let mut finger = Finger::down(&mut phone, start);
        finger.slide_by(&mut phone, -2.0 * DELETE_WORD_STEP - 1.0, 0.0);
        let two = TextSelection {
            anchor: 13,
            caret: 4,
        };
        assert_eq!(selection(&phone), two, "{editor:?}: two words");
        finger.slide_by(&mut phone, DELETE_WORD_STEP + 1.0, 0.0);
        let one = TextSelection {
            anchor: 13,
            caret: 8,
        };
        assert_eq!(selection(&phone), one, "{editor:?}: one word back");
        finger.slide_to(&mut phone, start);
        let none = TextSelection {
            anchor: 13,
            caret: 13,
        };
        assert_eq!(selection(&phone), none, "{editor:?}: all back");
        finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
        finger.lift(&mut phone);
        assert_eq!(text(&phone), "one two ", "{editor:?}");
    }
}

#[test]
fn a_second_finger_or_a_cancel_deletes_nothing_in_the_rich_editors() {
    for editor in RICH_EDITORS {
        let _clock = crate::clock::scoped_virtual(None);
        let mut phone = editor.phone("one two three");
        let mut finger = Finger::down(&mut phone, backspace());
        finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
        let (x, y) = screen(&phone, center(char_key('q').rect));
        phone.app.on_touch_start(FINGER.0, TouchId(2), x, y, None);
        phone.app.on_touch_end(FINGER.0, TouchId(2));
        finger.lift(&mut phone);
        assert_eq!(text(&phone), "one two threeq", "{editor:?}: second finger");
        drop(phone);

        let mut phone = editor.phone("one two three");
        let mut finger = Finger::down(&mut phone, backspace());
        finger.slide_by(&mut phone, -2.0 * DELETE_WORD_STEP - 1.0, 0.0);
        let _ = finger;
        phone.app.on_touch_cancel(FINGER.0, FINGER.1);
        phone.paint();
        assert_eq!(text(&phone), "one two three", "{editor:?}: cancel");
        let back = TextSelection {
            anchor: 13,
            caret: 13,
        };
        assert_eq!(selection(&phone), back, "{editor:?}: cancel");
    }
}

#[test]
fn the_trackpad_moves_by_line_in_the_markdown_editor() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = Editor::Markdown.phone_on("abc\n\ndef", InputProfile::MobileIOS);
    set_selection(&mut phone, 1, 1);
    let mut finger = Finger::down(&mut phone, space());
    wait(&mut phone, TRACKPAD_HOLD + Duration::from_millis(100));
    assert!(test_hook::trackpad_active());
    finger.slide_by(&mut phone, 0.0, -TRACKPAD_LINE_STEP - 1.0);
    let moved = selection(&phone);
    assert_eq!(moved.caret, moved.anchor);
    assert!(moved.caret > 3, "down into the second paragraph: {moved:?}");
    finger.lift(&mut phone);
}

#[test]
fn the_backspace_slide_selects_words_with_each_platforms_word_modifier() {
    for platform in [Platform::MacOS, Platform::Windows, Platform::Linux] {
        for editor in [Editor::Field, Editor::Area, Editor::Rich, Editor::Markdown] {
            let _bindings = override_platform_for_thread(platform);
            let _clock = crate::clock::scoped_virtual(None);
            let mut phone = editor.phone("one two three");
            let mut finger = Finger::down(&mut phone, backspace());
            finger.slide_by(&mut phone, -2.0 * DELETE_WORD_STEP - 1.0, 0.0);
            finger.lift(&mut phone);
            assert_eq!(text(&phone), "one ", "{platform:?}, {editor:?}");
        }
    }
}

/// The keys a [`KeyLog`] was sent.
type Keys = Rc<RefCell<Vec<(Key, Modifiers)>>>;

/// A text widget that takes keys but can't report its selection: it logs
/// the keys it is sent.
struct KeyLog {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    keys: Keys,
}

impl Widget for KeyLog {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, 40.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn is_focusable(&self) -> bool {
        true
    }
    fn accepts_text_input(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::KeyDown { key, modifiers } = event {
            self.keys.borrow_mut().push((key.clone(), *modifiers));
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

fn key_log() -> (Phone, Keys) {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let widget = KeyLog {
        bounds: Rect::default(),
        children: Vec::new(),
        keys: Rc::clone(&keys),
    };
    let phone = Phone::new(Box::new(widget), false);
    keys.borrow_mut().clear();
    (phone, keys)
}

/// The logged keys as `<` (word left), `>` (word right) and `B` (Backspace).
fn logged(keys: &Keys) -> String {
    keys.borrow()
        .iter()
        .map(|(k, m)| match k {
            Key::ArrowLeft if m.shift => '<',
            Key::ArrowRight if m.shift => '>',
            Key::Backspace => 'B',
            _ => '?',
        })
        .collect()
}

#[test]
fn a_widget_that_cannot_put_its_selection_back_gets_the_inverse_keys() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut phone, keys) = key_log();
    let mut finger = Finger::down(&mut phone, backspace());
    finger.slide_by(&mut phone, -2.0 * DELETE_WORD_STEP - 1.0, 0.0);
    finger.slide_by(&mut phone, DELETE_WORD_STEP + 1.0, 0.0);
    finger.lift(&mut phone);
    assert_eq!(
        logged(&keys),
        "<<>B",
        "two words, one back, delete the other"
    );
    drop(phone);

    // A cancel gives every word back and deletes nothing.
    let (mut phone, keys) = key_log();
    let mut finger = Finger::down(&mut phone, backspace());
    finger.slide_by(&mut phone, -2.0 * DELETE_WORD_STEP - 1.0, 0.0);
    let _ = finger;
    phone.app.on_touch_cancel(FINGER.0, FINGER.1);
    phone.paint();
    assert_eq!(logged(&keys), "<<>>");
}

#[test]
fn a_trackpad_cancel_after_a_dictation_puts_the_caret_after_its_words() {
    use super::on_screen_keyboard_voice::{bound_field, FakeVoice, Rig, PHONE};
    use crate::widgets::on_screen_keyboard::VoiceState;
    let _clock = crate::clock::scoped_virtual(None);
    let voice = FakeVoice::new(VoiceState::Ready);
    let (field, cell) = bound_field("");
    let mut rig = Rig::new(field, InputProfile::MobileIOS, Some(&voice));
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    assert_eq!(rig.preedit().as_deref(), Some(" there"));
    let at = center(key_of("Space").rect);
    let (dev, id) = FINGER;
    rig.app
        .on_touch_start(dev, id, at.x, PHONE.height - at.y, None);
    for _ in 0..12 {
        crate::clock::advance(Duration::from_millis(50));
        rig.paint();
    }
    assert!(test_hook::trackpad_active());
    rig.app.on_touch_cancel(dev, id);
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello there");
    assert_eq!(rig.field().cursor_pos(), "Hello there".len());
}

#[test]
fn a_selection_put_back_clamps_to_a_character_boundary() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("héllo", 0);
    // Byte 2 is inside the two-byte é: back to its start.
    set_selection(&mut phone, 2, 99);
    assert_eq!(
        selection(&phone),
        TextSelection {
            anchor: 1,
            caret: 6
        }
    );
    assert_eq!(text_field(&mut phone).selection(), "éllo");
}
