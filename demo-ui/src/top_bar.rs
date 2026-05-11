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

use agg_gui::widget::paint_subtree;
use agg_gui::widgets::label::Label;
use agg_gui::{
    set_visuals, AccentColor, Button, Color, DrawCtx, Event, EventResult, FlexRow, Font, Hyperlink,
    Rect, Size, SizedBox, ThemePreference, VAnchor, Visuals, Widget,
};

/// Detect OS colour scheme and return the matching `ThemePreference`.
pub fn detect_system_theme() -> ThemePreference {
    match dark_light::detect() {
        dark_light::Mode::Light | dark_light::Mode::Default => ThemePreference::Light,
        dark_light::Mode::Dark => ThemePreference::Dark,
    }
}

/// Apply visuals matching the selected theme and accent swatch.
pub fn apply_theme_visuals(pref: ThemePreference, accent: AccentColor) {
    let base = match pref {
        ThemePreference::Light => Visuals::light(),
        ThemePreference::Dark => Visuals::dark(),
        ThemePreference::System => match detect_system_theme() {
            ThemePreference::Light => Visuals::light(),
            _ => Visuals::dark(),
        },
    };
    set_visuals(base.with_accent_color(accent));
}

// ── Theme toggle widget ────────────────────────────────────────────────────────

/// Three-segment selector — Light / Dark / System — built out of three
/// real `Button` children sharing a `Rc<Cell<ThemePreference>>`.  Each
/// segment uses [`Button::with_subtle`] + [`Button::with_active_fn`] so
/// the inactive segments paint in muted theme colours and the selected
/// segment flips to the accent surface.  Glyphs are cached via the Label
/// child every Button already wraps — no manual `paint_subtree` call,
/// no separate label vector.
///
/// The widget keeps a thin wrapper struct so the inspector still shows
/// "ThemeToggle" as a meaningful semantic node above its three buttons.
struct ThemeToggle {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl ThemeToggle {
    const BTN_W: f64 = 68.0;
    const BTN_H: f64 = 24.0;
    const GAP_X: f64 = 8.0;
    // Gap between adjacent buttons — baked into position math, not a widget margin property.
    const INNER_GAP: f64 = 2.0;

    fn new(
        font: Arc<Font>,
        pref: Rc<Cell<ThemePreference>>,
        accent: Rc<Cell<AccentColor>>,
    ) -> Self {
        let segments: [(&'static str, ThemePreference); 3] = [
            ("\u{F185} Light", ThemePreference::Light),
            ("\u{F186} Dark", ThemePreference::Dark),
            ("\u{F108} System", ThemePreference::System),
        ];
        let children: Vec<Box<dyn Widget>> = segments
            .iter()
            .map(|(label, this_pref)| {
                let pref_active = Rc::clone(&pref);
                let pref_click = Rc::clone(&pref);
                let accent_for_click = Rc::clone(&accent);
                let this = *this_pref;
                let btn = Button::new(*label, Arc::clone(&font))
                    .with_font_size(11.0)
                    .with_subtle()
                    .with_outlined()
                    .with_active_fn(move || {
                        std::mem::discriminant(&pref_active.get()) == std::mem::discriminant(&this)
                    })
                    .on_click(move || {
                        pref_click.set(this);
                        apply_theme_visuals(this, accent_for_click.get());
                        agg_gui::animation::request_draw();
                    });
                Box::new(btn) as Box<dyn Widget>
            })
            .collect();
        Self {
            bounds: Rect::default(),
            children,
        }
    }
}

impl Widget for ThemeToggle {
    fn type_name(&self) -> &'static str {
        "ThemeToggle"
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

    fn layout(&mut self, available: Size) -> Size {
        let step = Self::BTN_W + Self::INNER_GAP;
        let natural_w =
            (3.0 * Self::BTN_W + Self::INNER_GAP * 2.0 + Self::GAP_X * 2.0).min(available.width);
        self.bounds = Rect::new(0.0, 0.0, natural_w, available.height);
        let gy = ((available.height - Self::BTN_H) * 0.5).max(0.0);
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(Self::BTN_W, Self::BTN_H));
            child.set_bounds(Rect::new(
                Self::GAP_X + i as f64 * step,
                gy,
                Self::BTN_W,
                Self::BTN_H,
            ));
        }
        Size::new(natural_w, available.height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // All paint goes through the Button children via the framework's
        // tree walk — `paint_subtree` recurses into `self.children` after
        // this returns.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        // Bubbling: events route to the hit-tested Button child by the
        // standard tree-dispatch path; the wrapper itself has no
        // behaviour of its own.
        EventResult::Ignored
    }
}

// ── Backend toggle button ─────────────────────────────────────────────────────

/// "Backend" button that toggles the left-side backend panel.  Wraps a
/// real `Button` child sized to the top-bar slot — `with_subtle()` +
/// `with_active_fn()` make it light up in the accent surface while the
/// panel is open and stay muted otherwise.
struct BackendButton {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl BackendButton {
    const W: f64 = 112.0;
    const H: f64 = 24.0;

    fn new(font: Arc<Font>, show: Rc<Cell<bool>>) -> Self {
        let show_active = Rc::clone(&show);
        let show_click = show;
        let btn = Button::new("\u{F109} Backend", font)
            .with_font_size(12.0)
            .with_subtle()
            .with_outlined()
            .with_active_fn(move || show_active.get())
            .on_click(move || {
                show_click.set(!show_click.get());
                agg_gui::animation::request_draw();
            });
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(btn)],
        }
    }
}

impl Widget for BackendButton {
    fn type_name(&self) -> &'static str {
        "BackendButton"
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

    fn layout(&mut self, available: Size) -> Size {
        let w = Self::W + 8.0;
        self.bounds = Rect::new(0.0, 0.0, w, available.height);
        let gy = ((available.height - Self::H) * 0.5).max(0.0);
        let child = &mut self.children[0];
        child.layout(Size::new(Self::W, Self::H));
        child.set_bounds(Rect::new(4.0, gy, Self::W, Self::H));
        Size::new(w, available.height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Button child paints itself via the framework's tree walk.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Mobile-only "Demos" hamburger that toggles the bottom-sheet menu.
/// Hidden above the mobile breakpoint and rendered as a real `Button`
/// child — the only thing this wrapper adds is the responsive
/// breakpoint check and the breakpoint-driven `is_visible` gate that
/// hides the button on desktop layouts.
struct MenuButton {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    visible: bool,
}

impl MenuButton {
    const W: f64 = 92.0;
    const H: f64 = 24.0;
    const MOBILE_BREAKPOINT: f64 = 720.0;

    fn new(font: Arc<Font>, open: Rc<Cell<bool>>) -> Self {
        let open_active = Rc::clone(&open);
        let open_click = open;
        let btn = Button::new("\u{F0C9} Demos", font)
            .with_font_size(12.0)
            .with_subtle()
            .with_outlined()
            .with_active_fn(move || open_active.get())
            .on_click(move || {
                open_click.set(!open_click.get());
                agg_gui::animation::request_draw();
            });
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(btn)],
            visible: false,
        }
    }
}

impl Widget for MenuButton {
    fn type_name(&self) -> &'static str {
        "MenuButton"
    }
    fn is_visible(&self) -> bool {
        self.visible
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
    fn layout(&mut self, available: Size) -> Size {
        self.visible = available.width > 0.0 && available.width < Self::MOBILE_BREAKPOINT;
        if !self.visible {
            self.bounds = Rect::new(0.0, 0.0, 0.0, available.height);
            return Size::new(0.0, available.height);
        }
        let w = Self::W + 8.0;
        self.bounds = Rect::new(0.0, 0.0, w, available.height);
        let gy = ((available.height - Self::H) * 0.5).max(0.0);
        let child = &mut self.children[0];
        child.layout(Size::new(Self::W, Self::H));
        child.set_bounds(Rect::new(4.0, gy, Self::W, Self::H));
        Size::new(w, available.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Button child paints itself via the framework's tree walk.
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Accent colour dropdown ────────────────────────────────────────────────────

struct AccentDropdown {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    pref: Rc<Cell<ThemePreference>>,
    accent: Rc<Cell<AccentColor>>,
    open: bool,
    hovered_button: bool,
    hovered_swatch: Option<usize>,
    label: Label,
}

impl AccentDropdown {
    const W: f64 = 86.0;
    const H: f64 = 24.0;
    const SWATCH: f64 = 18.0;
    const GAP: f64 = 6.0;
    const POPUP_PAD: f64 = 8.0;

    fn new(
        font: Arc<Font>,
        pref: Rc<Cell<ThemePreference>>,
        accent: Rc<Cell<AccentColor>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            pref,
            accent,
            open: false,
            hovered_button: false,
            hovered_swatch: None,
            label: Label::new("Color", font).with_font_size(11.0),
        }
    }

    fn btn_rect(&self) -> Rect {
        Rect::new(0.0, (self.bounds.height - Self::H) * 0.5, Self::W, Self::H)
    }

    fn popup_rect(&self) -> Rect {
        let w = Self::POPUP_PAD * 2.0
            + AccentColor::ALL.len() as f64 * Self::SWATCH
            + (AccentColor::ALL.len() - 1) as f64 * Self::GAP;
        let h = Self::POPUP_PAD * 2.0 + Self::SWATCH;
        Rect::new(Self::W - w, -h - 4.0, w, h)
    }

    fn contains(r: Rect, p: agg_gui::Point) -> bool {
        p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
    }

    fn swatch_rect(&self, idx: usize) -> Rect {
        let p = self.popup_rect();
        Rect::new(
            p.x + Self::POPUP_PAD + idx as f64 * (Self::SWATCH + Self::GAP),
            p.y + Self::POPUP_PAD,
            Self::SWATCH,
            Self::SWATCH,
        )
    }

    fn hit_swatch(&self, pos: agg_gui::Point) -> Option<usize> {
        AccentColor::ALL
            .iter()
            .enumerate()
            .find_map(|(i, _)| Self::contains(self.swatch_rect(i), pos).then_some(i))
    }
}

impl Widget for AccentDropdown {
    fn type_name(&self) -> &'static str {
        "AccentDropdown"
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
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, Self::W, available.height);
        let s = self.label.layout(Size::new(Self::W, Self::H));
        self.label
            .set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        Size::new(Self::W, available.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let r = self.btn_rect();
        let fill = if self.open {
            v.accent
        } else if self.hovered_button {
            v.widget_bg_hovered
        } else {
            v.widget_bg
        };
        ctx.set_fill_color(fill);
        ctx.begin_path();
        ctx.rounded_rect(r.x, r.y, r.width, r.height, 4.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(r.x + 0.5, r.y + 0.5, r.width - 1.0, r.height - 1.0, 4.0);
        ctx.stroke();

        let swatch = Rect::new(r.x + 7.0, r.y + 5.0, 14.0, 14.0);
        ctx.set_fill_color(self.accent.get().color());
        ctx.begin_path();
        ctx.rounded_rect(swatch.x, swatch.y, swatch.width, swatch.height, 3.0);
        ctx.fill();

        self.label.set_color(if self.open {
            Color::white()
        } else {
            v.text_color
        });
        let lx = r.x + 28.0;
        let ly = r.y + (r.height - self.label.bounds().height) * 0.5;
        self.label.set_bounds(Rect::new(
            lx,
            ly,
            self.label.bounds().width,
            self.label.bounds().height,
        ));
        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }
    fn hit_test_global_overlay(&self, local_pos: agg_gui::Point) -> bool {
        self.open && Self::contains(self.popup_rect(), local_pos)
    }
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.open {
            return;
        }
        let v = ctx.visuals();
        let p = self.popup_rect();
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(p.x, p.y, p.width, p.height, 6.0);
        ctx.fill();
        ctx.set_stroke_color(v.window_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(p.x + 0.5, p.y + 0.5, p.width - 1.0, p.height - 1.0, 6.0);
        ctx.stroke();

        for (i, accent) in AccentColor::ALL.iter().enumerate() {
            let r = self.swatch_rect(i);
            ctx.set_fill_color(accent.color());
            ctx.begin_path();
            ctx.rounded_rect(r.x, r.y, r.width, r.height, 4.0);
            ctx.fill();
            let selected = self.accent.get() == *accent;
            let hovered = self.hovered_swatch == Some(i);
            if selected || hovered {
                ctx.set_stroke_color(if selected {
                    v.text_color
                } else {
                    v.widget_stroke
                });
                ctx.set_line_width(if selected { 2.0 } else { 1.0 });
                ctx.begin_path();
                ctx.rounded_rect(r.x - 2.0, r.y - 2.0, r.width + 4.0, r.height + 4.0, 5.0);
                ctx.stroke();
            }
        }
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was_button = self.hovered_button;
                let was_swatch = self.hovered_swatch;
                self.hovered_button = Self::contains(self.btn_rect(), *pos);
                self.hovered_swatch = if self.open {
                    self.hit_swatch(*pos)
                } else {
                    None
                };
                if was_button != self.hovered_button || was_swatch != self.hovered_swatch {
                    agg_gui::animation::request_draw();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: agg_gui::MouseButton::Left,
                pos,
                ..
            } => {
                if self.open {
                    if let Some(idx) = self.hit_swatch(*pos) {
                        self.accent.set(AccentColor::ALL[idx]);
                        apply_theme_visuals(self.pref.get(), self.accent.get());
                        self.open = false;
                        self.hovered_swatch = None;
                        agg_gui::animation::request_draw();
                        return EventResult::Consumed;
                    }
                }
                if Self::contains(self.btn_rect(), *pos) {
                    self.open = !self.open;
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// URL of the project on GitHub — opened by the "View on GitHub" link.
/// Single source of truth so the README badge and the in-app link stay
/// in sync.
const GITHUB_URL: &str = "https://github.com/larsbrubaker/agg-gui";

/// Build the FlexRow child for `TopMenuBar`.
///
/// Layout: `[Backend button] [Demos button on mobile] [spacer] [flex(1.0)] [GitHub link] [gap] [ThemeToggle]`.
/// The GitHub `Hyperlink` opens the project page in a new tab on both
/// native (via the `webbrowser` crate) and WASM (via
/// `window.open(_, "_blank")`).
pub fn build_top_bar_inner(
    font: Arc<Font>,
    show_backend: Rc<Cell<bool>>,
    mobile_menu_open: Rc<Cell<bool>>,
    theme_pref: Rc<Cell<ThemePreference>>,
    accent_color: Rc<Cell<AccentColor>>,
) -> Box<dyn Widget> {
    let github_link = Hyperlink::new("View on GitHub", Arc::clone(&font))
        .with_font_size(13.0)
        // VAnchor::CENTER tells the wrapping `SizedBox` to vertically
        // centre the link inside its 28-px-tall slot.  Default
        // `VAnchor::FIT` would bottom-anchor it (Y-up convention),
        // leaving it sitting low in the row instead of lined up with
        // the theme toggle.
        .with_v_anchor(VAnchor::CENTER)
        .on_click(|| crate::url::open_url(GITHUB_URL));

    // Sized box wraps the link so the FlexRow doesn't try to flex it
    // — Hyperlink reports `available.width` as its natural width and
    // would otherwise stretch across the bar.  `VAnchor::CENTER` on the
    // box itself pulls it to the vertical centre of the top bar;
    // without it FlexRow bottom-anchors the box (Y-up FIT default) and
    // the link would sit low next to the ThemeToggle.
    let github_widget: Box<dyn Widget> = Box::new(
        SizedBox::new()
            .with_width(110.0)
            .with_height(28.0)
            .with_v_anchor(VAnchor::CENTER)
            .with_child(Box::new(github_link)),
    );

    Box::new(
        FlexRow::new()
            .with_gap(0.0)
            .add(Box::new(BackendButton::new(
                Arc::clone(&font),
                show_backend,
            )))
            .add(Box::new(MenuButton::new(
                Arc::clone(&font),
                mobile_menu_open,
            )))
            .add(Box::new(SizedBox::new().with_width(8.0)))
            .add_flex(Box::new(SizedBox::new()), 1.0)
            .add(github_widget)
            .add(Box::new(SizedBox::new().with_width(12.0)))
            .add(Box::new(ThemeToggle::new(
                Arc::clone(&font),
                Rc::clone(&theme_pref),
                Rc::clone(&accent_color),
            )))
            .add(Box::new(SizedBox::new().with_width(6.0)))
            .add(Box::new(AccentDropdown::new(
                font,
                theme_pref,
                accent_color,
            )))
            // 3-px breathing room between the Color button and the window's
            // right edge / native window-control buttons.  Without this the
            // button's outer border touches the chrome and looks pinched.
            .add(Box::new(SizedBox::new().with_width(3.0))),
    )
}
