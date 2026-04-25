//! `Button` — a clickable button with hover, pressed, and focus states.
//!
//! # Composition
//!
//! Button is fully compositional: it always has exactly one child widget, a
//! [`Label`], which is responsible for rendering the button's text.  The
//! [`paint_subtree`] machinery handles the Label automatically after
//! [`Button::paint`] draws the background.
//!
//! ```text
//! Button (background + focus ring)
//!   └── Label (text, tight bounds, centred within button)
//! ```
//!
//! `Label::layout` returns tight text bounds.  `Button::layout` centres the
//! label within the button area.  Because [`Label::hit_test`] returns `false`,
//! the Label is invisible to the hit-test and event-routing system; the Button
//! retains full ownership of focus and click events.

use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::{Label, LabelAlign};

/// A theme for [`Button`] visual states.
pub struct ButtonTheme {
    pub background: Color,
    pub background_hovered: Color,
    pub background_pressed: Color,
    pub label_color: Color,
    pub border_radius: f64,
    pub focus_ring_color: Color,
    pub focus_ring_width: f64,
}

impl Default for ButtonTheme {
    fn default() -> Self {
        Self {
            background: Color::rgb(0.22, 0.45, 0.88),
            background_hovered: Color::rgb(0.30, 0.52, 0.92),
            background_pressed: Color::rgb(0.16, 0.36, 0.72),
            label_color: Color::white(),
            border_radius: 6.0,
            focus_ring_color: Color::rgba(0.22, 0.45, 0.88, 0.55),
            focus_ring_width: 2.5,
        }
    }
}

/// A clickable button.
///
/// Build with [`Button::new`] and optionally chain builder methods.
pub struct Button {
    bounds: Rect,
    /// Always exactly one child: the `Label` for the button's text.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    /// Source of truth for the label text, kept so `build_label` can rebuild.
    label_text: String,
    font: Arc<Font>,
    font_size: f64,
    pub theme: ButtonTheme,
    on_click: Option<Box<dyn FnMut()>>,
    /// Optional gate: when `Some`, the button is enabled only while the
    /// closure returns `true`.  Queried each paint / event so the caller
    /// can base it on live state (e.g. "only enable Relaunch when the
    /// selected MSAA differs from the running one") without rebuilding
    /// the widget tree.  `None` = always enabled.
    enabled_fn: Option<Rc<dyn Fn() -> bool>>,

    hovered: bool,
    pressed: bool,
    focused: bool,
}

impl Button {
    /// Create a button with the given label.
    pub fn new(label: impl Into<String>, font: Arc<Font>) -> Self {
        let label_text: String = label.into();
        let font_size = 14.0;
        let theme = ButtonTheme::default();
        let child = Self::build_label(&label_text, &font, font_size, &theme);
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            label_text,
            font,
            font_size,
            theme,
            on_click: None,
            enabled_fn: None,
            hovered: false,
            pressed: false,
            focused: false,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self.children[0] = Self::build_label(&self.label_text, &self.font, size, &self.theme);
        self
    }

    pub fn with_theme(mut self, theme: ButtonTheme) -> Self {
        self.theme = theme;
        self.children[0] =
            Self::build_label(&self.label_text, &self.font, self.font_size, &self.theme);
        self
    }

    pub fn on_click(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(cb));
        self
    }

    /// Gate the button on a live predicate.  Returned-`false` frames paint
    /// the button in its disabled style and ignore mouse / keyboard input.
    pub fn with_enabled_fn(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.enabled_fn = Some(Rc::new(f));
        self
    }

    fn is_enabled(&self) -> bool {
        self.enabled_fn.as_ref().map(|f| f()).unwrap_or(true)
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
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    fn fire_click(&mut self) {
        if let Some(cb) = self.on_click.as_mut() {
            cb();
        }
    }

    fn disabled_colors(v: &crate::theme::Visuals) -> (Color, Color, Color) {
        let luma = v.bg_color.r * 0.299 + v.bg_color.g * 0.587 + v.bg_color.b * 0.114;
        if luma < 0.5 {
            (
                v.window_fill,
                Color::rgba(1.0, 1.0, 1.0, 0.22),
                v.text_dim.with_alpha(0.42),
            )
        } else {
            (v.track_bg, v.widget_stroke.with_alpha(0.45), v.text_dim)
        }
    }

    /// Construct a label child from the button's current state.
    ///
    /// Called from `new()`, `with_theme()`, and `with_font_size()` so the
    /// child always reflects the button's configuration.
    fn build_label(
        text: &str,
        font: &Arc<Font>,
        font_size: f64,
        theme: &ButtonTheme,
    ) -> Box<dyn Widget> {
        Box::new(
            Label::new(text, Arc::clone(font))
                .with_font_size(font_size)
                .with_color(theme.label_color)
                .with_align(LabelAlign::Center),
        )
    }
}

impl Widget for Button {
    fn type_name(&self) -> &'static str {
        "Button"
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

    fn is_focusable(&self) -> bool {
        self.is_enabled()
    }

    fn margin(&self) -> Insets {
        self.base.margin
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

    fn layout(&mut self, available: Size) -> Size {
        let height = (self.font_size * 2.4).max(28.0);
        // Measure the label first so we can report a "fit" width — label
        // width plus horizontal padding — instead of stretching to the whole
        // available width.  This makes Buttons share horizontal space
        // politely when placed inside a `FlexRow` next to other widgets.
        // Parents that want a full-width button should wrap in a `SizedBox`
        // with an explicit width, or set `HAnchor::FILL` — handled by the
        // flex layout before this method is called.
        let pad_h = self.font_size * 1.4;
        let label_size = self.children[0].layout(Size::new(available.width, height));
        let natural_w = (label_size.width + pad_h).max(48.0);
        let width = natural_w.min(available.width);
        let size = Size::new(width, height);
        let label_x = ((size.width - label_size.width) * 0.5).max(0.0);
        let label_y = ((size.height - label_size.height) * 0.5).max(0.0);
        self.children[0].set_bounds(Rect::new(
            label_x,
            label_y,
            label_size.width,
            label_size.height,
        ));
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.theme.border_radius;
        let enabled = self.is_enabled();

        // Focus ring (behind the button surface) — skipped when disabled
        // because the disabled button never actually holds focus.
        if enabled && self.focused {
            let ring = self.theme.focus_ring_width;
            ctx.set_stroke_color(self.theme.focus_ring_color);
            ctx.set_line_width(ring);
            ctx.begin_path();
            ctx.rounded_rect(-ring * 0.5, -ring * 0.5, w + ring, h + ring, r + ring * 0.5);
            ctx.stroke();
        }

        // Background — color depends on interaction state. Disabled buttons
        // use neutral widget colors instead of a washed-out accent, so they
        // don't look like secondary active actions.
        let base_bg = if self.pressed {
            self.theme.background_pressed
        } else if self.hovered {
            self.theme.background_hovered
        } else {
            self.theme.background
        };
        let v = ctx.visuals();
        let (disabled_bg, disabled_stroke, _) = Self::disabled_colors(&v);
        let bg = if enabled { base_bg } else { disabled_bg };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        if !enabled {
            ctx.set_stroke_color(disabled_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0), r);
            ctx.stroke();
        }

        // Text is NOT drawn here. `paint_subtree` recurses into the Label
        // child automatically after this method returns.
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if self.is_enabled() {
            return;
        }

        // The normal child Label was built for the enabled foreground color.
        // Cover it and repaint the label with the disabled text color.
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.theme.border_radius;
        let v = ctx.visuals();
        let (disabled_bg, disabled_stroke, disabled_text) = Self::disabled_colors(&v);

        ctx.set_fill_color(disabled_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        ctx.set_stroke_color(disabled_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0), r);
        ctx.stroke();

        let font =
            crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font));
        ctx.set_font(font);
        ctx.set_font_size(self.font_size * crate::font_settings::current_font_size_scale());
        ctx.set_fill_color(disabled_text);
        if let Some(m) = ctx.measure_text(&self.label_text) {
            let tx = ((w - m.width) * 0.5).max(0.0);
            let ty = m.centered_baseline_y(h).max(0.0);
            ctx.fill_text(&self.label_text, tx, ty);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.is_enabled() {
            // Clear any lingering hover / pressed state so the button
            // looks idle the instant it's disabled mid-interaction.
            self.hovered = false;
            self.pressed = false;
            return EventResult::Ignored;
        }
        match event {
            Event::MouseMove { pos } => {
                let was_hovered = self.hovered;
                let was_pressed = self.pressed;
                self.hovered = self.hit_test(*pos);
                if !self.hovered {
                    self.pressed = false;
                }
                if was_hovered != self.hovered || was_pressed != self.pressed {
                    crate::animation::request_tick();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                if !self.pressed {
                    crate::animation::request_tick();
                }
                self.pressed = true;
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed {
                    crate::animation::request_tick();
                }
                if was_pressed && self.hovered {
                    self.fire_click();
                    // Click handler almost always mutates app state that
                    // affects the next paint; request one so the handler's
                    // side-effects are visible.
                    crate::animation::request_tick();
                }
                EventResult::Consumed
            }
            Event::KeyDown { key, .. } => {
                use crate::event::Key;
                match key {
                    Key::Enter | Key::Char(' ') => {
                        self.fire_click();
                        crate::animation::request_tick();
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            Event::FocusGained => {
                self.focused = true;
                crate::animation::request_tick();
                EventResult::Ignored
            }
            Event::FocusLost => {
                self.focused = false;
                self.pressed = false;
                crate::animation::request_tick();
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("label", self.label_text.clone()),
            ("font_size", format!("{:.1}", self.font_size)),
        ]
    }
}
