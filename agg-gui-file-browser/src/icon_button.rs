//! `BrowserIconButton`: a borderless square button drawing one of the browser's icons
//! ([`BrowserIcon`]), as the look draws it ([`BrowserLook::icon`]) or else as its Font Awesome
//! glyph. It fills with the hover colour under the pointer (nothing otherwise) and runs its click
//! action on a release over it, even when the pointer left and came back while the button was
//! held. A disabled button ignores the pointer, so a click passes to its parent, drops a press in
//! progress, and draws its icon dimmed: the look dims its own image (it is asked with
//! `enabled = false`), and the glyph draws at a fifth of its alpha. Its tooltip lives in its
//! `WidgetBase`, so `Widget::set_tooltip_text` changes it.
//!
//! The look's image is drawn centred with its corner moved to a whole device pixel, so an icon
//! made for the device scale lands 1:1 and stays crisp. The image is kept between paints and
//! asked for again only when the button is enabled or disabled, the device scale changes or the
//! look's art does (`BrowserLook::icon_generation`).
//! A button may carry a hint (`with_hint`) the look shows while the pointer is over it and
//! clears when it leaves (`BrowserLook::show_hint`, an app's status bar), and its own hover fill
//! and glyph colour (`with_hover_fill`, `with_glyph_color`).
//! The breadcrumb's up button (`bread_crumb.rs`) and the navigation bar's buttons (`nav/`) are
//! ones.

use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::text::Font;
use agg_gui::{
    measure_text_metrics, Color, DrawCtx, Event, EventResult, MouseButton, Point, Rect, Size,
    Widget, WidgetBase,
};

use crate::look::{glyph_face, BrowserIcon, BrowserLook};

/// The share of the button's side its Font Awesome glyph fills.
const GLYPH_SHARE: f64 = 0.6;
/// The alpha a disabled glyph keeps.
const DISABLED_ALPHA: f64 = 0.2;

/// The look's image as last asked for: whether the button was enabled, the device scale, the
/// look's icon generation, and the answer.
type IconCache = (bool, f64, u64, Option<IconImage>);

/// A square button showing a browser icon ([module docs](self)).
pub struct BrowserIconButton {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    look: Rc<dyn BrowserLook>,
    which: BrowserIcon,
    /// The logical size the look's image is asked for at.
    icon_size: Size,
    icon: Option<IconCache>,
    font: Arc<Font>,
    side: f64,
    glyph_color: Color,
    hover_fill: Color,
    hovered: bool,
    pressed: bool,
    enabled: bool,
    name: Option<String>,
    on_click: Option<Box<dyn FnMut()>>,
    /// The hint the look shows while the pointer is over the enabled button (a status bar's
    /// "⌘Z"), `BrowserLook::show_hint`.
    hint: Option<String>,
}

impl BrowserIconButton {
    /// A `side` x `side` button drawing `which` as `look` gives it, in the look's text colour,
    /// filled with its hover colour under the pointer. The look's image is asked for 16 x 16
    /// unless [`with_icon_size`](Self::with_icon_size) says otherwise.
    pub fn new(look: Rc<dyn BrowserLook>, which: BrowserIcon, side: f64) -> Self {
        let style = look.style();
        let mut base = WidgetBase::new();
        base.min_size = Size::new(side, side);
        Self {
            bounds: Rect::default(),
            base,
            children: Vec::new(),
            look,
            which,
            icon_size: Size::new(16.0, 16.0),
            icon: None,
            font: glyph_face(),
            side,
            glyph_color: style.text_color,
            hover_fill: style.hover_fill,
            hovered: false,
            pressed: false,
            enabled: true,
            name: None,
            on_click: None,
            hint: None,
        }
    }

    /// Fills with `color` under the pointer in place of the look's hover colour.
    pub fn with_hover_fill(mut self, color: Color) -> Self {
        self.hover_fill = color;
        self
    }

    /// Draws its glyph in `color` in place of the look's text colour.
    pub fn with_glyph_color(mut self, color: Color) -> Self {
        self.glyph_color = color;
        self
    }

    /// The look shows `text` as a hint while the pointer is over the enabled button, and clears
    /// it when the pointer leaves (`BrowserLook::show_hint`: an app's status bar).
    pub fn with_hint(mut self, text: &str) -> Self {
        self.hint = Some(text.to_string());
        self
    }

    /// Sets the hover state, showing or clearing the hint as it changes.
    fn set_hovered(&mut self, hovered: bool) -> bool {
        if hovered == self.hovered {
            return false;
        }
        self.hovered = hovered;
        if let Some(hint) = &self.hint {
            self.look.show_hint(hovered.then_some(hint.as_str()));
        }
        agg_gui::animation::request_draw();
        true
    }

    /// Asks the look for its image `size` in logical units.
    pub fn with_icon_size(mut self, size: Size) -> Self {
        self.icon_size = size;
        self.icon = None;
        self
    }

    /// The widget name the button is found by.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The tooltip shown while the pointer rests on the button (kept in its `WidgetBase`, so
    /// `Widget::set_tooltip_text` changes it later).
    pub fn with_tooltip_text(mut self, text: &str) -> Self {
        self.base.tooltip = Some(text.to_string());
        self
    }

    /// Whether the button takes clicks; a disabled one draws dimmed.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Runs `action` on a click.
    pub fn on_click(mut self, action: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(action));
        self
    }

    /// Whether the button takes clicks.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Makes the button take clicks or not. Disabling it also drops a press in progress, so a
    /// release after it is enabled again does not click.
    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.set_hovered(false);
            self.pressed = false;
        }
        if self.enabled != enabled {
            self.enabled = enabled;
            agg_gui::animation::request_draw();
        }
    }

    /// Whether the pointer is over the enabled button (its hint showing, when it has one).
    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// The icon the button draws.
    pub fn icon(&self) -> BrowserIcon {
        self.which
    }

    fn contains(&self, p: Point) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x <= self.bounds.width && p.y <= self.bounds.height
    }

    /// The look's image for the button as it is now, asked for again when it changed.
    fn current_icon(&mut self) -> Option<IconImage> {
        let scale = agg_gui::device_scale();
        let generation = self.look.icon_generation();
        let fresh = matches!(&self.icon, Some((enabled, at, made_in, _))
            if *enabled == self.enabled && *at == scale && *made_in == generation);
        if !fresh {
            let image = self.look.icon(self.which, self.icon_size, self.enabled);
            self.icon = Some((self.enabled, scale, generation, image));
        }
        self.icon
            .as_ref()
            .and_then(|(_, _, _, image)| image.clone())
    }
}

/// `(x, y)` in the current drawing units, moved so it lands on the nearest whole device pixel
/// under the current transform. A turned or mirrored transform has no pixel grid to land on, so
/// the point is returned as it is.
fn snap_to_device_pixel(ctx: &dyn DrawCtx, x: f64, y: f64) -> (f64, f64) {
    let t = ctx.transform();
    let on_grid = t.shx == 0.0 && t.shy == 0.0 && t.sx > 0.0 && t.sy > 0.0;
    if !on_grid {
        return (x, y);
    }
    let snap = |v: f64, scale: f64, offset: f64| ((v * scale + offset).round() - offset) / scale;
    (snap(x, t.sx, t.tx), snap(y, t.sy, t.ty))
}

impl Widget for BrowserIconButton {
    fn type_name(&self) -> &'static str {
        "BrowserIconButton"
    }
    fn id(&self) -> Option<&str> {
        self.name.as_deref()
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
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn is_enabled(&self) -> bool {
        self.enabled
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.side, self.side)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (w, h) = (self.bounds.width, self.bounds.height);
        if self.enabled && self.hovered && self.hover_fill.a > 0.0 {
            ctx.set_fill_color(self.hover_fill);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
        if let Some(image) = self.current_icon() {
            let size = image.size();
            let (x, y) = snap_to_device_pixel(ctx, (w - size.width) / 2.0, (h - size.height) / 2.0);
            image.draw_in(ctx, Rect::new(x, y, size.width, size.height));
            return;
        }
        let color = if self.enabled {
            self.glyph_color
        } else {
            let mut dim = self.glyph_color;
            dim.a *= DISABLED_ALPHA as f32;
            dim
        };
        let size = self.side * GLYPH_SHARE;
        let text = self.which.glyph().to_string();
        let m = measure_text_metrics(&self.font, &text, size);
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(size);
        ctx.set_fill_color(color);
        let x = (w - m.width) / 2.0;
        let y = (h - (m.ascent - m.descent)) / 2.0;
        ctx.fill_text(&text, x, y);
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.enabled {
            return EventResult::Ignored;
        }
        match event {
            Event::MouseMove { pos } => {
                let hovered = self.contains(*pos);
                match self.set_hovered(hovered) {
                    true => EventResult::Consumed,
                    false => EventResult::Ignored,
                }
            }
            // Leaving keeps a press: pressing, dragging off and back, and releasing over the
            // button still clicks.
            Event::MouseLeave => {
                self.set_hovered(false);
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
                let clicked = std::mem::take(&mut self.pressed) && self.contains(*pos);
                if clicked {
                    if let Some(action) = self.on_click.as_mut() {
                        action();
                    }
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("icon", self.look.icon_name(self.which)),
            ("glyph", format!("U+{:04X}", self.which.glyph() as u32)),
            ("enabled", self.enabled.to_string()),
            ("tooltip", self.base.tooltip.clone().unwrap_or_default()),
            ("hint", self.hint.clone().unwrap_or_default()),
        ]
    }
}
