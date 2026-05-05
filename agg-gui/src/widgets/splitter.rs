//! `Splitter` — draggable divider between two children.
//!
//! Supports horizontal split (left | right) and vertical split (top / bottom).
//! Use [`Splitter::new`] for the historical horizontal layout, or
//! [`Splitter::vertical`] for the Y-axis variant.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// A draggable divider that splits its two children along one axis.
///
/// Horizontal: `children[0]` = left, `children[1]` = right; `ratio` is
/// the fraction of width going to `children[0]`.
///
/// Vertical: `children[0]` = top, `children[1]` = bottom; `ratio` is
/// the fraction of height going to `children[0]` (the top pane).
pub struct Splitter {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // exactly 2
    base: WidgetBase,
    /// Split position as a fraction of total length along the split axis.
    /// Clamped to [0.05, 0.95].
    pub ratio: f64,
    /// Width of the draggable divider strip (perpendicular to the split axis).
    pub divider_width: f64,
    /// `true` for top/bottom (Y-axis) split, `false` (default) for
    /// left/right (X-axis) split.
    pub vertical: bool,

    hovered: bool,
    dragging: bool,
}

impl Splitter {
    /// Horizontal split: `left` | `right`.
    pub fn new(left: Box<dyn Widget>, right: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![left, right],
            base: WidgetBase::new(),
            ratio: 0.5,
            divider_width: 6.0,
            vertical: false,
            hovered: false,
            dragging: false,
        }
    }

    /// Vertical split: `top` (visually upper, higher Y in agg-gui's Y-up
    /// coords) over `bottom`. `ratio` is the fraction of total height
    /// allocated to the top pane.
    pub fn vertical(top: Box<dyn Widget>, bottom: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![top, bottom],
            base: WidgetBase::new(),
            ratio: 0.5,
            divider_width: 6.0,
            vertical: true,
            hovered: false,
            dragging: false,
        }
    }

    pub fn with_ratio(mut self, ratio: f64) -> Self {
        self.ratio = ratio.clamp(0.05, 0.95);
        self
    }

    pub fn with_divider_width(mut self, w: f64) -> Self {
        self.divider_width = w;
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

    /// Length of the bounds along the splitting axis.
    fn axis_length(&self) -> f64 {
        if self.vertical {
            self.bounds.height
        } else {
            self.bounds.width
        }
    }

    /// Position of the divider along the splitting axis. For horizontal
    /// splits this is the X of the divider's left edge (so children[0]
    /// occupies x in [0, divider_pos]). For vertical splits in Y-up
    /// coords, children[0] is the top pane — its bottom edge is at
    /// `axis_length - divider_width - divider_pos_from_bottom` ... see
    /// the layout / paint / event branches for the worked-out coords.
    fn divider_pos(&self) -> f64 {
        (self.axis_length() - self.divider_width) * self.ratio
    }
}

impl Widget for Splitter {
    fn type_name(&self) -> &'static str {
        "Splitter"
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

    fn hit_test(&self, local_pos: Point) -> bool {
        // Capture all events during drag, even if cursor leaves bounds.
        if self.dragging {
            return true;
        }
        let b = self.bounds();
        local_pos.x >= 0.0
            && local_pos.x <= b.width
            && local_pos.y >= 0.0
            && local_pos.y <= b.height
    }

    fn layout(&mut self, available: Size) -> Size {
        let div = self.divider_width;

        if self.children.len() < 2 {
            return available;
        }

        if self.vertical {
            // Y-up: children[0] = top, children[1] = bottom. ratio is
            // the fraction of height going to the TOP pane.
            let top_h = ((available.height - div) * self.ratio).max(0.0);
            let bot_h = (available.height - div - top_h).max(0.0);
            let w = available.width;

            // Top pane sits above the divider — its bottom edge is at
            // bot_h + div, height extends up to bot_h + div + top_h.
            self.children[0].layout(Size::new(w, top_h));
            self.children[0].set_bounds(Rect::new(0.0, bot_h + div, w, top_h));

            // Bottom pane sits at y = 0, height bot_h.
            self.children[1].layout(Size::new(w, bot_h));
            self.children[1].set_bounds(Rect::new(0.0, 0.0, w, bot_h));
        } else {
            let left_w = ((available.width - div) * self.ratio).max(0.0);
            let right_w = (available.width - div - left_w).max(0.0);
            let h = available.height;

            self.children[0].layout(Size::new(left_w, h));
            self.children[0].set_bounds(Rect::new(0.0, 0.0, left_w, h));

            let right_x = left_w + div;
            self.children[1].layout(Size::new(right_w, h));
            self.children[1].set_bounds(Rect::new(right_x, 0.0, right_w, h));
        }

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let color = if self.dragging {
            v.accent.with_alpha(0.6)
        } else if self.hovered {
            v.text_color.with_alpha(0.15)
        } else {
            v.text_color.with_alpha(0.08)
        };

        let grip_color = if self.hovered || self.dragging {
            v.accent.with_alpha(0.7)
        } else {
            v.text_color.with_alpha(0.25)
        };

        ctx.set_fill_color(color);

        if self.vertical {
            // Divider is a horizontal strip at Y = bottom_pane_height.
            let bot_h = ((self.bounds.height - self.divider_width) * (1.0 - self.ratio)).max(0.0);
            let div_y = bot_h;
            let w = self.bounds.width;
            ctx.begin_path();
            ctx.rect(0.0, div_y, w, self.divider_width);
            ctx.fill();

            // Grip dots horizontally centered across the divider.
            if w > 30.0 {
                ctx.set_fill_color(grip_color);
                let cy = div_y + self.divider_width * 0.5;
                let cx = w * 0.5;
                for i in -1i32..=1 {
                    ctx.begin_path();
                    ctx.circle(cx + i as f64 * 5.0, cy, 1.5);
                    ctx.fill();
                }
            }
        } else {
            let div_x = self.divider_pos();
            let h = self.bounds.height;
            ctx.begin_path();
            ctx.rect(div_x, 0.0, self.divider_width, h);
            ctx.fill();

            if h > 30.0 {
                ctx.set_fill_color(grip_color);
                let cx = div_x + self.divider_width * 0.5;
                let cy = h * 0.5;
                for i in -1i32..=1 {
                    ctx.begin_path();
                    ctx.circle(cx, cy + i as f64 * 5.0, 1.5);
                    ctx.fill();
                }
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if self.vertical {
            let div = self.divider_width;
            let total = self.bounds.height;
            // Bottom pane height; divider's bottom edge is at this Y.
            let bot_h = ((total - div) * (1.0 - self.ratio)).max(0.0);
            let div_y = bot_h;
            let div_end = div_y + div;

            match event {
                Event::MouseMove { pos } => {
                    let over_div = pos.y >= div_y - 2.0 && pos.y <= div_end + 2.0;
                    let was = self.hovered;
                    self.hovered = over_div;
                    if self.dragging {
                        if total > div {
                            // ratio is fraction going to top — that's the
                            // upper portion above the divider midline.
                            // Convert pos.y (Y-up) into top fraction.
                            let div_mid = pos.y;
                            let top_h = (total - div_mid).max(0.0);
                            self.ratio = (top_h / total).clamp(0.05, 0.95);
                        }
                        crate::animation::request_draw();
                        EventResult::Consumed
                    } else {
                        if was != self.hovered {
                            crate::animation::request_draw();
                            return EventResult::Consumed;
                        }
                        EventResult::Ignored
                    }
                }
                Event::MouseDown {
                    pos,
                    button: MouseButton::Left,
                    ..
                } => {
                    if pos.y >= div_y - 2.0 && pos.y <= div_end + 2.0 {
                        self.dragging = true;
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                Event::MouseUp {
                    button: MouseButton::Left,
                    ..
                } => {
                    let was_dragging = self.dragging;
                    self.dragging = false;
                    if was_dragging {
                        crate::animation::request_draw();
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                _ => EventResult::Ignored,
            }
        } else {
            let div_x = self.divider_pos();
            let div_end = div_x + self.divider_width;

            match event {
                Event::MouseMove { pos } => {
                    let over_div = pos.x >= div_x - 2.0 && pos.x <= div_end + 2.0;
                    let was = self.hovered;
                    self.hovered = over_div;
                    if self.dragging {
                        let total = self.bounds.width;
                        if total > self.divider_width {
                            self.ratio = (pos.x / total).clamp(0.05, 0.95);
                        }
                        crate::animation::request_draw();
                        EventResult::Consumed
                    } else {
                        if was != self.hovered {
                            crate::animation::request_draw();
                            return EventResult::Consumed;
                        }
                        EventResult::Ignored
                    }
                }
                Event::MouseDown {
                    pos,
                    button: MouseButton::Left,
                    ..
                } => {
                    if pos.x >= div_x - 2.0 && pos.x <= div_end + 2.0 {
                        self.dragging = true;
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                Event::MouseUp {
                    button: MouseButton::Left,
                    ..
                } => {
                    let was_dragging = self.dragging;
                    self.dragging = false;
                    if was_dragging {
                        crate::animation::request_draw();
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                _ => EventResult::Ignored,
            }
        }
    }
}
