//! Caret suggestions for a [`TextField`](super::TextField): a list hanging
//! below the word being typed, the way code editors complete it.
//!
//! Ports agg-sharp `Gui/TextWidgets/TextSuggestion.cs`,
//! `TextSuggestionController.cs` and `TextSuggestionPopup.cs`. The data types
//! ([`TextSuggestion`], [`TextSuggestionList`], [`TextSuggestionProvider`])
//! and the controller's state live here; the field side (key preview, accept,
//! hit testing) is `text_field/suggestions.rs`, and the list's drawing is
//! `text_suggestion/paint.rs`.
//!
//! Unlike a popup menu, where the highlighted row is the focused one, the
//! keyboard focus never leaves the field: the user keeps typing and each edit
//! refilters the list. The keys that drive the highlight (Up, Down, PageUp,
//! PageDown, Enter, Tab, Escape) are taken before the field can edit, submit,
//! tab away or scroll for them. Shift+Enter and Shift+Tab are left to the
//! field, Shift+Tab closing the list on its way past. Ctrl+Space (Ctrl on Mac
//! too, as in VS Code: Cmd+Space belongs to Spotlight) asks the provider at
//! the caret on demand.
//!
//! The list is the field's app-level overlay (`paint_global_overlay` /
//! `hit_test_global_overlay`), so a press on it is routed to the field
//! itself: focus never moves, and the field's edit-complete never fires.
//! Being placed each frame from the field's own transform, it follows the
//! field when the window resizes; a [`ScrollView`](super::ScrollView) above
//! the field closes it instead when it scrolls
//! ([`Widget::on_ancestor_scrolled`](crate::widget::Widget::on_ancestor_scrolled)),
//! as the list is drawn unclipped over the whole window.
//!
//! Text positions are UTF-8 byte offsets, as everywhere in `TextField`
//! (C# used UTF-16 indices; they agree for ASCII).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::geometry::{Point, Rect, Size};
use crate::text::{measure_advance, Font};
use crate::widgets::popup::Popup;

mod paint;

#[cfg(test)]
mod controller_tests;

/// One row of a suggestion list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSuggestion {
    /// What replaces the list's span when this row is accepted.
    pub insert_text: String,
    /// What the row shows on its left; the insert text by default.
    pub label: String,
    /// Drawn right aligned and dimmer, e.g. a current value.
    pub detail: Option<String>,
    /// One line shown below the list while this row is highlighted.
    pub description: Option<String>,
}

impl TextSuggestion {
    /// A row inserting and showing `insert_text`.
    pub fn new(insert_text: impl Into<String>) -> Self {
        let insert_text = insert_text.into();
        Self {
            label: insert_text.clone(),
            insert_text,
            detail: None,
            description: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// The suggestions for one caret position and the span of the text an
/// accepted one replaces: `[replace_start, replace_start + replace_length)`
/// in bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSuggestionList {
    pub replace_start: usize,
    pub replace_length: usize,
    pub suggestions: Vec<TextSuggestion>,
}

impl TextSuggestionList {
    pub fn new(
        replace_start: usize,
        replace_length: usize,
        suggestions: Vec<TextSuggestion>,
    ) -> Self {
        Self {
            replace_start,
            replace_length,
            suggestions,
        }
    }

    /// No suggestions: keeps the list closed.
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Answers what may be typed at the caret of a text field. Asked after every
/// typed character, and while the list is open after every edit, caret move
/// and accepted suggestion. An empty list keeps the list closed, so the
/// provider alone decides what triggers suggestions. Closures
/// `FnMut(&str, usize) -> TextSuggestionList` are providers.
///
/// **Offsets are UTF-8 byte offsets** into `text`, always on a character
/// boundary: `caret`, and the `replace_start` / `replace_length` of the
/// answer (as `TextField::cursor_pos` and `TextField::replace_range`).
/// A provider ported from C#, whose indices count UTF-16 code units, must
/// convert: slice `&text[..caret]` and measure spans with `str::len`, not
/// with a character count. (`TextField::set_cursor_position` alone takes a
/// character index, as C#'s `SetCursorPosition`.) The two agree for ASCII.
pub trait TextSuggestionProvider {
    /// What may be typed at byte offset `caret` of `text`; see the trait
    /// docs for the offset convention.
    fn get_suggestions(&mut self, text: &str, caret: usize) -> TextSuggestionList;
}

impl<F: FnMut(&str, usize) -> TextSuggestionList> TextSuggestionProvider for F {
    fn get_suggestions(&mut self, text: &str, caret: usize) -> TextSuggestionList {
        self(text, caret)
    }
}

/// At most this many rows show; the rest are reached by the highlight or the wheel.
pub const MAX_VISIBLE_ROWS: usize = 8;
/// C# `ThemeConfig.MenuRowHeight` (24 × DeviceScale; agg-gui is in logical units).
const ROW_HEIGHT: f64 = 24.0;
/// C# `3 * DeviceScale`.
const PAD: f64 = 3.0;
/// C# `TextSuggestionPopup.Place`'s `AnchoredPopup { Gap = 2 }`.
const GAP: f64 = 2.0;

std::thread_local! {
    /// Lists open on this thread; a scroll only walks its subtree for
    /// [`on_ancestor_scrolled`](crate::widget::Widget::on_ancestor_scrolled)
    /// while some list is showing.
    static OPEN_LISTS: Cell<usize> = const { Cell::new(0) };
}

/// Whether any suggestion list is open (the scroll walk's fast path).
pub(crate) fn any_list_open() -> bool {
    OPEN_LISTS.with(|c| c.get() > 0)
}

/// The controller's state; shared between the field and the app's handle.
pub(crate) struct State {
    pub(crate) is_open: bool,
    pub(crate) suggestions: TextSuggestionList,
    pub(crate) highlight: usize,
    last_query: Option<(String, usize)>,
    /// Set while an accept rewrites the field, so the edit it makes is not
    /// taken for the user's own.
    pub(crate) accepting: bool,
    pub(crate) first_visible_row: usize,
    /// Font the rows are measured and drawn with: the field's, from the
    /// last query, unless the app chose one ([`TextSuggestionController::with_font`]).
    pub(crate) font: Option<(Arc<Font>, f64)>,
    pub(crate) font_override: Option<(Arc<Font>, f64)>,
    /// List colours; `None` follows the visuals (see `paint.rs`).
    pub(crate) background: Option<Color>,
    pub(crate) highlight_color: Option<Color>,
    /// The field's local origin in root logical space and the viewport, from
    /// the field's last overlay paint; placement clamps against them.
    pub(crate) root_offset: Option<Point>,
    pub(crate) viewport: Option<Size>,
    /// Placed list, in the field's local coordinates.
    pub(crate) rect: Rect,
}

/// Shows a [`TextSuggestionProvider`]'s suggestions below the caret of a
/// [`TextField`](super::TextField). A cheap clonable handle: attach it with
/// [`TextField::with_text_suggestions`](super::TextField::with_text_suggestions)
/// and keep a clone to read its state.
#[derive(Clone)]
pub struct TextSuggestionController {
    pub(crate) state: Rc<RefCell<State>>,
    provider: Rc<RefCell<Box<dyn TextSuggestionProvider>>>,
}

impl TextSuggestionController {
    pub fn new(provider: impl TextSuggestionProvider + 'static) -> Self {
        Self {
            state: Rc::new(RefCell::new(State {
                is_open: false,
                suggestions: TextSuggestionList::empty(),
                highlight: 0,
                last_query: None,
                accepting: false,
                first_visible_row: 0,
                font: None,
                font_override: None,
                background: None,
                highlight_color: None,
                root_offset: None,
                viewport: None,
                rect: Rect::default(),
            })),
            provider: Rc::new(RefCell::new(Box::new(provider))),
        }
    }

    /// Measure and draw the rows with `font` at `size` instead of the field's
    /// font (C# draws them at the theme's `DefaultFontSize`).
    pub fn with_font(self, font: Arc<Font>, size: f64) -> Self {
        self.state.borrow_mut().font_override = Some((font, size));
        self
    }

    /// The list's background; default `Visuals::bg_color`, the app
    /// background (C# `theme.BackgroundColor`).
    pub fn with_background_color(self, color: Color) -> Self {
        self.state.borrow_mut().background = Some(color);
        self
    }

    /// The highlighted row's tint; default the accent at alpha 128/255 (C#
    /// `theme.AccentMimimalOverlay`).
    pub fn with_highlight_color(self, color: Color) -> Self {
        self.state.borrow_mut().highlight_color = Some(color);
        self
    }

    pub fn is_open(&self) -> bool {
        self.state.borrow().is_open
    }

    /// What the list is showing; empty while it is closed.
    pub fn suggestions(&self) -> TextSuggestionList {
        self.state.borrow().suggestions.clone()
    }

    /// The labels of the rows, in order.
    pub fn labels(&self) -> Vec<String> {
        let st = self.state.borrow();
        st.suggestions
            .suggestions
            .iter()
            .map(|s| s.label.clone())
            .collect()
    }

    pub fn highlight_index(&self) -> usize {
        self.state.borrow().highlight
    }

    /// Rows in view: at most [`MAX_VISIBLE_ROWS`].
    pub fn visible_row_count(&self) -> usize {
        self.state.borrow().visible_row_count()
    }

    /// The list's bounds in root logical (Y-up) space; `None` while closed or
    /// before the field has painted it.
    pub fn popup_bounds(&self) -> Option<Rect> {
        let st = self.state.borrow();
        let off = st.root_offset?;
        st.is_open.then(|| translate(st.rect, off))
    }

    /// The bounds of suggestion `index`'s row in root logical space; `None`
    /// when it is scrolled out of view (C# returned an empty rectangle).
    pub fn row_bounds(&self, index: usize) -> Option<Rect> {
        let st = self.state.borrow();
        let off = st.root_offset?;
        let row = st.row_bounds_local(index)?;
        st.is_open.then(|| translate(row, off))
    }

    /// Closes the list.
    pub fn close(&self) {
        self.state.borrow_mut().close();
    }

    /// Asks the provider about `text` at `caret` for a focused field, showing
    /// the answer or closing when there is none. `true` when the list was
    /// (re)filled, so the caller places it again.
    pub(crate) fn requery(&self, text: &str, caret: usize, font: (Arc<Font>, f64)) -> bool {
        {
            let mut st = self.state.borrow_mut();
            // Typing moves the caret and changes the text, and both say so:
            // ask once per state, not per event.
            if st.is_open
                && st
                    .last_query
                    .as_ref()
                    .is_some_and(|(t, c)| t == text && *c == caret)
            {
                return true;
            }
            st.last_query = Some((text.to_string(), caret));
        }
        // The provider is app code: run it with no state borrowed.
        let list = self.provider.borrow_mut().get_suggestions(text, caret);
        let mut st = self.state.borrow_mut();
        if list.suggestions.is_empty() {
            st.close();
            return false;
        }
        st.suggestions = list;
        st.highlight = 0;
        st.first_visible_row = 0;
        st.font = Some(st.font_override.clone().unwrap_or(font));
        if !st.is_open {
            st.is_open = true;
            OPEN_LISTS.with(|c| c.set(c.get() + 1));
        }
        true
    }

    /// Forgets the last query, so the next requery asks again even when
    /// the text and caret are unchanged (Ctrl+Space).
    pub(crate) fn forget_last_query(&self) {
        self.state.borrow_mut().last_query = None;
    }

    /// Moves the highlight by `delta` rows, wrapping at the ends when asked.
    pub(crate) fn move_highlight(&self, delta: i64, wrap: bool) {
        let mut st = self.state.borrow_mut();
        let count = st.suggestions.suggestions.len() as i64;
        if count == 0 {
            return;
        }
        let next = st.highlight as i64 + delta;
        let next = if wrap {
            ((next % count) + count) % count
        } else {
            next.min(count - 1).max(0)
        };
        st.highlight = next as usize;
        st.scroll_highlight_into_view();
    }

    /// The wheel over the list: scrolls the rows three at a time and brings
    /// the highlight along to the nearest row in view, as Enter, Tab and the
    /// footer act on it. `delta_y > 0` shows rows above (C# `WheelDelta`).
    pub(crate) fn wheel(&self, delta_y: f64) {
        let mut st = self.state.borrow_mut();
        let max_first = st.suggestions.suggestions.len() as i64 - st.visible_row_count() as i64;
        let step = if delta_y > 0.0 {
            1
        } else if delta_y < 0.0 {
            -1
        } else {
            0
        };
        let first = (st.first_visible_row as i64 - step * 3)
            .min(max_first)
            .max(0);
        st.first_visible_row = first as usize;
        let last = st.first_visible_row + st.visible_row_count().saturating_sub(1);
        st.highlight = st.highlight.clamp(st.first_visible_row, last);
    }

    /// Places the list below `anchor` (where the replaced text starts, in
    /// the field's local coordinates), clamped into the visible viewport.
    pub(crate) fn place(&self, anchor: Rect) {
        self.state.borrow_mut().place(anchor);
    }

    /// Records where the field is in root space this frame.
    pub(crate) fn set_root_frame(&self, root_offset: Point, viewport: Option<Size>) {
        let mut st = self.state.borrow_mut();
        st.root_offset = Some(root_offset);
        st.viewport = viewport;
    }

    /// The row a field-local point is on, if it is on one in view.
    pub(crate) fn row_at(&self, local: Point) -> Option<usize> {
        let st = self.state.borrow();
        if !st.is_open {
            return None;
        }
        let first = st.first_visible_row;
        (first..first + st.visible_row_count())
            .find(|&i| st.row_bounds_local(i).is_some_and(|r| r.contains(local)))
    }

    /// Whether a field-local point is on the open list.
    pub(crate) fn contains(&self, local: Point) -> bool {
        let st = self.state.borrow();
        st.is_open && st.rect.contains(local)
    }
}

fn translate(r: Rect, by: Point) -> Rect {
    Rect::new(r.x + by.x, r.y + by.y, r.width, r.height)
}

/// A controller dropped while its list is open no longer counts as open.
impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}

impl State {
    pub(crate) fn close(&mut self) {
        if self.is_open {
            OPEN_LISTS.with(|c| c.set(c.get().saturating_sub(1)));
        }
        self.is_open = false;
        self.suggestions = TextSuggestionList::empty();
        self.last_query = None;
    }

    pub(crate) fn visible_row_count(&self) -> usize {
        self.suggestions.suggestions.len().min(MAX_VISIBLE_ROWS)
    }

    pub(crate) fn highlighted_description(&self) -> Option<&str> {
        self.suggestions
            .suggestions
            .get(self.highlight)
            .and_then(|s| s.description.as_deref())
            .filter(|d| !d.is_empty())
    }

    pub(crate) fn has_footer(&self) -> bool {
        self.highlighted_description().is_some()
    }

    fn scroll_highlight_into_view(&mut self) {
        let visible = self.visible_row_count();
        if self.highlight < self.first_visible_row {
            self.first_visible_row = self.highlight;
        } else if self.highlight >= self.first_visible_row + visible {
            self.first_visible_row = self.highlight + 1 - visible;
        }
    }

    pub(crate) fn measure(&self, text: &str) -> f64 {
        match &self.font {
            Some((font, size)) => measure_advance(font, text, *size),
            None => 0.0,
        }
    }

    /// Row `index`'s bounds in the field's local space; `None` out of view.
    pub(crate) fn row_bounds_local(&self, index: usize) -> Option<Rect> {
        let slot = index.checked_sub(self.first_visible_row)?;
        if slot >= self.visible_row_count() {
            return None;
        }
        let top = self.rect.top() - PAD - slot as f64 * ROW_HEIGHT;
        Some(Rect::new(
            self.rect.x + PAD,
            top - ROW_HEIGHT,
            (self.rect.width - 2.0 * PAD).max(0.0),
            ROW_HEIGHT,
        ))
    }

    /// C# `TextSuggestionPopup.Resize`: wide enough for the widest row (label
    /// plus detail) and the footer, between 150 and 480; tall enough for the
    /// visible rows and the footer.
    fn size(&self) -> Size {
        let mut widest: f64 = 0.0;
        for s in &self.suggestions.suggestions {
            let mut row = self.measure(&s.label);
            if let Some(detail) = s.detail.as_deref().filter(|d| !d.is_empty()) {
                row += 6.0 * PAD + self.measure(detail);
            }
            widest = widest.max(row);
        }
        if let Some(d) = self.highlighted_description() {
            widest = widest.max(self.measure(d));
        }
        let width = (widest + 4.0 * PAD).clamp(150.0, 480.0);
        let rows = self.visible_row_count() + usize::from(self.has_footer());
        Size::new(width, rows as f64 * ROW_HEIGHT + 2.0 * PAD)
    }

    fn place(&mut self, anchor: Rect) {
        let size = self.size();
        let off = self.root_offset.unwrap_or(Point::ORIGIN);
        let mut popup = Popup::new();
        popup.gap = GAP;
        popup.set_anchor(translate(anchor, off));
        popup.set_size(size);
        let placed = match self.viewport {
            Some(vp) => popup.rect(vp),
            None => popup.align.place_child(popup.anchor(), size, GAP),
        };
        self.rect = translate(placed, Point::new(-off.x, -off.y));
    }
}
