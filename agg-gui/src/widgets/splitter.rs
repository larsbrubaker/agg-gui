//! `Splitter` — draggable divider between two children.
//!
//! Supports horizontal split (left | right) and vertical split (top / bottom).
//! Use [`Splitter::new`] for the historical horizontal layout, or
//! [`Splitter::vertical`] for the Y-axis variant.
//!
//! [`SplitterRatio`] is the optional two-way channel a host uses to
//! *persist* a divider position: the widget publishes every drag into it
//! and adopts any value written from outside on its next layout. Without
//! it there is no way to either read the ratio back out of a boxed
//! widget tree or restore one — the field is only reachable while the
//! host still owns the concrete `Splitter`.
//!
//! [`Splitter::with_collapse_tab`] opts into a collapsible side: the bar
//! carries a pull tab ([`BarTab`], `splitter_bar_tab.rs`) that collapses one
//! pane to nothing and brings it back, the bar staying at the edge while it
//! is collapsed. The state is a shared [`SplitterCollapsed`] cell, two-way
//! like [`SplitterRatio`]; the ratio is left alone while collapsed, so a
//! click reopens the side at the width it had. Dragging the collapsed bar
//! reopens the side once the pointer reaches where the open bar can sit
//! (the ratio's drag range), and the bar follows the pointer from there. A
//! drag never collapses. Without a tab the splitter is unchanged.

use std::sync::{Arc, Mutex};

use crate::color::Color;
use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;
use crate::widgets::splitter_bar_tab::{BarTab, BarTabResponse, CollapseSide, SplitterCollapsed};

/// Shared, clonable divider position — the seam between a [`Splitter`]
/// buried in a widget tree and a host that wants to save or restore it.
///
/// Two-way by design: the splitter writes the cell on every drag, and
/// reads it at the start of each `layout()` so a host write (restoring a
/// document's saved layout) lands on the next frame. They never fight —
/// the value the splitter adopts is the value it would publish.
///
/// Poisoning is swallowed: the payload is one `f64` that no panic can
/// leave logically inconsistent, so both accessors take the inner value
/// rather than propagating a panic into the UI thread.
#[derive(Clone, Debug)]
pub struct SplitterRatio(Arc<Mutex<f64>>);

impl SplitterRatio {
    /// New channel seeded with `ratio` (clamped like
    /// [`Splitter::with_ratio`]; a non-finite seed falls back to a even
    /// split, see [`Self::set`]).
    pub fn new(ratio: f64) -> Self {
        Self(Arc::new(Mutex::new(clamp_ratio(ratio, 0.5))))
    }

    /// Current divider position as a fraction of the total length going
    /// to `children[0]` (left / top pane).
    pub fn get(&self) -> f64 {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Ask the splitter to move. Applied on its next layout pass.
    ///
    /// A non-finite ratio is **ignored** — `f64::clamp` passes `NaN`
    /// through, and a `NaN` divider makes both panes zero-sized with no
    /// way for the user to drag it back (the drag maths derives the new
    /// ratio from the old one). Restoring a corrupt document therefore
    /// leaves the splitter where it is rather than destroying the
    /// layout. Returns `true` when the value was accepted.
    pub fn set(&self, ratio: f64) -> bool {
        if !ratio.is_finite() {
            return false;
        }
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = ratio.clamp(RATIO_MIN, RATIO_MAX);
        crate::animation::request_draw();
        true
    }
}

/// Range the divider position is clamped to, at every entry point.
///
/// This is the *drag* range — how far the user may push the divider.
/// A host persisting the position may narrow it further (AtomArtist
/// clamps a saved divider to NodeDesigner's 20…80 % window before
/// writing it); the widget deliberately does not, because a pane
/// dragged to 5 % is a legitimate thing to do while working.
pub const RATIO_MIN: f64 = 0.05;
pub const RATIO_MAX: f64 = 0.95;

/// Clamp `ratio` into the drag range, substituting `fallback` for a
/// non-finite input (which `f64::clamp` would pass through untouched).
fn clamp_ratio(ratio: f64, fallback: f64) -> f64 {
    if ratio.is_finite() {
        ratio.clamp(RATIO_MIN, RATIO_MAX)
    } else {
        fallback
    }
}

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
    /// Optional host channel (see [`SplitterRatio`]).
    ratio_handle: Option<SplitterRatio>,
    /// The bar's pull tab, when a side is collapsible
    /// ([`Self::with_collapse_tab`]).
    collapse: Option<BarTab>,
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
            ratio_handle: None,
            collapse: None,
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
            ratio_handle: None,
            collapse: None,
        }
    }

    pub fn with_ratio(mut self, ratio: f64) -> Self {
        self.set_ratio(ratio);
        self
    }

    /// Install a shared [`SplitterRatio`] and adopt its current value.
    ///
    /// Call it *after* [`with_ratio`](Self::with_ratio): the handle is
    /// the authority once installed, which is what lets a host seed the
    /// default in one place (the handle's constructor) and have a
    /// restored document overwrite it later.
    pub fn with_ratio_handle(mut self, handle: SplitterRatio) -> Self {
        self.ratio = handle.get();
        self.ratio_handle = Some(handle);
        self
    }

    /// Clamp, store, and publish a new divider position. A non-finite
    /// ratio (an `x / 0` in a caller's own layout maths) leaves the
    /// divider where it is.
    fn set_ratio(&mut self, ratio: f64) {
        self.ratio = clamp_ratio(ratio, self.ratio);
        if let Some(handle) = &self.ratio_handle {
            handle.set(self.ratio);
        }
    }

    /// The resize drag's new ratio. A collapsed side stays collapsed until
    /// the ratio is inside the drag range (the pointer has reached where
    /// the open bar can sit), then reopens with the bar there, so the bar
    /// follows the pointer out from the edge. Open, this is [`Self::set_ratio`].
    fn drag_to(&mut self, ratio: f64) {
        if let Some(tab) = self.collapse.as_ref().filter(|tab| tab.collapsed()) {
            let reachable = match tab.side() {
                CollapseSide::Second => ratio <= RATIO_MAX,
                CollapseSide::First => ratio >= RATIO_MIN,
            };
            if !reachable {
                return;
            }
            tab.state().set(false);
        }
        self.set_ratio(ratio);
    }

    /// Adopt a host-written ratio, if the handle disagrees with ours.
    /// Called at the top of `layout()`.
    fn adopt_handle_ratio(&mut self) {
        if let Some(handle) = &self.ratio_handle {
            let wanted = handle.get();
            if wanted != self.ratio {
                self.ratio = wanted;
            }
        }
    }

    /// Make `side` collapsible by a pull tab on the bar, its state in
    /// `state` (see the module docs and [`BarTab`]). A click on the tab or a
    /// double click on the bar collapses or reopens the side; a drag on the
    /// bar still resizes. Collapsed, the side takes no room and the bar
    /// stays at the edge with its tab.
    pub fn with_collapse_tab(mut self, side: CollapseSide, state: SplitterCollapsed) -> Self {
        self.collapse = Some(BarTab::new(state, side));
        self
    }

    /// The pull tab's tooltip: `hide` while the side is open, `show` while
    /// it is collapsed (the host localizes both). Call after
    /// [`Self::with_collapse_tab`]; without a tab it does nothing.
    pub fn with_collapse_tooltips(
        mut self,
        hide: impl Into<String>,
        show: impl Into<String>,
    ) -> Self {
        if let Some(tab) = &mut self.collapse {
            tab.set_tooltips(hide, show);
        }
        self
    }

    /// The bar's colour with a pull tab (the host's theme splitter colour,
    /// opaque or translucent); without one the visuals' text colour at a low
    /// alpha. Call after [`Self::with_collapse_tab`]; without a tab it does
    /// nothing.
    pub fn with_bar_color(mut self, color: Color) -> Self {
        if let Some(tab) = &mut self.collapse {
            tab.set_bar_color(Some(color));
        }
        self
    }

    /// Whether the collapsible side is collapsed (`false` without one).
    pub fn is_collapsed(&self) -> bool {
        self.collapse.as_ref().is_some_and(BarTab::collapsed)
    }

    /// The bar's pull tab, when a side is collapsible.
    pub fn collapse_tab(&self) -> Option<&BarTab> {
        self.collapse.as_ref()
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

    /// The resize cursor for this split's orientation: a left/right
    /// split drags horizontally, a top/bottom split vertically. Matches
    /// egui's side/top panel resize handles.
    fn resize_cursor(&self) -> CursorIcon {
        if self.vertical {
            CursorIcon::ResizeVertical
        } else {
            CursorIcon::ResizeHorizontal
        }
    }

    /// Show the resize cursor while the bar is hovered and for the whole
    /// of a drag — the splitter holds mouse capture then, so it keeps
    /// receiving moves (and keeps the cursor) after the pointer leaves
    /// the bar. Otherwise leave the framework's per-move `Default`.
    fn apply_cursor(&self) {
        if self.hovered || self.dragging {
            set_cursor_icon(self.resize_cursor());
        }
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
        self.first_length(self.axis_length())
    }

    /// The first (left / top) pane's length along the split axis of a
    /// `total` long splitter: its share of what the bar leaves, or all or
    /// nothing while a side is collapsed.
    fn first_length(&self, total: f64) -> f64 {
        let room = total - self.divider_width;
        match self.collapse.as_ref() {
            Some(tab) if tab.collapsed() => match tab.side() {
                CollapseSide::First => 0.0,
                CollapseSide::Second => room.max(0.0),
            },
            _ => room * self.ratio,
        }
    }

    /// The bar's rectangle (local, Y up).
    pub fn bar_rect(&self) -> Rect {
        self.bar_rect_in(Size::new(self.bounds.width, self.bounds.height))
    }

    /// The bar's rectangle in a splitter of `size`.
    fn bar_rect_in(&self, size: Size) -> Rect {
        if self.vertical {
            let room = size.height - self.divider_width;
            let bot_h = if self.is_collapsed() {
                (room - self.first_length(size.height)).max(0.0)
            } else {
                // The plain splitter's own expression, kept bit for bit.
                (room * (1.0 - self.ratio)).max(0.0)
            };
            Rect::new(0.0, bot_h, size.width, self.divider_width)
        } else {
            let first = self.first_length(size.width);
            Rect::new(first, 0.0, self.divider_width, size.height)
        }
    }

    /// Put the pull tab on the bar, centred along it.
    fn place_tab(&mut self, size: Size) {
        let bar = self.bar_rect_in(size);
        let vertical = self.vertical;
        if let Some(tab) = &mut self.collapse {
            let center = if vertical {
                size.width / 2.0
            } else {
                size.height / 2.0
            };
            tab.place(bar, vertical, center);
        }
    }

    /// An event for a splitter with a pull tab: the tab's gestures first,
    /// then the resize drag it hands over. `None` leaves the event to the
    /// plain splitter's drag handling.
    fn tab_event(&mut self, event: &Event) -> Option<EventResult> {
        // The bar may have moved since the last layout (a drag, a host
        // write): hit-test the tab where it is now.
        self.place_tab(Size::new(self.bounds.width, self.bounds.height));
        let resize = self.resize_cursor();
        let dragging = self.dragging;
        let tab = self.collapse.as_mut()?;
        if let Event::MouseMove { pos } | Event::MouseDown { pos, .. } = event {
            // Resize on the bar, the normal pointer on the tab.
            if dragging || (!tab.pressing_tab() && tab.on_bar(*pos) && !tab.on_tab(*pos)) {
                set_cursor_icon(resize);
            }
        }
        match tab.on_event(event) {
            BarTabResponse::Consumed => {
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            BarTabResponse::BeginResize => {
                self.dragging = true;
                crate::animation::request_draw();
                match event {
                    // A mouse dragging the tab: this move is the drag's
                    // first, so the plain drag handling below applies it.
                    Event::MouseMove { .. } => None,
                    _ => Some(EventResult::Consumed),
                }
            }
            BarTabResponse::Toggled => {
                // The bar moved: hover and tooltip follow it now, not at
                // the next pointer move.
                self.place_tab(Size::new(self.bounds.width, self.bounds.height));
                if let Event::MouseMove { pos }
                | Event::MouseDown { pos, .. }
                | Event::MouseUp { pos, .. } = event
                {
                    if let Some(tab) = &mut self.collapse {
                        tab.update_hover(Some(*pos));
                    }
                }
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            BarTabResponse::Ignored => match event {
                // Off bar and tab: not the splitter's press.
                Event::MouseDown { .. } => Some(EventResult::Ignored),
                // Hover is the tab's (it asked for the draw).
                Event::MouseMove { .. } if !self.dragging => Some(EventResult::Ignored),
                _ => None,
            },
        }
    }
}

impl Widget for Splitter {
    crate::widgets::widget_as_any!();
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
        self.adopt_handle_ratio();
        let div = self.divider_width;

        if self.children.len() < 2 {
            return available;
        }

        if self.vertical {
            // Y-up: children[0] = top, children[1] = bottom. ratio is
            // the fraction of height going to the TOP pane.
            let top_h = self.first_length(available.height).max(0.0);
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
            let left_w = self.first_length(available.width).max(0.0);
            let right_w = (available.width - div - left_w).max(0.0);
            let h = available.height;

            self.children[0].layout(Size::new(left_w, h));
            self.children[0].set_bounds(Rect::new(0.0, 0.0, left_w, h));

            let right_x = left_w + div;
            self.children[1].layout(Size::new(right_w, h));
            self.children[1].set_bounds(Rect::new(right_x, 0.0, right_w, h));
        }

        self.place_tab(available);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.collapse.is_some() {
            // Bar and tab paint over the panes (`paint_overlay`): the tab
            // reaches into the pane beside the bar.
            return;
        }
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
            let div_y = self.bar_rect().y;
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

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.place_tab(Size::new(self.bounds.width, self.bounds.height));
        if let Some(tab) = &self.collapse {
            tab.paint(ctx, self.dragging);
        }
    }

    fn claims_pointer_exclusively(&self, local_pos: Point) -> bool {
        match &self.collapse {
            Some(tab) => self.dragging || tab.claims(local_pos),
            None => false,
        }
    }

    fn tooltip_text(&self) -> Option<&str> {
        self.collapse.as_ref().and_then(BarTab::tooltip)
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if self.collapse.is_some() {
            if let Some(result) = self.tab_event(event) {
                return result;
            }
        }
        // The press ended without a release (the window lost activation
        // mid-drag): the drag ends where it was — the ratio already
        // follows the pointer — instead of tracking the next hover.
        if let Event::MouseCaptureLost = event {
            return if std::mem::take(&mut self.dragging) {
                crate::animation::request_draw();
                EventResult::Consumed
            } else {
                EventResult::Ignored
            };
        }
        if self.vertical {
            let div = self.divider_width;
            let total = self.bounds.height;
            // Bottom pane height; divider's bottom edge is at this Y.
            let div_y = self.bar_rect().y;
            let div_end = div_y + div;

            match event {
                Event::MouseMove { pos } => {
                    let over_div = pos.y >= div_y - 2.0 && pos.y <= div_end + 2.0;
                    let was = self.hovered;
                    self.hovered = over_div;
                    self.apply_cursor();
                    if self.dragging {
                        if total > div {
                            // ratio is fraction going to top — that's the
                            // upper portion above the divider midline.
                            // Convert pos.y (Y-up) into top fraction.
                            let div_mid = pos.y;
                            let top_h = (total - div_mid).max(0.0);
                            self.drag_to(top_h / total);
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
                    self.apply_cursor();
                    if self.dragging {
                        let total = self.bounds.width;
                        if total > self.divider_width {
                            self.drag_to(pos.x / total);
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
