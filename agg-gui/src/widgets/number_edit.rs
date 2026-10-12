//! `NumberEdit` — a single-line number box that refuses keys it cannot read.
//!
//! Port of agg-sharp `Gui/TextWidgets/NumberEdit.cs` and
//! `InternalNumberEdit.cs`. C#'s two-widget edit control (an outer
//! `NumberEdit` around an `InternalNumberEdit`) becomes one wrapper around
//! exactly one [`TextField`] child that fills it:
//!
//! - **typing** — a whole-edit judge on the field
//!   ([`TextField::with_text_filter`]) refuses any insertion that would leave
//!   text the box cannot read (C# `OnKeyPress` / `CanType`, `state.rs`);
//! - **Up / Down** step the value by the increment and commit, seen in
//!   [`Widget::preview_event`] before the field's own caret handling;
//! - **commit** — Enter, or focus loss with changed text — writes the value
//!   back as a number, or falls back to the last good value when the text is
//!   unreadable (C# `OnEditComplete`), then fires `on_edit_complete`.
//!
//! Numbers are shown and read through `display_number.rs` (C#'s
//! `DisplayNumber`, invariant culture). Unit tests are in
//! `number_edit/tests.rs`; TextEditTests' NumEdit tests are in
//! agg-gui-automation's `tests/text_edit/num_edit.rs`.

mod display_number;
mod state;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub use display_number::{format as format_display_number, try_read as read_display_number};
pub use state::{NumberEditError, TextValueParser};

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key};
use crate::geometry::{Rect, Size};
use crate::layout_props::WidgetBase;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::TextField;
use state::NumberState;

/// C#'s `NumberEdit` constructor options. The defaults are C#'s: no
/// negatives, no decimals, `int.MinValue..=int.MaxValue`, increment 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberEditOptions {
    pub starting_value: f64,
    pub allow_negatives: bool,
    pub allow_decimals: bool,
    pub min_value: f64,
    pub max_value: f64,
    pub increment: f64,
}

impl Default for NumberEditOptions {
    fn default() -> Self {
        Self {
            starting_value: 0.0,
            allow_negatives: false,
            allow_decimals: false,
            min_value: i32::MIN as f64,
            max_value: i32::MAX as f64,
            increment: 1.0,
        }
    }
}

/// What a commit hands `on_edit_complete`.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberCommit {
    /// The committed value (C# `Value` after the commit).
    pub value: f64,
    /// The field's text after the commit: the value written back as a
    /// number, or an expression left for the owner to read.
    pub text: String,
    /// C# `LastParsedText`: the entry the parser read the value out of.
    pub last_parsed_text: Option<String>,
}

type CommitCallback = Box<dyn FnMut(&NumberCommit)>;

/// A number box; see the module docs.
pub struct NumberEdit {
    bounds: Rect,
    /// Exactly one child: the [`TextField`] being edited.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    state: Rc<RefCell<NumberState>>,
    /// Set by the field's `on_edit_complete`; the commit runs after the
    /// field's event returns, where the field can be written.
    commit_pending: Rc<RefCell<bool>>,
    arrow_stepping: bool,
    on_edit_complete: Option<CommitCallback>,
    /// For the stand-in field [`map_field`](Self::map_field) swaps in.
    font: Arc<Font>,
}

impl NumberEdit {
    /// C# `new NumberEdit(startingValue, ..., allowNegatives, allowDecimals,
    /// minValue, maxValue, increment)`. Like C#, a box without negatives
    /// raises its minimum to 0 first, and a negative or fractional start,
    /// limit or increment the options do not allow is an error.
    pub fn new(font: Arc<Font>, options: NumberEditOptions) -> Result<Self, NumberEditError> {
        let mut options = options;
        if !options.allow_negatives {
            options.min_value = options.min_value.max(0.0);
        }
        let numbers = [
            options.starting_value,
            options.increment,
            options.min_value,
            options.max_value,
        ];
        if !options.allow_negatives && numbers.iter().any(|v| *v < 0.0) {
            return Err(NumberEditError::NegativeNotAllowed);
        }
        if !options.allow_decimals && numbers.iter().any(|v| *v != v.trunc()) {
            return Err(NumberEditError::FractionNotAllowed);
        }
        let state = Rc::new(RefCell::new(NumberState {
            allow_negatives: options.allow_negatives,
            allow_decimals: options.allow_decimals,
            min_value: options.min_value,
            max_value: options.max_value,
            increment: options.increment,
            max_decimal_places: -1,
            allow_expression_entry: false,
            parser: None,
            last_parsed_text: None,
            last_value: options.starting_value,
        }));
        let commit_pending = Rc::new(RefCell::new(false));
        let judge = Rc::clone(&state);
        let pending = Rc::clone(&commit_pending);
        let field = TextField::new(Arc::clone(&font))
            .with_text(display_number::format(options.starting_value, -1))
            .with_text_filter(move |inserted, typed| judge.borrow().can_type(inserted, typed))
            .on_edit_complete(move |_| *pending.borrow_mut() = true);
        Ok(Self {
            bounds: Rect::default(),
            children: vec![Box::new(field)],
            base: WidgetBase::new(),
            state,
            commit_pending,
            arrow_stepping: true,
            on_edit_complete: None,
            font,
        })
    }

    /// Configure the inner [`TextField`] (font size, padding, select-all on
    /// focus, ...). Its `on_edit_complete` and text filter are the box's own;
    /// replacing them breaks commits and key refusal — use the box's
    /// [`on_edit_complete`](Self::on_edit_complete) instead.
    pub fn map_field(mut self, f: impl FnOnce(TextField) -> TextField) -> Self {
        let placeholder = TextField::new(Arc::clone(&self.font));
        if let Some(field) = self.text_field_mut() {
            let taken = std::mem::replace(field, placeholder);
            *field = f(taken);
        }
        self
    }

    /// The inner text field, unless a caller took it out of `children_mut`.
    pub fn text_field(&self) -> Option<&TextField> {
        self.children.first()?.as_any()?.downcast_ref::<TextField>()
    }

    /// Mutable [`text_field`](Self::text_field) (C#'s `Text` setter,
    /// `CharIndexToInsertBefore`, ...).
    pub fn text_field_mut(&mut self) -> Option<&mut TextField> {
        self.children
            .first_mut()?
            .as_any_mut()?
            .downcast_mut::<TextField>()
    }

    /// C# `Text`.
    pub fn text(&self) -> String {
        self.text_field().map(TextField::text).unwrap_or_default()
    }

    /// C# `Value` (getter): the parser's reading of the text, else the
    /// plain numeric one, else the error value (`0` clamped into range).
    /// Updates [`last_parsed_text`](Self::last_parsed_text).
    pub fn value(&self) -> f64 {
        let text = self.text();
        self.state.borrow_mut().read_value(&text)
    }

    /// C# `Value` (setter): clamp to the range, remember it as the last good
    /// value and show it. A programmatic write fires no callback.
    pub fn set_value(&mut self, value: f64) {
        let new_value = self.state.borrow().validate_range(value);
        let old_readable = display_number::try_read(&self.text()).is_some();
        let current = self.value();
        let places = {
            let mut st = self.state.borrow_mut();
            st.last_value = new_value;
            st.max_decimal_places
        };
        let text = display_number::format(new_value, places);
        if let Some(field) = self.text_field_mut() {
            field.set_text(text.as_str());
            // C#: when the text could not be read, the caret goes to the end
            // of the number that replaced it.
            if new_value == current && !old_readable {
                field.set_cursor_position(text.chars().count());
            }
        }
    }

    /// C# `MinValue`.
    pub fn min_value(&self) -> f64 {
        self.state.borrow().min_value
    }

    /// C# `MinValue` setter (does not re-clamp the current value, as C#).
    pub fn set_min_value(&mut self, value: f64) -> Result<(), NumberEditError> {
        let mut st = self.state.borrow_mut();
        st.check_limit(value)?;
        st.min_value = value;
        Ok(())
    }

    /// C# `MaxValue`.
    pub fn max_value(&self) -> f64 {
        self.state.borrow().max_value
    }

    /// C# `MaxValue` setter (does not re-clamp the current value, as C#).
    pub fn set_max_value(&mut self, value: f64) -> Result<(), NumberEditError> {
        let mut st = self.state.borrow_mut();
        st.check_limit(value)?;
        st.max_value = value;
        Ok(())
    }

    /// The Up / Down step.
    pub fn increment(&self) -> f64 {
        self.state.borrow().increment
    }

    /// C# `MaxDecimalsPlaces`: decimals shown (at most 10); `<= 0` shows the
    /// shortest round-trip text. Re-shows the current value, as C#.
    pub fn set_max_decimal_places(&mut self, places: i32) {
        self.state.borrow_mut().max_decimal_places = places;
        let value = self.value();
        self.set_value(value);
    }

    /// Builder form of [`set_max_decimal_places`](Self::set_max_decimal_places).
    pub fn with_max_decimal_places(mut self, places: i32) -> Self {
        self.set_max_decimal_places(places);
        self
    }

    /// C# `TextValueParser`: reads text a plain number parse cannot. Setting
    /// one also lets letters and spaces be typed, since whatever spelling
    /// the parser understands has to be typeable first.
    pub fn set_text_value_parser(&mut self, parser: Option<TextValueParser>) {
        self.state.borrow_mut().parser = parser;
    }

    /// Builder form of [`set_text_value_parser`](Self::set_text_value_parser).
    pub fn with_text_value_parser(
        mut self,
        parser: impl Fn(&str) -> Option<f64> + 'static,
    ) -> Self {
        self.set_text_value_parser(Some(Box::new(parser)));
        self
    }

    /// C# `LastParsedText`: the text the parser last read a value out of,
    /// `None` when the value last came from a plain number. A commit keeps
    /// it across rewriting the text as the number.
    pub fn last_parsed_text(&self) -> Option<String> {
        self.state.borrow().last_parsed_text.clone()
    }

    /// C# `ClearLastParsedText`.
    pub fn clear_last_parsed_text(&mut self) {
        self.state.borrow_mut().last_parsed_text = None;
    }

    /// C# `AllowExpressionEntry`: text beginning with `=` may be typed (for
    /// an owner that takes expressions over through `on_change`), and a
    /// commit leaves such text alone.
    pub fn allow_expression_entry(&self) -> bool {
        self.state.borrow().allow_expression_entry
    }

    pub fn set_allow_expression_entry(&mut self, allow: bool) {
        self.state.borrow_mut().allow_expression_entry = allow;
    }

    pub fn with_allow_expression_entry(mut self, allow: bool) -> Self {
        self.set_allow_expression_entry(allow);
        self
    }

    /// Whether Up / Down step the value (default on). Turn it off when the
    /// owner steps the value itself through the field's
    /// [`with_key_intercept`](TextField::with_key_intercept).
    pub fn with_arrow_stepping(mut self, on: bool) -> Self {
        self.arrow_stepping = on;
        self
    }

    /// C# `TextChanged`: called with the text after every user edit.
    pub fn on_change(self, cb: impl FnMut(&str) + 'static) -> Self {
        self.map_field(|field| field.on_change(cb))
    }

    /// C# `EditComplete`: called after every commit (Enter, focus loss with
    /// changed text, Up / Down).
    pub fn on_edit_complete(mut self, cb: impl FnMut(&NumberCommit) + 'static) -> Self {
        self.on_edit_complete = Some(Box::new(cb));
        self
    }

    /// C# `OnEditComplete`: commit what the field holds. Text the box cannot
    /// read keeps the last good value instead (a stray `.` would otherwise
    /// commit the error value silently); an expression (`=...`, with
    /// expression entry on) is the owner's to read and is left as typed.
    pub fn commit(&mut self) {
        let text = self.text();
        let (readable, expression) = {
            let st = self.state.borrow();
            let expression = st.allow_expression_entry && text.trim_start().starts_with('=');
            (st.commit_is_readable(&text), expression)
        };
        if !readable {
            let last = self.state.borrow().last_value;
            self.set_value(last);
        } else if !expression {
            // Reading the value decides where it came from, and writing it
            // back clears that; hold the parsed entry across the write so the
            // text the user actually typed is still readable to the commit.
            let committed_value = self.value();
            let committed_parsed = self.last_parsed_text();
            self.set_value(committed_value);
            self.state.borrow_mut().last_parsed_text = committed_parsed;
        }
        if let Some(field) = self.text_field_mut() {
            field.mark_text_committed();
        }
        // Every branch leaves the committed value as the last good one (an
        // expression leaves the previous one standing); reading the text
        // again here would clear the parsed entry just kept.
        let commit = NumberCommit {
            value: self.state.borrow().last_value,
            text: self.text(),
            last_parsed_text: self.last_parsed_text(),
        };
        if let Some(mut cb) = self.on_edit_complete.take() {
            cb(&commit);
            self.on_edit_complete = Some(cb);
        }
    }

    /// Up / Down: C# `Value = Value ± increment; OnEditComplete`.
    fn step(&mut self, up: bool) {
        let increment = self.increment();
        let value = self.value();
        self.set_value(if up {
            value + increment
        } else {
            value - increment
        });
        self.commit();
    }

    /// Deliver `event` to the field directly, then run a commit it asked for.
    fn forward_to_field(&mut self, event: &Event) -> EventResult {
        let result = self
            .children
            .first_mut()
            .map_or(EventResult::Ignored, |field| field.on_event(event));
        if std::mem::take(&mut *self.commit_pending.borrow_mut()) {
            self.commit();
            crate::animation::request_draw();
        }
        result
    }

    /// Whether Up / Down step the value now: stepping on, field editable.
    fn steps_on_arrows(&self) -> bool {
        self.arrow_stepping && self.text_field().is_some_and(|field| !field.read_only)
    }
}

impl Widget for NumberEdit {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "NumberEdit"
    }
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn layout(&mut self, available: Size) -> Size {
        let Some(field) = self.children.first_mut() else {
            return Size::ZERO;
        };
        let size = field.layout(available);
        field.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        size
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn preview_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::KeyDown {
                key: Key::ArrowUp, ..
            } if self.steps_on_arrows() => {
                self.step(true);
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::ArrowDown,
                ..
            } if self.steps_on_arrows() => {
                self.step(false);
                EventResult::Consumed
            }
            // The field commits on these; the box delivers them itself so its
            // commit runs right after the field's, and claims them so the
            // dispatcher does not deliver them a second time.
            Event::KeyDown {
                key: Key::Enter, ..
            }
            | Event::FocusLost => {
                self.forward_to_field(event);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
