//! Ports agg-sharp `Tests/Agg.Tests/Agg.UI/TextSuggestionControllerTests.cs`
//! 1:1 (same names in snake_case, same expectations): keys and clicks pushed
//! through a headless [`App`] holding the production `TextField`, painted
//! after every input as a frame would be. Focus stays in the field
//! throughout — the contract that makes this different from a popup menu.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use super::{TextSuggestion, TextSuggestionController, TextSuggestionList};
use crate::event::{Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{Insets, VAnchor};
use crate::text::{measure_advance, Font};
use crate::widget::{App, Widget};
use crate::widgets::{AbsoluteLayout, ScrollView, SizedBox, TextField};
use crate::{Framebuffer, GfxCtx};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

/// `TextField`'s default font size, which the harness fields keep.
const FONT_SIZE: f64 = 14.0;

const WORDS: [&str; 16] = [
    "apple", "apricot", "avocado", "banana", "obj.", "self.", "cab", "cad", "cake", "calf", "calm",
    "camp", "can", "cap", "car", "cat",
];

fn words(prefix: &str) -> Vec<&'static str> {
    WORDS
        .iter()
        .copied()
        .filter(|w| w.starts_with(prefix))
        .collect()
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// The C# `FakeProvider`.
#[derive(Clone, Default)]
struct FakeProvider {
    calls: Rc<RefCell<Vec<(String, usize)>>>,
    /// Answer an empty word with every word, as a provider listing all it
    /// knows on demand would.
    offer_everything_for_an_empty_word: Rc<Cell<bool>>,
}

impl FakeProvider {
    fn get_suggestions(&self, text: &str, caret: usize) -> TextSuggestionList {
        self.calls.borrow_mut().push((text.to_string(), caret));
        if text[..caret].ends_with("obj.") {
            return TextSuggestionList::new(
                caret,
                0,
                vec![
                    TextSuggestion::new("width").with_detail("20"),
                    TextSuggestion::new("height")
                        .with_detail("10")
                        .with_description("How tall it is"),
                ],
            );
        }
        let bytes = text.as_bytes();
        let mut start = caret;
        while start > 0 && bytes[start - 1].is_ascii_alphabetic() {
            start -= 1;
        }
        let word = &text[start..caret];
        if word.is_empty() && !self.offer_everything_for_an_empty_word.get() {
            return TextSuggestionList::empty();
        }
        let matches = words(word).into_iter().map(TextSuggestion::new).collect();
        TextSuggestionList::new(start, caret - start, matches)
    }

    fn last_call(&self) -> (String, usize) {
        self.calls.borrow().last().cloned().expect("a call")
    }
}

enum Mount {
    /// The field sits directly in the window at (50, 200).
    Plain,
    /// Top anchored, 40 below the window's top.
    TopAnchored,
    /// At (10, 500) in 600-tall content of a 300×200 scroll view.
    Scrolled(Rc<Cell<f64>>),
}

struct Harness {
    app: App,
    size: Size,
    controller: TextSuggestionController,
    provider: FakeProvider,
    field_path: Vec<usize>,
    other_path: Option<Vec<usize>>,
    enter_pressed: Rc<Cell<u32>>,
    edit_complete: Rc<Cell<u32>>,
    escapes_seen_unhandled: Rc<Cell<u32>>,
}

impl Harness {
    fn new() -> Self {
        Self::build(Mount::Plain, true)
    }

    fn build(mount: Mount, with_other_field: bool) -> Self {
        Self::build_with(mount, with_other_field, |c| c)
    }

    /// `configure` adjusts the controller before it is attached.
    fn build_with(
        mount: Mount,
        with_other_field: bool,
        configure: impl FnOnce(TextSuggestionController) -> TextSuggestionController,
    ) -> Self {
        let provider = FakeProvider::default();
        let p = provider.clone();
        let controller = configure(TextSuggestionController::new(move |t: &str, c: usize| {
            p.get_suggestions(t, c)
        }));
        let enter_pressed = Rc::new(Cell::new(0));
        let edit_complete = Rc::new(Cell::new(0));
        let (e1, e2) = (Rc::clone(&enter_pressed), Rc::clone(&edit_complete));
        let field = TextField::new(font())
            .with_text_suggestions(controller.clone())
            .with_max_size(Size::new(200.0, f64::MAX))
            .on_enter(move |_| e1.set(e1.get() + 1))
            .on_edit_complete(move |_| e2.set(e2.get() + 1));
        let mut root = AbsoluteLayout::new();
        let field_path = match mount {
            Mount::Plain => {
                root = root.add(Box::new(field.with_origin(50.0, 200.0)));
                vec![0]
            }
            Mount::TopAnchored => {
                let field = field
                    .with_origin(50.0, 0.0)
                    .with_v_anchor(VAnchor::TOP)
                    .with_margin(Insets {
                        top: 40.0,
                        ..Insets::default()
                    });
                root = root.add(Box::new(field));
                vec![0]
            }
            Mount::Scrolled(offset) => {
                let content = SizedBox::fixed(280.0, 600.0).with_child(Box::new(
                    AbsoluteLayout::new().add(Box::new(field.with_origin(10.0, 500.0))),
                ));
                let scroller = ScrollView::new(Box::new(content))
                    .with_offset_cell(offset)
                    .with_min_size(Size::new(300.0, 200.0))
                    .with_max_size(Size::new(300.0, 200.0));
                root = root.add(Box::new(scroller.with_origin(0.0, 50.0)));
                vec![0, 0, 0, 0]
            }
        };
        let other_path = if with_other_field {
            let other = TextField::new(font())
                .with_max_size(Size::new(200.0, f64::MAX))
                .with_origin(50.0, 20.0);
            root = root.add(Box::new(other));
            Some(vec![1])
        } else {
            None
        };
        let mut app = App::new(Box::new(root));
        let escapes = Rc::new(Cell::new(0));
        let esc = Rc::clone(&escapes);
        app.set_global_key_handler(move |key, _| {
            if key == Key::Escape {
                esc.set(esc.get() + 1);
            }
            false
        });
        let mut harness = Self {
            app,
            size: Size::new(400.0, 300.0),
            controller,
            provider,
            field_path,
            other_path,
            enter_pressed,
            edit_complete,
            escapes_seen_unhandled: escapes,
        };
        harness.frame();
        let center = harness.rect_of(&harness.field_path).center();
        harness.click(center);
        assert!(harness.field_focused(), "the harness focuses the field");
        harness
    }

    /// Lay out and paint, as one frame would.
    fn frame(&mut self) {
        self.app.layout(self.size);
        let mut fb = Framebuffer::new(self.size.width as u32, self.size.height as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        self.app.paint(&mut ctx);
    }

    /// Root-space rect of the widget at `path`.
    fn rect_of(&self, path: &[usize]) -> Rect {
        let mut w: &dyn Widget = self.app.root();
        let (mut x, mut y) = (0.0, 0.0);
        for &i in path {
            w = w.children()[i].as_ref();
            x += w.bounds().x;
            y += w.bounds().y;
        }
        Rect::new(x, y, w.bounds().width, w.bounds().height)
    }

    fn field(&self) -> &TextField {
        let mut w: &dyn Widget = self.app.root();
        for &i in &self.field_path {
            w = w.children()[i].as_ref();
        }
        w.as_any()
            .and_then(|a| a.downcast_ref::<TextField>())
            .expect("the field")
    }

    fn field_mut(&mut self) -> &mut TextField {
        let mut w: &mut dyn Widget = self.app.root_mut();
        for &i in &self.field_path {
            w = w.children_mut()[i].as_mut();
        }
        w.as_any_mut()
            .and_then(|a| a.downcast_mut::<TextField>())
            .expect("the field")
    }

    fn text(&self) -> String {
        self.field().text()
    }

    fn set_text(&mut self, text: &str) {
        self.field_mut().set_text(text);
        self.frame();
    }

    fn field_focused(&self) -> bool {
        self.app.focused_path() == Some(self.field_path.as_slice())
    }

    fn other_contains_focus(&self) -> bool {
        self.other_path.is_some() && self.app.focused_path() == self.other_path.as_deref()
    }

    fn labels(&self) -> Vec<String> {
        self.controller.labels()
    }

    fn type_char(&mut self, c: char) {
        self.app.on_key_down(Key::Char(c), Modifiers::default());
        self.frame();
    }

    fn key(&mut self, key: Key, mods: Modifiers) {
        self.app.on_key_down(key, mods);
        self.frame();
    }

    fn click(&mut self, root_pos: Point) {
        let y = self.size.height - root_pos.y;
        self.app
            .on_mouse_down(root_pos.x, y, MouseButton::Left, Modifiers::default());
        self.app
            .on_mouse_up(root_pos.x, y, MouseButton::Left, Modifiers::default());
        self.frame();
    }

    fn popup_bounds(&self) -> Rect {
        self.controller.popup_bounds().expect("the list is showing")
    }

    /// The bottom of the caret's line where the text at `index` starts, in
    /// root coordinates.
    fn line_bottom_at(&self, index: usize) -> Point {
        let origin = self.rect_of(&self.field_path);
        let line = self.field().line_bounds_at(index);
        Point::new(origin.x + line.x, origin.y + line.y)
    }

    fn line_bottom_at_replace_start(&self) -> Point {
        self.line_bottom_at(self.controller.suggestions().replace_start)
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

fn none() -> Modifiers {
    Modifiers::default()
}

#[test]
fn opens_when_typing_finds_suggestions_and_stays_closed_when_none() {
    let mut h = Harness::new();

    h.type_char('z');
    assert!(!h.controller.is_open());
    assert_eq!(
        h.provider.calls.borrow().len(),
        1,
        "typing asks the provider even when it then has nothing"
    );

    h.set_text("");
    h.type_char('a');
    assert!(h.controller.is_open());
    assert!(h.controller.popup_bounds().is_some());
    assert_eq!(h.labels(), ["apple", "apricot", "avocado"]);
    assert_eq!(h.controller.highlight_index(), 0);
}

#[test]
fn down_and_up_move_the_highlight_and_wrap() {
    let mut h = Harness::new();
    h.type_char('a');

    h.key(Key::ArrowDown, none());
    assert_eq!(h.controller.highlight_index(), 1);
    h.key(Key::ArrowDown, none());
    h.key(Key::ArrowDown, none());
    assert_eq!(
        h.controller.highlight_index(),
        0,
        "Down past the last row wraps to the first"
    );
    h.key(Key::ArrowUp, none());
    assert_eq!(
        h.controller.highlight_index(),
        2,
        "Up past the first row wraps to the last"
    );
    assert_eq!(
        h.text(),
        "a",
        "the arrows belong to the list while it is open"
    );
    assert!(h.field_focused());
}

#[test]
fn enter_accepts_keeps_focus_and_does_not_submit() {
    let mut h = Harness::new();
    h.type_char('a');
    h.type_char('p');
    h.key(Key::ArrowDown, none());
    h.key(Key::Enter, none());

    assert_eq!(h.text(), "apricot");
    assert_eq!(h.field().cursor_pos(), 7);
    assert!(h.field_focused());
    assert_eq!(h.enter_pressed.get(), 0);
    assert_eq!(h.edit_complete.get(), 0);
}

#[test]
fn tab_accepts_and_does_not_move_focus() {
    let mut h = Harness::new();
    h.type_char('b');
    h.key(Key::Tab, none());

    assert_eq!(h.text(), "banana");
    assert!(h.field_focused());
    assert!(!h.other_contains_focus());
}

#[test]
fn escape_closes_without_any_other_effect() {
    let mut h = Harness::new();
    h.type_char('a');
    h.key(Key::Escape, none());

    assert!(!h.controller.is_open());
    assert_eq!(h.text(), "a");
    assert!(h.field_focused());
    assert_eq!(
        h.escapes_seen_unhandled.get(),
        0,
        "a dialog must not also close on the Escape that closed the list"
    );

    h.key(Key::Escape, none());
    assert_eq!(
        h.escapes_seen_unhandled.get(),
        1,
        "with the list closed Escape is the field's again"
    );
}

#[test]
fn clicking_a_row_accepts_it_and_keeps_focus_in_the_field() {
    let mut h = Harness::new();
    h.type_char('a');

    let row_center = h.controller.row_bounds(2).expect("row 2 in view").center();
    h.click(row_center);

    assert_eq!(h.text(), "avocado");
    assert!(h.field_focused());
    assert_eq!(
        h.edit_complete.get(),
        0,
        "the press on the list must never take focus off the field"
    );
}

#[test]
fn typing_and_backspace_refilter() {
    let mut h = Harness::new();
    h.type_char('a');
    h.type_char('p');

    assert_eq!(h.provider.last_call(), ("ap".to_string(), 2));
    assert_eq!(h.labels(), ["apple", "apricot"]);

    h.key(Key::Backspace, none());
    assert_eq!(h.provider.last_call(), ("a".to_string(), 1));
    assert_eq!(h.labels(), ["apple", "apricot", "avocado"]);

    h.key(Key::Home, none());
    assert!(
        !h.controller.is_open(),
        "no word before the caret, so nothing to offer"
    );
}

#[test]
fn losing_focus_closes() {
    let mut h = Harness::new();
    h.type_char('a');
    let other = h.rect_of(&[1]).center();
    h.click(other);

    assert!(!h.controller.is_open());
    assert!(h.controller.popup_bounds().is_none());
}

#[test]
fn accept_queries_again_at_the_new_caret() {
    let mut h = Harness::new();
    h.type_char('o');
    h.key(Key::Enter, none());

    assert_eq!(h.text(), "obj.");
    assert_eq!(h.provider.last_call(), ("obj.".to_string(), 4));
    assert!(
        h.controller.is_open(),
        "an insert ending in '.' shows the members next"
    );
    assert_eq!(h.labels(), ["width", "height"]);
}

/// Accepting a suggestion is one edit of its own: Ctrl+Z puts back exactly
/// what was typed, caret and all, and redo brings the accepted text back.
#[test]
fn accept_is_its_own_undo_step() {
    let mut h = Harness::new();
    for c in ['=', 's', 'e', 'l'] {
        h.type_char(c);
    }
    assert_eq!(h.labels(), ["self."]);
    h.key(Key::Enter, none());
    assert_eq!(h.text(), "=self.");

    h.key(Key::Char('z'), ctrl());
    assert_eq!(h.text(), "=sel");
    assert_eq!(h.field().cursor_pos(), 4);

    h.key(Key::Char('y'), ctrl());
    assert_eq!(h.text(), "=self.");
    assert_eq!(h.field().cursor_pos(), 6);
}

/// The list hangs just below the caret's line, lined up with the start of
/// the word being completed, and does not slide right as the word is typed.
#[test]
fn popup_sits_just_below_the_word_being_completed() {
    let mut h = Harness::new();
    h.set_text("1 + ");
    h.field_mut().set_cursor_position(4);
    h.type_char('b');
    let first_left = h.popup_bounds().x;
    h.type_char('a');

    // Where "1 + " ends, measured here rather than asked of the field: the
    // text inset plus the advance of the text before the word, and the
    // bottom of the text line centred in the field (ascender to descender).
    let field = h.rect_of(&h.field_path);
    let insets = h.field().text_insets();
    let f = font();
    let (ascent, descent) = (f.ascender_px(FONT_SIZE), f.descender_px(FONT_SIZE));
    let centre = field.height * 0.5 + (insets.bottom - insets.top) * 0.5;
    let line_bottom = Point::new(
        field.x + insets.left + measure_advance(&f, "1 + ", FONT_SIZE),
        field.y + centre - (ascent - descent) * 0.5 - descent,
    );
    let popup = h.popup_bounds();

    assert!(popup.top() <= line_bottom.y);
    assert!(line_bottom.y - popup.top() < 10.0);
    assert!((popup.x - line_bottom.x).abs() < 1.0);
    assert!(
        line_bottom.x > h.rect_of(&h.field_path).x + 5.0,
        "the word starts after \"1 + \", so this pins the anchor to the word, not the field's left edge"
    );
    assert_eq!(popup.x, first_left);
}

/// The list follows a top-anchored field when a window resize moves it.
#[test]
fn popup_follows_the_field_when_the_window_resizes() {
    let mut h = Harness::build(Mount::TopAnchored, true);
    h.type_char('a');
    let before = h.line_bottom_at_replace_start();

    h.size = Size::new(400.0, 500.0);
    h.frame();

    let line_bottom = h.line_bottom_at_replace_start();
    assert!(
        line_bottom.y > before.y + 100.0,
        "the resize must actually move the field"
    );
    let popup = h.popup_bounds();
    assert!(popup.top() <= line_bottom.y);
    assert!(line_bottom.y - popup.top() < 10.0);
    assert!((popup.x - line_bottom.x).abs() < 1.0);
}

/// Scrolling a container the field sits in closes the list: it is drawn
/// over the whole window, not clipped by the container.
#[test]
fn scrolling_an_ancestor_closes_the_list() {
    let offset = Rc::new(Cell::new(0.0));
    let mut h = Harness::build(Mount::Scrolled(Rc::clone(&offset)), true);
    h.type_char('a');
    assert!(h.controller.is_open());

    offset.set(offset.get() + 30.0);
    h.frame();

    assert!(!h.controller.is_open());
    assert!(h.controller.popup_bounds().is_none());
    assert!(h.field_focused());
}

/// The wheel scrolls the rows, and the highlight comes along so it is always
/// a row in view.
#[test]
fn wheel_keeps_the_highlight_on_a_visible_row() {
    let mut h = Harness::new();
    h.type_char('c');
    assert_eq!(h.labels().len(), 10);

    let center = h.controller.row_bounds(1).expect("row 1").center();
    h.app
        .on_mouse_wheel(center.x, h.size.height - center.y, -1.0);
    h.frame();

    assert!(
        h.controller.row_bounds(0).is_none(),
        "the first row scrolled out of view"
    );
    let highlight = h.controller.highlight_index();
    assert!(h
        .controller
        .row_bounds(highlight)
        .is_some_and(|r| r.width > 0.0));

    h.key(Key::Enter, none());
    assert_eq!(h.text(), words("c")[highlight]);
}

#[test]
fn shift_enter_does_not_accept() {
    let mut h = Harness::new();
    h.type_char('a');
    h.key(Key::Enter, shift());

    assert_eq!(h.text(), "a");
}

/// Shift+Tab closes the list and still does what Shift+Tab does.
#[test]
fn shift_tab_closes_and_is_not_consumed() {
    let mut h = Harness::new();
    h.type_char('a');
    h.key(Key::Tab, shift());

    assert!(!h.controller.is_open());
    assert_eq!(h.text(), "a");
    assert!(
        h.other_contains_focus(),
        "the list must not swallow the window's Shift+Tab focus move"
    );

    // With nowhere else to go the focus stays in the field, and the list
    // still closes.
    let mut alone = Harness::build(Mount::Plain, false);
    alone.type_char('a');
    alone.key(Key::Tab, shift());

    assert!(alone.field_focused());
    assert!(!alone.controller.is_open());
}

/// Ctrl+Space asks the provider at the caret on demand and shows whatever it
/// offers, without typing a space.
#[test]
fn control_space_opens_the_list_on_demand() {
    let mut h = Harness::new();
    h.provider.offer_everything_for_an_empty_word.set(true);

    h.key(Key::Char(' '), ctrl());
    assert!(h.controller.is_open());
    assert_eq!(h.provider.last_call(), (String::new(), 0));
    assert_eq!(h.labels().len(), WORDS.len());
    assert_eq!(h.text(), "");

    h.type_char('a');
    h.key(Key::Escape, none());
    assert!(!h.controller.is_open());
    h.key(Key::Char(' '), ctrl());
    assert!(h.controller.is_open());
    assert_eq!(h.labels(), ["apple", "apricot", "avocado"]);
    assert_eq!(h.text(), "a");
}

// ── Beyond the C# class: review follow-ups ──────────────────────────────────

/// Dropping the field closes its list, so the controller stops reporting a
/// list nobody shows, and the open-list count goes back to zero.
#[test]
fn dropping_the_field_closes_its_list() {
    let mut h = Harness::new();
    h.type_char('a');
    assert!(h.controller.is_open());
    assert!(super::any_list_open());

    let controller = h.controller.clone();
    drop(h);

    assert!(!controller.is_open());
    assert!(controller.popup_bounds().is_none());
    assert!(!super::any_list_open());
}

/// A controller dropped while its list is open no longer counts as open.
#[test]
fn dropping_an_open_controller_releases_the_open_count() {
    let ctrl = TextSuggestionController::new(|_: &str, c: usize| {
        TextSuggestionList::new(c, 0, vec![TextSuggestion::new("x")])
    });
    assert!(ctrl.requery("", 0, (font(), FONT_SIZE)));
    assert!(super::any_list_open());
    drop(ctrl);
    assert!(!super::any_list_open());
}

/// Accepting the word exactly as typed changes nothing, so it adds no undo
/// step: Ctrl+Z still undoes the typing itself.
#[test]
fn accepting_the_typed_word_adds_no_undo_step() {
    let mut h = Harness::new();
    for c in ['c', 'a', 't'] {
        h.type_char(c);
    }
    assert_eq!(h.labels(), ["cat"]);
    h.key(Key::Enter, none());
    assert_eq!(h.text(), "cat");
    assert_eq!(h.field().cursor_pos(), 3);

    h.key(Key::Char('z'), ctrl());
    assert_eq!(h.text(), "", "the one undo step is the typed word");
}

/// The pointer over the list is the arrow, not the field's I-beam.
#[test]
fn the_list_shows_the_arrow_cursor() {
    let mut h = Harness::new();
    h.type_char('a');
    let row = h.controller.row_bounds(1).expect("row 1").center();
    h.app.on_mouse_move(row.x, h.size.height - row.y);
    assert_eq!(
        crate::cursor::current_cursor_icon(),
        crate::cursor::CursorIcon::Default
    );
    let field = h.rect_of(&h.field_path).center();
    h.app.on_mouse_move(field.x, h.size.height - field.y);
    assert_eq!(
        crate::cursor::current_cursor_icon(),
        crate::cursor::CursorIcon::Text
    );
}

/// `with_font` sizes the rows with the app's font (MatterCAD's theme size)
/// rather than the field's.
#[test]
fn with_font_measures_the_rows_with_that_font() {
    let mut small = Harness::new();
    small.type_char('a');
    assert_eq!(
        small.popup_bounds().width,
        150.0,
        "short labels take the minimum width"
    );

    let mut large = Harness::build_with(Mount::Plain, true, |c| c.with_font(font(), 40.0));
    large.type_char('a');
    let expected = measure_advance(&font(), "avocado", 40.0) + 4.0 * 3.0;
    assert!((large.popup_bounds().width - expected).abs() < 1e-9);
}
