//! The notice a browser shows after each change to its history (`mod.rs`): [`BrowserNotice`], a
//! short line such as "Moved 12 items to Brackets" in a rounded box, with an Undo button (Redo
//! after an undo), held for a few seconds and then faded away. An app places it where it likes
//! (the composed browser puts it at the bottom of the list). A line too long for a narrow browser
//! is cut back to "..." so its button stays in view, and shows whole as the tooltip. The box is
//! the look's text colour with its background colour's text on it, so it stands out from the list
//! in a light or a dark theme. The button ([`NoticeButton`], which an app may reuse for a notice
//! of its own) takes the keyboard (Tab reaches it; Enter or Space presses it), and its click runs
//! the same undo or redo as the keys.
//!
//! The notice reads the history's latest `Notice` at each layout; a new one (a new id) starts the
//! hold and fade again. While the pointer is over the notice, or its button has the keyboard, it
//! stays at full strength, so a user reaching for Undo never sees it fade under them; once both
//! end it holds its full time again before fading. While it shows it asks the UI clock for the
//! frames it needs: one when the hold ends, then one every few dozen milliseconds while it fades.
//! Its age is counted from the instant it was shown on the UI clock (`agg_gui::clock`), never as
//! time since the clock's epoch, which a virtual clock set before it would hold at zero.
//! Its names are the look's (`BrowserNames::notice`, `notice_button`), so two browsers' notices
//! can be told apart.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use agg_gui::text::Font;
use agg_gui::{
    measure_text_metrics, Color, DrawCtx, Event, EventResult, Key, MouseButton, Point, Rect, Size,
    Widget, WidgetBase,
};
use agg_gui_files::tr;
use web_time::Instant;

use super::{Notice, NoticeAction};
use crate::fitted_text::fit_text;
use crate::state::BrowserState;

/// How long the notice shows at full strength.
pub const NOTICE_HOLD_MS: u64 = 5000;
/// How long it then takes to fade away.
pub const NOTICE_FADE_MS: u64 = 1000;
/// The frame interval while it fades.
const FADE_FRAME: Duration = Duration::from_millis(40);
/// Room around the text and between the text and the button.
const PADDING: f64 = 10.0;

/// The notice's strength `age_ms` after it was shown: 1 while held, falling to 0 as it fades.
pub fn notice_alpha(age_ms: u64) -> f64 {
    if age_ms <= NOTICE_HOLD_MS {
        1.0
    } else {
        1.0 - ((age_ms - NOTICE_HOLD_MS) as f64 / NOTICE_FADE_MS as f64).min(1.0)
    }
}

/// A colour at `alpha` of its own opacity.
fn faded(color: Color, alpha: f64) -> Color {
    Color {
        a: color.a * alpha as f32,
        ..color
    }
}

/// The word on the notice's button for `action`.
fn action_label(action: NoticeAction) -> String {
    match action {
        NoticeAction::Undo => tr("Undo"),
        NoticeAction::Redo => tr("Redo"),
    }
}

/// The browser's notice (see the module header).
pub struct BrowserNotice {
    name: String,
    button_name: String,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    font_size: f64,
    fill: Color,
    text_color: Color,
    showing: Option<Notice>,
    /// The notice's text as it fits beside the button, set at each layout.
    shown_text: String,
    alpha: Rc<Cell<f64>>,
    /// The UI clock instant a wake is already asked for, so layouts don't pile up timers.
    wake_at: Option<Instant>,
    state: Rc<RefCell<BrowserState>>,
    /// The pointer is over the notice.
    hovered: Rc<Cell<bool>>,
    /// The notice's button has the keyboard.
    button_focused: Rc<Cell<bool>>,
    /// The UI clock instant the notice was last held (hovered or focused); its hold counts from
    /// then when that is later than when it was shown.
    held_at: Option<Instant>,
}

impl BrowserNotice {
    /// The notice of the browser `state`'s history, drawn in its look with `font`.
    pub fn new(state: &Rc<RefCell<BrowserState>>, font: Arc<Font>) -> Self {
        let look = Rc::clone(&state.borrow().config().look);
        let (style, names) = (look.style(), look.names().clone());
        Self {
            name: names.notice,
            button_name: names.notice_button,
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: style.notice_font_size,
            fill: style.text_color,
            text_color: style.background,
            showing: None,
            shown_text: String::new(),
            alpha: Rc::new(Cell::new(0.0)),
            wake_at: None,
            state: Rc::clone(state),
            hovered: Rc::new(Cell::new(false)),
            button_focused: Rc::new(Cell::new(false)),
            held_at: None,
        }
    }

    /// The button for the notice now showing, if it offers one.
    fn button_for(&self, action: NoticeAction) -> NoticeButton {
        let state = Rc::clone(&self.state);
        let press: Rc<dyn Fn()> = Rc::new(move || {
            match action {
                NoticeAction::Undo => BrowserState::undo(&state),
                NoticeAction::Redo => BrowserState::redo(&state),
            };
        });
        NoticeButton::new(
            action_label(action),
            self.button_name.clone(),
            press,
            (Arc::clone(&self.font), self.font_size),
            Rc::clone(&self.alpha),
            self.text_color,
            Rc::clone(&self.button_focused),
        )
    }

    /// Follows the history's latest notice: a new one replaces the button; the strength follows
    /// the clock, asking for the next frame the fade needs.
    fn follow(&mut self) {
        let latest = self.state.borrow().history().notice();
        if latest.as_ref().map(|n| n.id) != self.showing.as_ref().map(|n| n.id) {
            self.children.clear();
            match latest.as_ref().and_then(|n| n.action) {
                Some(action) => self.children.push(Box::new(self.button_for(action))),
                // No button is left to hold the keyboard.
                None => self.button_focused.set(false),
            }
            self.showing = latest;
        }
        let Some(notice) = &self.showing else {
            self.alpha.set(0.0);
            return;
        };
        let now = agg_gui::clock::now();
        if self.hovered.get() || self.button_focused.get() {
            self.held_at = Some(now);
            self.alpha.set(1.0);
            return;
        }
        let start = match self.held_at {
            Some(held) if held > notice.shown_at => held,
            _ => notice.shown_at,
        };
        // Whole milliseconds, as the hold and fade are written; `as` is exact far beyond any
        // notice's age.
        let age = agg_gui::clock::since(start).as_millis() as u64;
        self.alpha.set(notice_alpha(age));
        if age >= NOTICE_HOLD_MS + NOTICE_FADE_MS {
            return;
        }
        let next = if age < NOTICE_HOLD_MS {
            start + Duration::from_millis(NOTICE_HOLD_MS)
        } else {
            now + FADE_FRAME
        };
        if self
            .wake_at
            .is_none_or(|at| at <= now || (age < NOTICE_HOLD_MS && at != next))
        {
            self.wake_at = Some(next);
            agg_gui::ui_thread::run_on_idle_after(next.saturating_duration_since(now), || {
                agg_gui::animation::request_layout();
            });
        }
    }

    fn text(&self) -> &str {
        self.showing.as_ref().map_or("", |n| n.text.as_str())
    }
}

impl Widget for BrowserNotice {
    fn type_name(&self) -> &'static str {
        "BrowserNotice"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
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
    fn is_visible(&self) -> bool {
        self.alpha.get() > 0.0
    }
    fn layout(&mut self, available: Size) -> Size {
        self.follow();
        if !self.is_visible() {
            return Size::new(0.0, 0.0);
        }
        let button = self
            .children
            .first_mut()
            .map(|button| (button.layout(available), button));
        // The text gets the room the button and the padding leave, so the button always shows.
        let button_room = button
            .as_ref()
            .map_or(0.0, |(size, _)| size.width + PADDING);
        let text_room = available.width - button_room - 2.0 * PADDING;
        let full = self.showing.as_ref().map_or("", |n| n.text.as_str());
        self.shown_text = fit_text(&self.font, full, self.font_size, text_room);
        let text = measure_text_metrics(&self.font, &self.shown_text, self.font_size);
        let height = (text.ascent - text.descent) + 2.0 * PADDING;
        let mut width = text.width + 2.0 * PADDING;
        if let Some((size, button)) = button {
            button.set_bounds(Rect::new(
                width,
                (height - size.height) / 2.0,
                size.width,
                size.height,
            ));
            width += size.width + PADDING;
        }
        Size::new(width.min(available.width), height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let alpha = self.alpha.get();
        let (w, h) = (self.bounds.width, self.bounds.height);
        ctx.set_fill_color(faded(self.fill, alpha));
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.fill();
        let m = measure_text_metrics(&self.font, &self.shown_text, self.font_size);
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(faded(self.text_color, alpha));
        ctx.fill_text(
            &self.shown_text,
            PADDING,
            (h - (m.ascent - m.descent)) / 2.0,
        );
    }
    /// A press on the notice's own box stays there rather than reaching the list under it. The
    /// pointer arriving holds the notice; its leaving lets the hold start again.
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { .. } | Event::MouseUp { .. } => EventResult::Consumed,
            Event::MouseEnter | Event::MouseLeave => {
                self.hovered.set(matches!(event, Event::MouseEnter));
                agg_gui::animation::request_layout();
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
    /// A line cut short shows in full on hover.
    fn tooltip_text(&self) -> Option<&str> {
        (self.shown_text != self.text()).then(|| self.text())
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("text", self.text().to_string()),
            ("shown_text", self.shown_text.clone()),
            ("alpha", self.alpha.get().to_string()),
        ]
    }
}

/// A notice's button: its word in an outlined box in the notice's text colour, pressed by a
/// click, Enter or Space.
pub struct NoticeButton {
    name: String,
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    label: String,
    on_press: Rc<dyn Fn()>,
    font: Arc<Font>,
    font_size: f64,
    alpha: Rc<Cell<f64>>,
    color: Color,
    pressed: bool,
    hovered: bool,
    /// Shared with the notice, which holds while the button has the keyboard.
    focused: Rc<Cell<bool>>,
}

impl NoticeButton {
    /// The button `name` reading `label` in `font` at its size, running `on_press` when
    /// pressed, drawn in `color` at `alpha` of its strength; `focused` follows whether it has the
    /// keyboard.
    pub fn new(
        label: String,
        name: String,
        on_press: Rc<dyn Fn()>,
        font: (Arc<Font>, f64),
        alpha: Rc<Cell<f64>>,
        color: Color,
        focused: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            name,
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
            label,
            on_press,
            font: font.0,
            font_size: font.1,
            alpha,
            color,
            pressed: false,
            hovered: false,
            focused,
        }
    }

    fn press(&self) {
        (self.on_press)();
        agg_gui::animation::request_layout();
    }

    fn contains(&self, p: Point) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x <= self.bounds.width && p.y <= self.bounds.height
    }
}

impl Widget for NoticeButton {
    fn type_name(&self) -> &'static str {
        "NoticeButton"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
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
    fn is_focusable(&self) -> bool {
        true
    }
    fn layout(&mut self, _available: Size) -> Size {
        let m = measure_text_metrics(&self.font, &self.label, self.font_size);
        Size::new(m.width + 16.0, (m.ascent - m.descent) + 8.0)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let alpha = self.alpha.get();
        let (w, h) = (self.bounds.width, self.bounds.height);
        if self.hovered {
            ctx.set_fill_color(faded(self.color, alpha * 0.2));
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
            ctx.fill();
        }
        ctx.set_stroke_color(faded(self.color, alpha));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.5, 0.5, w - 1.0, h - 1.0, 4.0);
        ctx.stroke();
        let m = measure_text_metrics(&self.font, &self.label, self.font_size);
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(faded(self.color, alpha));
        ctx.fill_text(&self.label, 8.0, (h - (m.ascent - m.descent)) / 2.0);
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let hovered = self.contains(*pos);
                if hovered != self.hovered {
                    self.hovered = hovered;
                    agg_gui::animation::request_draw();
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
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let clicked = self.pressed && self.contains(*pos);
                self.pressed = false;
                if clicked {
                    self.press();
                }
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::Enter | Key::Char(' '),
                ..
            } => {
                self.press();
                EventResult::Consumed
            }
            Event::FocusGained | Event::FocusLost => {
                self.focused.set(matches!(event, Event::FocusGained));
                agg_gui::animation::request_layout();
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("label", self.label.clone())]
    }
}
