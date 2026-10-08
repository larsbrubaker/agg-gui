//! `RadioButton` — one standalone radio option, exclusive with the radio
//! buttons that share its parent (agg-sharp's `Gui/RadioButton/RadioButton.cs`).
//!
//! [`RadioGroup`](super::RadioGroup) draws all of its options inside one
//! widget, so an option cannot be named, found, laid out or disabled on its
//! own. A `RadioButton` is a widget per option: put several in any container
//! and checking one unchecks the others (C#'s `UncheckSiblings`, which walks
//! `Parent.Children`).
//!
//! # How siblings are unchecked
//!
//! A widget cannot reach its siblings while it handles an event (its parent
//! is borrowed by the dispatch walk). So a click checks the button and
//! *posts* itself ([`WidgetId`]) as the pending check; the dispatch walk in
//! `widget/tree.rs` settles it one level up, where the parent is in hand
//! ([`settle_pending_check`]): every other `RadioButton` child is unchecked,
//! then the clicked button's `checked_state_changed` and `on_click`
//! callbacks run. This is C#'s order (`Checked = true` unchecks the siblings,
//! raises `CheckedStateChanged`, then the `Click` handlers run), and it all
//! happens inside the same dispatch, so a click callback already sees its
//! siblings unchecked.
//!
//! Code that checks a button programmatically uses [`check_child`] on the
//! parent (the counterpart of C#'s `radioButton.Checked = true`);
//! [`RadioButton::set_checked`] changes only the button it is called on.
//!
//! ```text
//! RadioButton (circle drawn via paths)
//!   └── Label (text, backbuffered)
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{Widget, WidgetId};
use crate::widgets::label::Label;

/// Outer circle radius (matches `RadioGroup`'s dot).
const DOT_R: f64 = 7.0;
/// Room around the circle for its stroke and the focus ring.
const FOCUS_PAD: f64 = 2.0;
const GAP: f64 = 8.0;

thread_local! {
    /// The radio button whose click is waiting for its parent to uncheck
    /// its siblings and run its callbacks.
    static PENDING_CHECK: Cell<Option<WidgetId>> = const { Cell::new(None) };
}

/// A single radio option with a text label. See the module docs.
pub struct RadioButton {
    bounds: Rect,
    /// `children[0]` is the [`Label`] that renders the text.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    font: Arc<Font>,
    label_text: String,
    font_size: f64,
    checked: bool,
    /// When set, the authoritative checked state (kept in step both ways).
    state_cell: Option<Rc<Cell<bool>>>,
    enabled: bool,
    hovered: bool,
    focused: bool,
    /// A left press landed on the button; a release on it clicks.
    pressed: bool,
    on_checked_state_changed: Option<Box<dyn FnMut(bool)>>,
    on_click: Option<Box<dyn FnMut()>>,
}

impl RadioButton {
    /// An unchecked, enabled radio button labelled `label`.
    pub fn new(label: impl Into<String>, font: Arc<Font>) -> Self {
        let font_size = crate::font_settings::default_font_size_or(14.0);
        let label_text: String = label.into();
        let label_widget = Label::new(&label_text, Arc::clone(&font)).with_font_size(font_size);
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(label_widget)],
            base: WidgetBase::new(),
            font,
            label_text,
            font_size,
            checked: false,
            state_cell: None,
            enabled: true,
            hovered: false,
            focused: false,
            pressed: false,
            on_checked_state_changed: None,
            on_click: None,
        }
    }

    /// Start checked or unchecked, without unchecking siblings or calling
    /// back (use it on a freshly built set, one checked).
    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        if let Some(cell) = &self.state_cell {
            cell.set(checked);
        }
        self
    }

    /// Mirror the checked state in `cell`: the button reads it when it paints
    /// and answers [`checked`](Self::checked), and writes every change to it.
    pub fn with_state_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.checked = cell.get();
        self.state_cell = Some(cell);
        self
    }

    /// Enable or disable this option alone (C#'s `Enabled`). A disabled
    /// radio button ignores the pointer and keys and paints dimmed.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// C#'s `CheckedStateChanged`: called with the new state whenever it
    /// changes, including when a sibling's check unchecks this one.
    pub fn on_checked_state_changed(mut self, cb: impl FnMut(bool) + 'static) -> Self {
        self.on_checked_state_changed = Some(Box::new(cb));
        self
    }

    /// C#'s `Click`: called after a click has checked this button and
    /// unchecked its siblings.
    pub fn on_click(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(cb));
        self
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self.children[0] =
            Box::new(Label::new(&self.label_text, Arc::clone(&self.font)).with_font_size(size));
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }

    /// Whether this option is checked (C#'s `Checked`).
    pub fn checked(&self) -> bool {
        match &self.state_cell {
            Some(cell) => cell.get(),
            None => self.checked,
        }
    }

    /// Set this button's checked state, calling `checked_state_changed` when
    /// it changes. Siblings are untouched; [`check_child`] checks one option
    /// of a parent and unchecks the rest.
    pub fn set_checked(&mut self, checked: bool) {
        if self.store_checked(checked) {
            self.notify_checked_state();
        }
    }

    /// Enable or disable this option (C#'s `Enabled`).
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            if !enabled {
                self.hovered = false;
                self.pressed = false;
            }
            crate::animation::request_draw();
        }
    }

    /// Store `checked` without calling back; `true` when it changed.
    fn store_checked(&mut self, checked: bool) -> bool {
        if self.checked() == checked {
            return false;
        }
        self.checked = checked;
        if let Some(cell) = &self.state_cell {
            cell.set(checked);
        }
        crate::animation::request_draw();
        true
    }

    fn notify_checked_state(&mut self) {
        let checked = self.checked();
        if let Some(cb) = self.on_checked_state_changed.as_mut() {
            cb(checked);
        }
    }

    /// A click (or Space): check, and leave the siblings and callbacks to
    /// the parent's settle step.
    fn activate(&mut self) {
        PENDING_CHECK.with(|p| p.set(Some(WidgetId::of(&*self))));
    }

    fn label_color(&self, v: &crate::theme::Visuals) -> Color {
        let c = v.text_color;
        if self.enabled {
            c
        } else {
            Color::rgba(c.r, c.g, c.b, c.a * 0.4)
        }
    }
}

fn as_radio(widget: &mut dyn Widget) -> Option<&mut RadioButton> {
    widget.as_any_mut()?.downcast_mut::<RadioButton>()
}

/// Check the child at `index` of `parent` and uncheck every other
/// `RadioButton` child (C#'s `radioButton.Checked = true`), calling back
/// each button whose state changed: the siblings first, then the checked one.
/// Does nothing when that child is not a `RadioButton`.
pub fn check_child(parent: &mut dyn Widget, index: usize) {
    check_child_then(parent, index, false);
}

/// [`check_child`], and run the checked button's click callback last when
/// `clicked`.
fn check_child_then(parent: &mut dyn Widget, index: usize, clicked: bool) {
    let changed = {
        let Some(radio) = parent
            .children_mut()
            .get_mut(index)
            .and_then(|c| as_radio(c.as_mut()))
        else {
            return;
        };
        radio.store_checked(true)
    };
    for (i, sibling) in parent.children_mut().iter_mut().enumerate() {
        if i != index {
            if let Some(radio) = as_radio(sibling.as_mut()) {
                radio.set_checked(false);
            }
        }
    }
    if let Some(radio) = as_radio(parent.children_mut()[index].as_mut()) {
        if changed {
            radio.notify_checked_state();
        }
        if clicked {
            if let Some(cb) = radio.on_click.as_mut() {
                cb();
            }
        }
    }
}

/// Settle a click posted by the radio button at child `index` of `parent`:
/// called by the dispatch walk once that child's event has been handled.
pub(crate) fn settle_pending_check(parent: &mut dyn Widget, index: usize) {
    let Some(pending) = PENDING_CHECK.with(Cell::get) else {
        return;
    };
    let is_child = parent
        .children()
        .get(index)
        .is_some_and(|c| WidgetId::of(c.as_ref()) == pending);
    if is_child {
        PENDING_CHECK.with(|p| p.set(None));
        check_child_then(parent, index, true);
    }
}

/// Settle a click posted by the dispatch root itself (a radio button with no
/// parent in this tree): it has no siblings, so it only checks and calls back.
pub(crate) fn settle_pending_check_at_root(root: &mut dyn Widget) {
    let Some(pending) = PENDING_CHECK.with(Cell::get) else {
        return;
    };
    if WidgetId::of(&*root) != pending {
        return;
    }
    PENDING_CHECK.with(|p| p.set(None));
    if let Some(radio) = as_radio(root) {
        if radio.store_checked(true) {
            radio.notify_checked_state();
        }
        if let Some(cb) = radio.on_click.as_mut() {
            cb();
        }
    }
}

impl Widget for RadioButton {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "RadioButton"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn is_enabled(&self) -> bool {
        self.enabled
    }
    fn is_focusable(&self) -> bool {
        self.enabled
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    /// The label is part of the option, not a separate target: the pointer
    /// anywhere on the row belongs to the radio button.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        true
    }

    fn layout(&mut self, available: Size) -> Size {
        let dot_slot = (DOT_R + FOCUS_PAD) * 2.0;
        let h = dot_slot.max(self.font_size * 1.25);
        let label_avail = (available.width - dot_slot - GAP).max(0.0);
        let s = self.children[0].layout(Size::new(label_avail, h));
        self.children[0].set_bounds(Rect::new(
            dot_slot + GAP,
            (h - s.height) * 0.5,
            s.width,
            s.height,
        ));
        let w = (dot_slot + GAP + s.width).min(available.width.max(dot_slot));
        self.bounds = Rect::new(self.bounds.x, self.bounds.y, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let cx = FOCUS_PAD + DOT_R;
        let cy = self.bounds.height * 0.5;
        let checked = self.checked();

        if self.focused && self.enabled {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.circle(cx, cy, DOT_R + 1.5);
            ctx.stroke();
        }

        // The same circle RadioGroup draws for each of its options.
        let border = if !self.enabled {
            v.widget_stroke.with_alpha(0.4)
        } else if checked {
            v.accent
        } else if self.hovered {
            v.widget_bg_hovered
        } else {
            v.widget_stroke
        };
        let bg = if checked && self.enabled {
            v.accent
        } else if checked {
            v.accent.with_alpha(0.4)
        } else {
            v.widget_bg
        };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.circle(cx, cy, DOT_R);
        ctx.fill();
        ctx.set_stroke_color(border);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.circle(cx, cy, DOT_R);
        ctx.stroke();
        if checked {
            ctx.set_fill_color(v.widget_bg);
            ctx.begin_path();
            ctx.circle(cx, cy, DOT_R * 0.45);
            ctx.fill();
        }

        let label_color = self.label_color(&v);
        self.children[0].set_label_color(label_color);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.enabled {
            return EventResult::Ignored;
        }
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if was != self.hovered {
                    crate::animation::request_draw();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                self.pressed = true;
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                // C#'s click: the press and the release both on the button.
                if std::mem::take(&mut self.pressed) && self.hit_test(*pos) {
                    self.activate();
                }
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::Char(' '),
                ..
            } => {
                self.activate();
                EventResult::Consumed
            }
            Event::FocusGained | Event::FocusLost => {
                let focused = matches!(event, Event::FocusGained);
                if self.focused != focused {
                    self.focused = focused;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}
