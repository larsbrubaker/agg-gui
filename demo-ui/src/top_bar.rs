//! Top-bar widgets: theme toggle and backend toggle button.
//!
//! All text in this module is rendered through `Label` children with
//! `buffered = true` (the default), so glyph rasterization is cached to an
//! offscreen framebuffer and only repeated when the text or color changes.
//!
//! Exports:
//! - `build_top_bar_inner` — builds the FlexRow that fills the `TopMenuBar`

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult,
    FlexRow, Font, Rect, Size, SizedBox, Widget,
    ThemePreference, Visuals, set_visuals,
};
use agg_gui::widgets::label::Label;
use agg_gui::widget::paint_subtree;

/// Detect OS colour scheme and return the matching `ThemePreference`.
pub fn detect_system_theme() -> ThemePreference {
    match dark_light::detect() {
        dark_light::Mode::Light | dark_light::Mode::Default => ThemePreference::Light,
        dark_light::Mode::Dark => ThemePreference::Dark,
    }
}

/// Apply visuals matching the current OS color scheme.
fn apply_system_visuals() {
    match detect_system_theme() {
        ThemePreference::Light  => set_visuals(Visuals::light()),
        ThemePreference::Dark   => set_visuals(Visuals::dark()),
        ThemePreference::System => {} // won't happen
    }
}

// ── Theme toggle widget ────────────────────────────────────────────────────────

/// Three-button toggle: Light / Dark / System.
/// Writes the chosen `Visuals` via `set_visuals()` when clicked.
/// Text is rendered through backbuffered Label children.
struct ThemeToggle {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>, // always empty — labels are stored separately
    pref:     Rc<Cell<ThemePreference>>,
    hovered:  Option<usize>,
    /// One Label per segment. Positioned and painted manually.
    labels:   Vec<Label>,
}

impl ThemeToggle {
    const BTN_W: f64 = 68.0;
    const BTN_H: f64 = 24.0;
    // Font Awesome 4 icon prefixes: sun-o, moon-o, desktop.
    const LABELS: &'static [&'static str] = &[
        "\u{F185} Light",
        "\u{F186} Dark",
        "\u{F108} System",
    ];
    const PREFS: [ThemePreference; 3] = [
        ThemePreference::Light, ThemePreference::Dark, ThemePreference::System,
    ];

    fn new(font: Arc<Font>, pref: Rc<Cell<ThemePreference>>) -> Self {
        let labels = Self::LABELS.iter().map(|text| {
            Label::new(*text, Arc::clone(&font))
                .with_font_size(11.0)
        }).collect();
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            pref,
            hovered: None,
            labels,
        }
    }

    fn group_x(&self) -> f64 { 8.0 }

    fn btn_rect(&self, idx: usize) -> Rect {
        let gx = self.group_x();
        let gy = (self.bounds.height - Self::BTN_H) * 0.5;
        Rect::new(gx + idx as f64 * Self::BTN_W, gy, Self::BTN_W, Self::BTN_H)
    }

    fn hit_idx(&self, pos: agg_gui::Point) -> Option<usize> {
        for i in 0..3 {
            let r = self.btn_rect(i);
            if pos.x >= r.x && pos.x <= r.x + r.width
                && pos.y >= r.y && pos.y <= r.y + r.height
            { return Some(i); }
        }
        None
    }
}

impl Widget for ThemeToggle {
    fn type_name(&self) -> &'static str { "ThemeToggle" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let natural_w = (3.0 * Self::BTN_W + 16.0).min(available.width);
        self.bounds = Rect::new(0.0, 0.0, natural_w, available.height);
        // Layout each label to fill its button rect (for centered text).
        for i in 0..3 {
            let r = self.btn_rect(i);
            let s = self.labels[i].layout(Size::new(r.width, r.height));
            self.labels[i].set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        }
        Size::new(natural_w, available.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let current = self.pref.get();

        for (i, pref) in Self::PREFS.iter().enumerate() {
            let r = self.btn_rect(i);
            let active  = std::mem::discriminant(&current) == std::mem::discriminant(pref);
            let hovered = self.hovered == Some(i);

            let bg = if active { v.accent }
                     else if hovered { v.widget_bg_hovered }
                     else { v.widget_bg };
            ctx.set_fill_color(bg);
            ctx.begin_path();
            let radius = if i == 0 || i == 2 { 4.0 } else { 0.0 };
            ctx.rounded_rect(r.x, r.y, r.width, r.height, radius);
            ctx.fill();

            // Draw separator between buttons.
            if i < 2 {
                ctx.set_fill_color(v.widget_stroke);
                ctx.begin_path();
                ctx.rect(r.x + r.width - 1.0, r.y, 1.0, r.height);
                ctx.fill();
            }

            // Active segment = accent blue → white for max contrast.
            let text_color = if active { Color::white() } else { v.text_color };
            self.labels[i].set_color(text_color);

            // Reposition label centered within button rect.
            let lw = self.labels[i].bounds().width;
            let lh = self.labels[i].bounds().height;
            let lx = r.x + (r.width - lw) * 0.5;
            let ly = r.y + (r.height - lh) * 0.5;
            self.labels[i].set_bounds(Rect::new(lx, ly, lw, lh));

            // Paint the label (handles backbuffer caching internally).
            ctx.save();
            ctx.translate(lx, ly);
            paint_subtree(&mut self.labels[i], ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_idx(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { button: agg_gui::MouseButton::Left, pos, .. } => {
                if let Some(idx) = self.hit_idx(*pos) {
                    let pref = Self::PREFS[idx];
                    self.pref.set(pref);
                    match pref {
                        ThemePreference::Light  => set_visuals(Visuals::light()),
                        ThemePreference::Dark   => set_visuals(Visuals::dark()),
                        ThemePreference::System => apply_system_visuals(),
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── Backend toggle button ─────────────────────────────────────────────────────

/// "💻 Backend" button — toggles the left-side backend panel.
/// Text rendered through a backbuffered Label child.
struct BackendButton {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>, // always empty — label stored separately
    show:     Rc<Cell<bool>>,
    hovered:  bool,
    label:    Label,
}

impl BackendButton {
    const W: f64 = 112.0;
    const H: f64 = 24.0;

    fn new(font: Arc<Font>, show: Rc<Cell<bool>>) -> Self {
        // FA4 "laptop" icon prefix.
        let label = Label::new("\u{F109} Backend", Arc::clone(&font))
            .with_font_size(12.0);
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            show,
            hovered: false,
            label,
        }
    }

    fn btn_rect(&self) -> Rect {
        let gy = (self.bounds.height - Self::H) * 0.5;
        Rect::new(4.0, gy, Self::W, Self::H)
    }
}

impl Widget for BackendButton {
    fn type_name(&self) -> &'static str { "BackendButton" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = Self::W + 8.0;
        self.bounds = Rect::new(0.0, 0.0, w, available.height);
        let s = self.label.layout(Size::new(Self::W, Self::H));
        self.label.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        Size::new(w, available.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let r = self.btn_rect();
        let active = self.show.get();

        let bg = if active { v.accent }
                 else if self.hovered { v.widget_bg_hovered }
                 else { v.widget_bg };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(r.x, r.y, r.width, r.height, 4.0);
        ctx.fill();

        // Active state = accent blue → white text for contrast.
        let text_color = if active { Color::white() } else { v.text_color };
        self.label.set_color(text_color);

        // Center label within button rect.
        let lw = self.label.bounds().width;
        let lh = self.label.bounds().height;
        let lx = r.x + (r.width - lw) * 0.5;
        let ly = r.y + (r.height - lh) * 0.5;
        self.label.set_bounds(Rect::new(lx, ly, lw, lh));

        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let r = self.btn_rect();
        let in_btn = |p: agg_gui::Point| {
            p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
        };
        match event {
            Event::MouseMove { pos } => { self.hovered = in_btn(*pos); EventResult::Ignored }
            Event::MouseDown { button: agg_gui::MouseButton::Left, pos, .. } => {
                if in_btn(*pos) {
                    self.show.set(!self.show.get());
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Build the FlexRow child for `TopMenuBar`.
///
/// Layout: [Backend button] [spacer] [flex(1.0)] [ThemeToggle]
pub fn build_top_bar_inner(
    font:         Arc<Font>,
    show_backend: Rc<Cell<bool>>,
    theme_pref:   Rc<Cell<ThemePreference>>,
) -> Box<dyn Widget> {
    Box::new(FlexRow::new()
        .with_gap(0.0)
        .add(Box::new(BackendButton::new(Arc::clone(&font), show_backend)))
        .add(Box::new(SizedBox::new().with_width(8.0)))
        .add_flex(Box::new(SizedBox::new()), 1.0)
        .add(Box::new(ThemeToggle::new(font, theme_pref))))
}
