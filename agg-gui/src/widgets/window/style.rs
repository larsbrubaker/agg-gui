// The panel look of a `Window` as agg-sharp's styled `WindowWidget` exposes it
// (`CornerRadius`, `ShadowColor`/`ShadowOffset`/`ShadowBlur`, `TitleBarColor`, `BackgroundColor`,
// `WindowBorder`/`WindowBorderColor`, and the panel's `DoubleBuffer`), read
// and set at runtime. Every option defaults to the look `Window` has always
// had: radius 8, the drop shadow on, theme colours, a 1 px border, and the
// panel painted through its own rounded-clip layer.
//
// `paint.rs` reads these when it draws the shadow, body, title bar and border
// (`chrome.rs` holds the stateless helpers); `widget_impl.rs` reads the radius
// for the retained layer's rounded clip and `paint_overlay` for the resize
// highlight's tangent points.

use std::cell::Cell;

use super::*;
use crate::color::Color;

/// Per-window overrides of the chrome look. See the setters on [`Window`].
pub(super) struct PanelStyle {
    /// Corner radius of the panel, in logical units. `0` is a square window.
    pub(super) corner_radius: f64,
    /// Whether the soft drop shadow is drawn around the panel.
    pub(super) shadow: bool,
    /// The shadow's colour, offset and blur in place of the default halo.
    pub(super) shadow_style: Option<WindowShadow>,
    /// Title-bar fill in place of the theme's `window_title_fill`.
    pub(super) title_bar_color: Option<Color>,
    /// Body fill in place of the theme's `window_fill`.
    pub(super) body_color: Option<Color>,
    /// Width of the outer border stroke, drawn inside the panel's edge.
    pub(super) border_width: f64,
    /// Border colour in place of the theme's `window_stroke`.
    pub(super) border_color: Option<Color>,
    /// Whether a rounded panel paints through its own layer, which the
    /// rounded clip cuts (agg-sharp's panel `DoubleBuffer`).
    pub(super) panel_layer: bool,
    /// Whether the last paint went through the panel layer.
    pub(super) panel_layered: Cell<bool>,
}

impl Default for PanelStyle {
    fn default() -> Self {
        Self {
            corner_radius: CORNER_R,
            shadow: true,
            shadow_style: None,
            title_bar_color: None,
            body_color: None,
            border_width: 1.0,
            border_color: None,
            panel_layer: true,
            panel_layered: Cell::new(false),
        }
    }
}

/// A window's drop shadow as agg-sharp's `WindowWidget` describes it
/// (`ShadowColor`, `ShadowOffset`, `ShadowBlur`), in logical units. Set
/// with [`Window::set_shadow_style`].
///
/// The blur is stacked translucent rounded rectangles, widest first, one per
/// device pixel of blur (at most 12), each with the alpha that makes all of
/// them together reach `color`'s. A shadow reaching past the window's halo
/// (blur 14, offset (2, -6)) is cut at the halo's edge on a backend that
/// retains the window as a layer, as agg-sharp cuts it at the grab border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowShadow {
    /// The shadow's colour; its alpha is how dark it is at its darkest.
    pub color: Color,
    /// How far the shadow is shifted right of the panel.
    pub offset_x: f64,
    /// How far the shadow is shifted up from the panel (negative is down).
    pub offset_y: f64,
    /// How far the shadow fades out past the panel's edge.
    pub blur: f64,
}

impl Window {
    fn restyled(&mut self) {
        self.backbuffer.invalidate();
        crate::animation::request_draw();
    }

    /// The panel's corner radius in logical units (default 8).
    pub fn corner_radius(&self) -> f64 {
        self.panel_style.corner_radius
    }

    /// Round the panel's corners by `radius` logical units (agg-sharp
    /// `WindowWidget.CornerRadius`). Whatever the title bar and content paint
    /// is cut to the rounded corners; the shadow, drawn outside the panel, is
    /// not. `0` gives a square window that paints straight onto its parent,
    /// with no panel layer and no clip. Negative and NaN read as `0`.
    pub fn set_corner_radius(&mut self, radius: f64) {
        let radius = if radius.is_nan() {
            0.0
        } else {
            radius.max(0.0)
        };
        if radius != self.panel_style.corner_radius {
            self.panel_style.corner_radius = radius;
            self.restyled();
        }
    }

    /// Builder form of [`Window::set_corner_radius`].
    pub fn with_corner_radius(mut self, radius: f64) -> Self {
        self.set_corner_radius(radius);
        self
    }

    /// Whether the drop shadow is drawn (default `true`).
    pub fn shadow_shown(&self) -> bool {
        self.panel_style.shadow
    }

    /// Draw the drop shadow around the panel (`true`) or not (agg-sharp's
    /// transparent `ShadowColor`).
    pub fn set_shadow(&mut self, shown: bool) {
        if shown != self.panel_style.shadow {
            self.panel_style.shadow = shown;
            self.restyled();
        }
    }

    /// The shadow's colour, offset and blur, `None` for the default halo.
    pub fn shadow_style(&self) -> Option<WindowShadow> {
        self.panel_style.shadow_style
    }

    /// Draw the drop shadow as `style` describes it (agg-sharp's
    /// `ShadowColor`, `ShadowOffset`, `ShadowBlur`) instead of the default
    /// halo; `None` goes back to the default. [`Window::set_shadow`] still
    /// turns the shadow off and on.
    pub fn set_shadow_style(&mut self, style: Option<WindowShadow>) {
        if style != self.panel_style.shadow_style {
            self.panel_style.shadow_style = style;
            self.restyled();
        }
    }

    /// Builder form of [`Window::set_shadow_style`].
    pub fn with_shadow_style(mut self, style: WindowShadow) -> Self {
        self.set_shadow_style(Some(style));
        self
    }

    /// The title-bar colour override, `None` for the theme's.
    pub fn title_bar_color(&self) -> Option<Color> {
        self.panel_style.title_bar_color
    }

    /// Fill the title bar with `color` instead of the theme's title colour
    /// (agg-sharp `WindowWidget.TitleBarColor`); `None` goes back to the
    /// theme. The colour follows the panel's rounded top corners.
    pub fn set_title_bar_color(&mut self, color: Option<Color>) {
        if color != self.panel_style.title_bar_color {
            self.panel_style.title_bar_color = color;
            self.restyled();
        }
    }

    /// The body colour override, `None` for the theme's.
    pub fn body_color(&self) -> Option<Color> {
        self.panel_style.body_color
    }

    /// Fill the panel's body with `color` instead of the theme's window fill
    /// (agg-sharp's `BackgroundColor` on a rounded window); `None` goes back
    /// to the theme.
    pub fn set_body_color(&mut self, color: Option<Color>) {
        if color != self.panel_style.body_color {
            self.panel_style.body_color = color;
            self.restyled();
        }
    }

    /// The border stroke's width (default 1) and colour override.
    pub fn border(&self) -> (f64, Option<Color>) {
        (self.panel_style.border_width, self.panel_style.border_color)
    }

    /// Stroke the panel's outer border `width` logical units wide, inside its
    /// edge, in `color` (or the theme's window stroke for `None`). agg-sharp
    /// `WindowWidget.WindowBorder` and `WindowBorderColor`. Negative and NaN
    /// widths read as `0`, which draws no border.
    pub fn set_border(&mut self, width: f64, color: Option<Color>) {
        let width = if width.is_nan() { 0.0 } else { width.max(0.0) };
        if width != self.panel_style.border_width || color != self.panel_style.border_color {
            self.panel_style.border_width = width;
            self.panel_style.border_color = color;
            self.restyled();
        }
    }

    /// Whether a rounded panel paints through its own clipped layer (default
    /// `true`).
    pub fn panel_layer_enabled(&self) -> bool {
        self.panel_style.panel_layer
    }

    /// Paint a rounded panel through its own layer, cut by the rounded clip
    /// (`true`, the default), or straight onto what is behind it with no clip
    /// (agg-sharp's panel `DoubleBuffer`). A square window never uses the
    /// layer.
    pub fn set_panel_layer(&mut self, layered: bool) {
        if layered != self.panel_style.panel_layer {
            self.panel_style.panel_layer = layered;
            self.restyled();
        }
    }

    /// Whether the last paint drew the panel through its own clipped layer
    /// (agg-sharp's `TitleBar.Parent.DoubleBuffer` with a live `BackBuffer`).
    pub fn paints_through_panel_layer(&self) -> bool {
        self.panel_style.panel_layered.get()
    }

    /// Whether the panel paints through its layer on a backend that can
    /// composite one.
    pub(super) fn wants_panel_layer(&self) -> bool {
        self.panel_style.panel_layer && self.panel_style.corner_radius > 0.0
    }
}
