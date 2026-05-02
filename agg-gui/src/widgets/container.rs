//! `Container` — a rectangular box with optional background, border, and
//! padding that holds zero or more child widgets.
//!
//! Phase 4 child layout is a simple top-down vertical stack (bottom-most child
//! at `y = padding`, each subsequent child placed above the previous). Flex
//! layout arrives in Phase 5.

use crate::color::Color;
use crate::device_scale::device_scale;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// Inspector-visible properties of a [`Container`].
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[derive(Clone, Debug)]
pub struct ContainerProps {
    pub background: Color,
    pub border_color: Option<Color>,
    pub border_width: f64,
    pub corner_radius: f64,
    pub inner_padding: Insets,
    /// When `true`, `layout` returns the content's natural height + vertical
    /// padding instead of the full available height.  Off by default for
    /// backward compatibility (callers that used `Container` as a fill-
    /// parent decoration still work).  Match egui's `Frame` by opting in.
    pub fit_height: bool,
}

impl Default for ContainerProps {
    fn default() -> Self {
        Self {
            background: Color::rgba(0.0, 0.0, 0.0, 0.0),
            border_color: None,
            border_width: 1.0,
            corner_radius: 0.0,
            inner_padding: Insets::ZERO,
            fit_height: false,
        }
    }
}

/// A rectangular container widget.
///
/// Paints a background rounded-rect (optional border), then lets the framework
/// recurse into its children. Children are stacked bottom-to-top inside the
/// padding area.
pub struct Container {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    pub props: ContainerProps,
}

impl Container {
    /// Create a transparent container with no border and default padding.
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            props: ContainerProps::default(),
        }
    }

    /// Opt into content-fit height — [`layout`] returns
    /// `content_height + vertical_padding` instead of the full
    /// available height.  Required when this `Container` sits inside
    /// an auto-sized ancestor (e.g. `Window::with_auto_size(true)`),
    /// which would otherwise pick up the full available height as
    /// the container's preferred size and inflate the window.
    pub fn with_fit_height(mut self, fit: bool) -> Self {
        self.props.fit_height = fit;
        self
    }

    /// Append a child widget.
    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_background(mut self, color: Color) -> Self {
        self.props.background = color;
        self
    }

    pub fn with_border(mut self, color: Color, width: f64) -> Self {
        self.props.border_color = Some(color);
        self.props.border_width = width;
        self
    }

    pub fn with_corner_radius(mut self, r: f64) -> Self {
        self.props.corner_radius = r;
        self
    }

    pub fn with_padding(mut self, p: f64) -> Self {
        self.props.inner_padding = Insets::all(p);
        self
    }

    pub fn with_inner_padding(mut self, p: Insets) -> Self {
        self.props.inner_padding = p;
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
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn type_name(&self) -> &'static str {
        "Container"
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

    #[cfg(feature = "reflect")]
    fn as_reflect(&self) -> Option<&dyn bevy_reflect::Reflect> {
        Some(&self.props)
    }
    #[cfg(feature = "reflect")]
    fn as_reflect_mut(&mut self) -> Option<&mut dyn bevy_reflect::Reflect> {
        Some(&mut self.props)
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
    fn padding(&self) -> Insets {
        self.props.inner_padding
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
        let pad_l = self.props.inner_padding.left;
        let pad_r = self.props.inner_padding.right;
        let pad_t = self.props.inner_padding.top;
        let pad_b = self.props.inner_padding.bottom;
        let inner_w = (available.width - pad_l - pad_r).max(0.0);

        // Stack children top-to-bottom (first child = visually highest).
        // In Y-up coordinates, "top" = higher Y values.
        // Start cursor at the top of the inner area; move it downward each step.
        // Child margins are additive: top margin pushes the cursor down before
        // placing the child; bottom margin is consumed after it.
        let scale = device_scale();
        let start_cursor = available.height - pad_t;
        let mut cursor_y = start_cursor;

        for child in self.children.iter_mut() {
            let m = child.margin().scale(scale);
            let avail_w = (inner_w - m.left - m.right).max(0.0);
            let avail_h = (cursor_y - pad_b - m.top - m.bottom).max(0.0);
            let desired = child.layout(Size::new(avail_w, avail_h));

            // Top margin moves the cursor down before the child is placed.
            cursor_y -= m.top;
            let child_y = cursor_y - desired.height;
            let child_bounds = Rect::new(
                pad_l + m.left,
                child_y,
                desired.width.min(avail_w),
                desired.height,
            );
            child.set_bounds(child_bounds);
            // Bottom margin is consumed below the child.
            cursor_y = child_y - m.bottom;
        }

        // Default: fill the full available area (legacy — many demo
        // sites use `Container` as a decorated wrapper around content
        // that should stretch).  Opt in to content-fit via
        // `with_fit_height(true)` — matches egui `Frame` semantics.
        if self.props.fit_height {
            let consumed_h = (start_cursor - cursor_y).max(0.0);
            let natural_h = (consumed_h + pad_t + pad_b).min(available.height);
            Size::new(available.width, natural_h)
        } else {
            Size::new(available.width, available.height)
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.props.corner_radius;

        // Background
        if self.props.background.a > 0.001 {
            ctx.set_fill_color(self.props.background);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, r);
            ctx.fill();
        }

        // Border
        if let Some(bc) = self.props.border_color {
            ctx.set_stroke_color(bc);
            ctx.set_line_width(self.props.border_width);
            ctx.begin_path();
            let inset = self.props.border_width * 0.5;
            ctx.rounded_rect(
                inset,
                inset,
                (w - self.props.border_width).max(0.0),
                (h - self.props.border_width).max(0.0),
                r,
            );
            ctx.stroke();
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
