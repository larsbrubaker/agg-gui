//! `Button` — a clickable, compositional button with a `Label` child.

use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{measure_advance, Font};
use crate::widget::Widget;
use crate::widgets::label::{Label, LabelAlign};

/// Icon glyph drawn at the leading edge of a [`Button`]'s label.
/// The glyph is rendered with a separate font so callers can pair
/// e.g. a Font Awesome glyph with a Latin-only text font.
#[derive(Clone)]
pub struct ButtonIcon {
    pub glyph: char,
    pub font: Arc<Font>,
    pub font_size: f64,
}

/// Spacing between the icon glyph and the label text, in pixels.
const ICON_GAP: f64 = 8.0;

/// Default horizontal padding used to inset a left- or right-aligned label
/// from the button edge.  Center-aligned labels ignore this and centre
/// inside the button bounds.
const LEFT_LABEL_PAD: f64 = 8.0;

pub use super::button_theme::ButtonTheme;

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
    /// Optional toggle: when `Some` and the closure returns `true`, the
    /// button paints with the accent / selected appearance regardless of
    /// hover / press state.  When the closure returns `false`, an active-
    /// aware button uses the subtle (`widget_bg`) variant so segmented
    /// selectors look right.  `None` = legacy behaviour: always painted as
    /// the accent button.
    active_fn: Option<Rc<dyn Fn() -> bool>>,
    /// `true` selects the muted "secondary" visual style (theme widget_bg
    /// + theme text colour) instead of the accent appearance.  Combined
    /// with `active_fn`, this drives segmented toggles: each segment is a
    /// subtle button that flips to the accent look when its `active_fn`
    /// returns true.
    subtle: bool,
    /// When `true` AND in the inactive state, the inactive background
    /// is fully transparent (no fill) so the button reads as part of
    /// its parent — sidebar list rows want this.  Hovered / pressed
    /// inactive states paint a faint text-coloured overlay instead of
    /// the `widget_bg` shade.  Active state is unaffected.
    ghost: bool,
    /// When `true`, draw a 1-px stroke around the button rect using the
    /// theme's `widget_stroke` colour while inactive — gives subtle
    /// segmented buttons a defined edge so they don't visually bleed
    /// into a parent that has the same `widget_bg` shade.  Active state
    /// already has a high-contrast accent fill and skips the stroke.
    outlined: bool,
    /// How the child label is positioned inside the button rect.
    /// `Center` (default) centres horizontally; `Left` insets by
    /// [`LEFT_LABEL_PAD`] and is the right choice for full-width
    /// sidebar rows where the label hugs the leading edge.
    label_align: LabelAlign,
    /// Custom horizontal inset applied when `label_align` is `Left` or
    /// `Right`.  Defaults to [`LEFT_LABEL_PAD`]; sidebar entries with
    /// indent > 0 set this to push the label past a group-marker
    /// triangle.
    label_pad_h: f64,

    /// Optional icon glyph painted at the leading edge of the label.
    /// See [`with_icon`](Self::with_icon).
    icon: Option<ButtonIcon>,

    /// When true, drop the 48 px touch-target width floor and shrink
    /// the horizontal padding. Right for icon-only toolbar buttons
    /// that want to sit tightly next to each other; defaults false
    /// so regular buttons keep the comfortable touch target.
    compact: bool,

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
            active_fn: None,
            subtle: false,
            ghost: false,
            outlined: false,
            label_align: LabelAlign::Center,
            label_pad_h: LEFT_LABEL_PAD,
            icon: None,
            compact: false,
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

    /// Bind the button's "selected" state to a live predicate.  When the
    /// closure returns `true`, the button paints with the accent surface
    /// regardless of hover / press; when it returns `false`, an
    /// active-aware button (i.e. `with_subtle()` is also set) reverts to
    /// the muted `widget_bg` appearance.  Used to compose segmented
    /// toggles out of plain `Button`s without hand-rolled paint code.
    pub fn with_active_fn(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.active_fn = Some(Rc::new(f));
        self
    }

    /// Override how the child label is aligned inside the button rect.
    /// Defaults to [`LabelAlign::Center`].  Use [`LabelAlign::Left`] for
    /// full-width sidebar rows where the label hugs the leading edge.
    /// Also rebuilds the child Label so its own internal alignment matches.
    pub fn with_label_align(mut self, align: LabelAlign) -> Self {
        self.label_align = align;
        self.children[0] = Box::new(
            Label::new(&self.label_text, Arc::clone(&self.font))
                .with_font_size(self.font_size)
                .with_color(self.theme.label_color)
                .with_align(align),
        );
        self
    }

    /// Override the horizontal padding used when `label_align` is `Left`
    /// or `Right`.  Defaults to a small visual gutter; bump it up to indent
    /// the label past a group-marker triangle in sidebar rows.
    pub fn with_label_pad_h(mut self, pad: f64) -> Self {
        self.label_pad_h = pad;
        self
    }

    /// Compact mode: drop the 48 px width floor and use a tighter
    /// horizontal padding. Use for icon-only toolbar buttons where
    /// you want them packed close to the glyph; the 48 px touch-
    /// target default is right for stand-alone buttons but wastes
    /// horizontal space when 5+ icon buttons need to sit next to
    /// each other on a narrow mobile bar.
    pub fn with_compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Paint an icon glyph at the leading edge of the label.
    /// `icon_font` carries the glyph (e.g. a Font Awesome face);
    /// the label text continues to render in the button's main
    /// font, so callers can pair a Latin text font with an
    /// icon-only font without merging them.
    ///
    /// Defaults `font_size` to the button's current `font_size`.
    /// Use [`with_icon_sized`](Self::with_icon_sized) to scale the
    /// icon independently.
    pub fn with_icon(mut self, glyph: char, icon_font: Arc<Font>) -> Self {
        let font_size = self.font_size;
        self.icon = Some(ButtonIcon {
            glyph,
            font: icon_font,
            font_size,
        });
        self
    }

    /// Like [`with_icon`](Self::with_icon) but with an explicit
    /// icon font size — useful when the icon font's glyphs read
    /// larger or smaller than the text at the same point size.
    pub fn with_icon_sized(mut self, glyph: char, icon_font: Arc<Font>, font_size: f64) -> Self {
        self.icon = Some(ButtonIcon {
            glyph,
            font: icon_font,
            font_size,
        });
        self
    }

    /// Use a transparent inactive background + faint text-coloured
    /// hover/pressed overlay instead of the muted `widget_bg` fill.
    /// Implies [`with_subtle`] (theme text colour, accent on active).
    /// Right for sidebar list rows where the inactive state should
    /// blend with the panel.
    pub fn with_ghost(mut self) -> Self {
        self.subtle = true;
        self.ghost = true;
        let theme_text = crate::theme::current_visuals().text_color;
        self.children[0] =
            Self::build_label_with_color(&self.label_text, &self.font, self.font_size, theme_text);
        self
    }

    /// Switch to the muted (secondary) visual style: theme `widget_bg`
    /// fill, theme `text_color` label.  Pair with [`with_active_fn`] to
    /// build segmented controls — inactive segments paint subtle, the
    /// selected segment flips to the accent surface.
    /// Draw a 1-px `widget_stroke` outline around the button while inactive.
    /// Combined with [`Self::with_subtle`] this gives top-bar segmented
    /// controls a defined edge so they don't visually bleed into a parent
    /// that shares the same `widget_bg` colour.  Active state already paints
    /// a high-contrast accent fill and skips the stroke.
    pub fn with_outlined(mut self) -> Self {
        self.outlined = true;
        self
    }

    pub fn with_subtle(mut self) -> Self {
        self.subtle = true;
        // Subtle buttons use the theme's text colour, not the white-on-accent
        // default.  Rebuild the label with the active visuals' text colour
        // (the paint pass also retints each frame, so this just gives a
        // sensible first-paint colour before the visuals are queried).
        let theme_text = crate::theme::current_visuals().text_color;
        self.children[0] =
            Self::build_label_with_color(&self.label_text, &self.font, self.font_size, theme_text);
        self
    }

    fn is_enabled(&self) -> bool {
        self.enabled_fn.as_ref().map(|f| f()).unwrap_or(true)
    }

    fn is_active(&self) -> bool {
        self.active_fn.as_ref().map(|f| f()).unwrap_or(true)
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

    fn position_label(&mut self, size: Size, label_size: Size) {
        // Width contributed by the leading icon glyph (icon advance
        // + spacing gap). Zero when no icon is configured.
        let icon_block_w = self
            .icon
            .as_ref()
            .map(|i| measure_advance(&i.font, &i.glyph.to_string(), i.font_size) + ICON_GAP)
            .unwrap_or(0.0);
        // The (icon + gap + label) group is positioned as a unit;
        // align uses the COMBINED width so the icon stays directly
        // left of the label for any alignment mode.
        let group_w = label_size.width + icon_block_w;
        let group_x = match self.label_align {
            LabelAlign::Left => self.label_pad_h.min(size.width),
            LabelAlign::Right => (size.width - group_w - self.label_pad_h).max(0.0),
            LabelAlign::Center => ((size.width - group_w) * 0.5).max(0.0),
        };
        let label_x = group_x + icon_block_w;
        let label_y = ((size.height - label_size.height) * 0.5).max(0.0);
        self.children[0].set_bounds(Rect::new(
            label_x,
            label_y,
            label_size.width,
            label_size.height,
        ));
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
        Self::build_label_with_color(text, font, font_size, theme.label_color)
    }

    fn build_label_with_color(
        text: &str,
        font: &Arc<Font>,
        font_size: f64,
        color: Color,
    ) -> Box<dyn Widget> {
        Box::new(
            Label::new(text, Arc::clone(font))
                .with_font_size(font_size)
                .with_color(color)
                .with_align(LabelAlign::Center),
        )
    }

    /// Render the configured icon glyph centred vertically in the
    /// button using the glyph's *actual* outline bounding box — not
    /// the font's worst-case ascender/descender. Icon fonts (Font
    /// Awesome especially) place each glyph in a sub-rectangle of
    /// the design space; centring by the font metric leaves the glyph
    /// visibly high on the button (the "icons floating to the top"
    /// regression we've hit repeatedly). With the per-glyph bbox we
    /// solve for the baseline that puts the glyph's vertical midpoint
    /// at `button_h / 2`.
    fn paint_icon(
        ctx: &mut dyn DrawCtx,
        icon: &Option<ButtonIcon>,
        _label_font: &Arc<Font>,
        _label_font_size: f64,
        x: f64,
        button_h: f64,
        color: Color,
    ) {
        let Some(icon) = icon else { return };
        // (y_min, y_max) is the glyph's actual extent in pixels
        // relative to baseline, Y-up. y_min is usually negative
        // (descender region) or ~0, y_max is the cap-height of the
        // glyph. Pick the baseline so that
        //   baseline + (y_min + y_max) / 2  ==  button_h / 2
        // i.e. the glyph's midpoint sits at the button's midpoint.
        // Fall back to the font metric only if the glyph has no
        // outline (e.g. a space or a missing glyph).
        let baseline_y = match icon.font.glyph_visual_bounds(icon.glyph, icon.font_size) {
            Some((y_min, y_max)) => (button_h * 0.5 - (y_min + y_max) * 0.5).max(0.0),
            None => ((button_h - icon.font_size) * 0.5).max(0.0),
        };
        ctx.set_font(Arc::clone(&icon.font));
        ctx.set_font_size(icon.font_size);
        ctx.set_fill_color(color);
        ctx.fill_text(&icon.glyph.to_string(), x, baseline_y);
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

    fn layout(&mut self, available: Size) -> Size {
        let natural_height = (self.font_size * 1.7).max(24.0);
        let height = if available.height > 0.0 {
            natural_height.min(available.height)
        } else {
            natural_height
        };
        // Measure the label first so we can report a "fit" width — label
        // width plus horizontal padding — instead of stretching to the
        // whole available width.  This keeps Buttons polite siblings in a
        // `FlexRow`.  Parents that want a full-width button can:
        //   - wrap it in a `SizedBox` with an explicit width, or
        //   - apply `HAnchor::STRETCH`, or
        //   - set `with_min_size(Size::new(width, _))` for a width floor.
        // Compact mode tightens the horizontal pad and drops the
        // 48 px touch-target floor — icon-only toolbar buttons that
        // need to sit next to each other on a narrow bar would
        // otherwise eat all the row width.
        let pad_h = if self.compact {
            self.font_size * 0.7
        } else {
            self.font_size * 1.2
        };
        let label_size = self.children[0].layout(Size::new(available.width, height));
        let icon_block_w = self
            .icon
            .as_ref()
            .map(|i| measure_advance(&i.font, &i.glyph.to_string(), i.font_size) + ICON_GAP)
            .unwrap_or(0.0);
        let min_w = if self.compact { 0.0 } else { 48.0 };
        let natural_w = (label_size.width + icon_block_w + pad_h)
            .max(min_w)
            .max(self.base.min_size.width);
        let width = if self.base.h_anchor.is_stretch() {
            available.width.max(natural_w)
        } else {
            natural_w
        }
        .min(available.width);
        let size = Size::new(width, height);
        self.position_label(size, label_size);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.theme.border_radius;
        let enabled = self.is_enabled();
        let v = ctx.visuals();
        let use_visuals = self.theme == ButtonTheme::default();
        let active = self.is_active();
        // A subtle button paints in muted theme colours when inactive, and
        // flips to the accent surface (white text on accent fill) when its
        // `active_fn` returns true.  Plain (non-subtle) buttons always use
        // the accent surface — that's the existing primary-button look.
        let muted = self.subtle && !active;

        // Focus ring — drawn JUST INSIDE the button bounds so the parent's
        // `clip_children_rect` (defaults to widget bounds) doesn't chop
        // the leftmost stroke pixel when the button sits flush against
        // a container edge.  Painting outside-bounds with negative
        // coordinates was the long-standing cause of "the left edge of
        // my button looks clipped" reports.
        if enabled && self.focused {
            let ring = self.theme.focus_ring_width;
            let focus_ring = if use_visuals {
                v.accent_focus
            } else {
                self.theme.focus_ring_color
            };
            ctx.set_stroke_color(focus_ring);
            ctx.set_line_width(ring);
            ctx.begin_path();
            let inset = ring * 0.5;
            ctx.rounded_rect(
                inset,
                inset,
                (w - ring).max(0.0),
                (h - ring).max(0.0),
                (r - inset).max(0.0),
            );
            ctx.stroke();
        }

        // Background — color depends on interaction state. Disabled buttons
        // use neutral widget colors instead of a washed-out accent, so they
        // don't look like secondary active actions.
        let base_bg = if muted && self.ghost && self.pressed {
            // Ghost (transparent-inactive) buttons paint a faint
            // text-coloured overlay on hover / press instead of the
            // widget_bg shade.  Matches the egui-style sidebar row
            // look the demo's `ToggleButton` had before refactor.
            Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.16)
        } else if muted && self.ghost && self.hovered {
            Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.10)
        } else if muted && self.ghost {
            // Fully transparent when the user isn't interacting.
            Color::rgba(0.0, 0.0, 0.0, 0.0)
        } else if muted && (self.pressed || self.hovered) {
            v.widget_bg_hovered
        } else if muted {
            v.widget_bg
        } else if use_visuals && self.pressed {
            v.accent_pressed
        } else if use_visuals && self.hovered {
            v.accent_hovered
        } else if use_visuals {
            v.accent
        } else if self.pressed {
            self.theme.background_pressed
        } else if self.hovered {
            self.theme.background_hovered
        } else {
            self.theme.background
        };
        let (disabled_bg, disabled_stroke, _) = Self::disabled_colors(&v);
        let bg = if enabled { base_bg } else { disabled_bg };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        // Optional outline — opt-in via `with_outlined()` for inactive
        // segmented buttons that want a defined edge against a same-colour
        // parent (e.g. top-bar tabs).  Active state already has a
        // high-contrast accent fill and skips this so the selected segment
        // visually pops.
        if enabled && self.outlined && !active {
            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0), r);
            ctx.stroke();
        }

        // Retint the child label so subtle / active states show the right
        // foreground colour without rebuilding the Label widget.  Calling
        // through the dyn Widget keeps Button agnostic of the concrete
        // Label type — `set_label_color` is a default no-op that Label
        // overrides, see `Widget::set_label_color`.
        let label_color = if muted {
            v.text_color
        } else {
            self.theme.label_color
        };
        if let Some(child) = self.children.get_mut(0) {
            child.set_label_color(label_color);
        }

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
        let enabled = self.is_enabled();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.theme.border_radius;
        let v = ctx.visuals();

        if !enabled {
            // The normal child Label was built for the enabled foreground
            // colour. Cover it and repaint the label with the disabled
            // text colour. Icon (if any) renders in the same disabled
            // text colour at the same group_x as layout positioned it.
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

            let font = crate::font_settings::current_system_font()
                .unwrap_or_else(|| Arc::clone(&self.font));
            let icon_block_w = self
                .icon
                .as_ref()
                .map(|i| measure_advance(&i.font, &i.glyph.to_string(), i.font_size) + ICON_GAP)
                .unwrap_or(0.0);
            ctx.set_font(font);
            ctx.set_font_size(self.font_size * crate::font_settings::current_font_size_scale());
            ctx.set_fill_color(disabled_text);
            if let Some(m) = ctx.measure_text(&self.label_text) {
                let group_w = m.width + icon_block_w;
                let group_x = ((w - group_w) * 0.5).max(0.0);
                let tx = group_x + icon_block_w;
                let ty = m.centered_baseline_y(h).max(0.0);
                ctx.fill_text(&self.label_text, tx, ty);
                Self::paint_icon(
                    ctx,
                    &self.icon,
                    &self.font,
                    self.font_size,
                    group_x,
                    h,
                    disabled_text,
                );
            }
            return;
        }

        // Enabled state — only paint the icon (label has already been
        // drawn by the framework via the child Label's paint).
        if let Some(icon) = self.icon.clone() {
            let active = self.is_active();
            let muted = self.subtle && !active;
            let label_color = if muted {
                v.text_color
            } else {
                self.theme.label_color
            };
            let label_x = self
                .children
                .first()
                .map(|c| c.bounds().x)
                .unwrap_or_default();
            let icon_block_w =
                measure_advance(&icon.font, &icon.glyph.to_string(), icon.font_size) + ICON_GAP;
            let group_x = (label_x - icon_block_w).max(0.0);
            Self::paint_icon(
                ctx,
                &Some(icon),
                &self.font,
                self.font_size,
                group_x,
                h,
                label_color,
            );
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
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                if !self.pressed {
                    crate::animation::request_draw();
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
                    crate::animation::request_draw();
                }
                if was_pressed && self.hovered {
                    self.fire_click();
                    // Clear the focus ring after a mouse click — the ring is a
                    // keyboard-navigation aid and should not persist after a
                    // pointer interaction.
                    self.focused = false;
                    // Click handler almost always mutates app state that
                    // affects the next paint; request one so the handler's
                    // side-effects are visible.
                    crate::animation::request_draw();
                }
                EventResult::Consumed
            }
            Event::KeyDown { key, .. } => {
                use crate::event::Key;
                match key {
                    Key::Enter | Key::Char(' ') => {
                        self.fire_click();
                        crate::animation::request_draw();
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            Event::FocusGained => {
                let was = self.focused;
                self.focused = true;
                if !was {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusLost => {
                let was_focused = self.focused;
                let was_pressed = self.pressed;
                self.focused = false;
                self.pressed = false;
                if was_focused || was_pressed {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
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
