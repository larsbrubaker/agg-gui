//! `ScrollView` — scrolling container with egui-style scrollbars.
//!
//! Supports vertical, horizontal, or bidirectional scrolling.  The scrollbar
//! can be styled in detail (bar width, margins, fade, solid vs floating) via
//! [`ScrollBarStyle`] — set by value, by builder, or bound to an
//! `Rc<Cell<ScrollBarStyle>>` for live tweaking (used by the demo Appearance
//! tab).
//!
//! # Coordinate system
//! All local coordinates are Y-up.  `scroll_offset` is "how far the user has
//! scrolled down from the top" — `0` shows the TOP of the content,
//! `max_scroll_y` shows the BOTTOM.  Same convention for horizontal:
//! `h_scroll_offset = 0` shows the LEFT of the content.
//!
//! # Virtual rendering
//! `with_viewport_cell(Rc<Cell<Rect>>)` publishes the currently-visible
//! content-space rect each layout.  Children that want to cull off-viewport
//! work (e.g. painting 10k row labels) read this cell and limit their paint.

use std::cell::Cell;
use std::rc::Rc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// How the scrollbar is shown.  Matches egui's `ScrollBarVisibility`.
///
/// Hover-only behaviour is controlled by [`ScrollBarKind::Floating`] on the
/// [`ScrollBarStyle`], not by this enum — a Floating bar with
/// `VisibleWhenNeeded` only appears on hover; a Solid bar with
/// `VisibleWhenNeeded` is always visible when content overflows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollBarVisibility {
    /// Paint whenever content overflows, regardless of hover.
    AlwaysVisible,
    /// Paint when content overflows.  If the style is `Floating` the bar
    /// additionally hides until the cursor enters the hover zone.
    VisibleWhenNeeded,
    /// Never paint — wheel/drag still work, but no visual indicator.
    AlwaysHidden,
}

impl Default for ScrollBarVisibility {
    fn default() -> Self {
        Self::VisibleWhenNeeded
    }
}

/// Whether the bar reserves layout space (Solid) or floats over content (Floating).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollBarKind {
    Solid,
    Floating,
}

impl Default for ScrollBarKind {
    fn default() -> Self {
        Self::Floating
    }
}

/// Which pair of colours is used for the track vs thumb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollBarColor {
    /// Track = neutral background; thumb = slightly brighter.  Default.
    Background,
    /// Track = transparent; thumb = accent-tinted foreground.
    Foreground,
}

impl Default for ScrollBarColor {
    fn default() -> Self {
        Self::Background
    }
}

/// Full scrollbar appearance configuration — mirrors egui's `style.spacing.scroll`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBarStyle {
    /// Width of the full-size bar in pixels.  This is the bar width when the
    /// user is hovering or interacting with it.
    pub bar_width: f64,
    /// Thin width shown when the bar is dormant (not hovered, not dragging).
    /// Matches egui's `floating_width`.  On hover the bar grows from this to
    /// [`Self::bar_width`].  Set equal to `bar_width` to disable the expand
    /// effect.  Only takes effect when smaller than `bar_width`.
    pub floating_width: f64,
    /// Minimum length of the draggable thumb.
    pub handle_min_length: f64,
    /// Space between the bar and the panel's outer edge.
    pub outer_margin: f64,
    /// Space between the bar and the content area.
    pub inner_margin: f64,
    /// Space between sibling content and the bar area (applied when `kind = Solid`
    /// and as a decorative inset when `Floating`).
    pub content_margin: f64,
    /// `true` = use one value for both axes; `false` = each axis may differ
    /// (we keep a single value here for brevity and apply it to both).
    pub margin_same: bool,
    /// Bar kind — Solid reserves space in layout, Floating overlays content.
    pub kind: ScrollBarKind,
    /// Which colour role the bar uses.
    pub color: ScrollBarColor,
    /// Alpha of the fade-out region along the scroll-axis edges, 0..1.
    pub fade_strength: f64,
    /// Length of the fade region in pixels at each end.
    pub fade_size: f64,
}

impl ScrollBarStyle {
    /// Interpolated bar width for a hover-animation parameter `t` in `[0, 1]`.
    /// `t = 0` returns [`Self::floating_width`] (dormant); `t = 1` returns
    /// [`Self::bar_width`] (fully expanded).  Clamps `floating_width` so it
    /// never exceeds `bar_width`, regardless of what the caller set.
    ///
    /// [`ScrollBarKind::Solid`] bars do not animate width — they always
    /// render at `bar_width` so the "Full bar width" setting takes immediate
    /// visible effect.  Only [`ScrollBarKind::Floating`] bars expand on hover.
    pub fn bar_width_at(&self, t: f64) -> f64 {
        if self.kind == ScrollBarKind::Solid {
            return self.bar_width;
        }
        let from = self.floating_width.min(self.bar_width);
        let t = t.clamp(0.0, 1.0);
        from + (self.bar_width - from) * t
    }
}

impl Default for ScrollBarStyle {
    fn default() -> Self {
        Self {
            bar_width: 10.0,
            floating_width: 2.0,
            handle_min_length: 12.0,
            outer_margin: 0.0,
            inner_margin: 4.0,
            content_margin: 0.0,
            margin_same: true,
            kind: ScrollBarKind::default(),
            color: ScrollBarColor::Foreground,
            fade_strength: 0.5,
            fade_size: 20.0,
        }
    }
}

impl ScrollBarStyle {
    /// Preset matching egui's `ScrollStyle::solid` — always-visible bar, solid
    /// layout, fills reserved space.  Solid bars don't expand on hover so
    /// `floating_width` equals `bar_width`.
    pub fn solid() -> Self {
        Self {
            bar_width: 6.0,
            floating_width: 2.0,
            handle_min_length: 12.0,
            outer_margin: 0.0,
            inner_margin: 4.0,
            content_margin: 0.0,
            margin_same: true,
            kind: ScrollBarKind::Solid,
            color: ScrollBarColor::Background,
            fade_strength: 0.5,
            fade_size: 20.0,
        }
    }
    /// Preset matching egui's `ScrollStyle::thin` — a narrow floating bar
    /// that's always visible at its thin width and expands to full width when
    /// hovered.  Callers should pair this with
    /// [`ScrollBarVisibility::AlwaysVisible`] so the dormant thin bar is
    /// rendered even when the cursor isn't over it (the appearance panel's
    /// preset button does this).
    pub fn thin() -> Self {
        Self {
            bar_width: 10.0,
            floating_width: 2.0,
            handle_min_length: 12.0,
            outer_margin: 0.0,
            inner_margin: 4.0,
            content_margin: 0.0,
            margin_same: true,
            kind: ScrollBarKind::Floating,
            color: ScrollBarColor::Background,
            fade_strength: 0.5,
            fade_size: 20.0,
        }
    }
    /// Preset matching egui's `ScrollStyle::floating` — wide floating overlay
    /// with fade gradient at the edges.
    pub fn floating() -> Self {
        Self::default()
    }
}

// ── Global scroll style ─────────────────────────────────────────────────────
//
// Every `ScrollView` reads this value each layout unless the caller supplied
// an explicit `with_style(...)` or `with_style_cell(...)`.  The Appearance
// demo writes to this global so that "one slider affects every scroll bar in
// the application" — matching egui's `all_styles_mut` behaviour.

std::thread_local! {
    static CURRENT_SCROLL_STYLE:      Cell<ScrollBarStyle>      = Cell::new(ScrollBarStyle::default());
    static CURRENT_SCROLL_VISIBILITY: Cell<ScrollBarVisibility> = Cell::new(ScrollBarVisibility::VisibleWhenNeeded);
}

/// Read the current global scroll-bar style.
pub fn current_scroll_style() -> ScrollBarStyle {
    CURRENT_SCROLL_STYLE.with(|c| c.get())
}

/// Replace the global scroll-bar style.  All subsequent `ScrollView` layouts
/// that don't have an explicit override pick this up.
pub fn set_scroll_style(s: ScrollBarStyle) {
    CURRENT_SCROLL_STYLE.with(|c| c.set(s));
}

/// Read the current global scroll-bar visibility policy.
pub fn current_scroll_visibility() -> ScrollBarVisibility {
    CURRENT_SCROLL_VISIBILITY.with(|c| c.get())
}

/// Replace the global scroll-bar visibility policy.  Every `ScrollView` that
/// doesn't bind its own `with_bar_visibility_cell(...)` or call
/// `with_bar_visibility(...)` reads this value on each layout.
pub fn set_scroll_visibility(v: ScrollBarVisibility) {
    CURRENT_SCROLL_VISIBILITY.with(|c| c.set(v));
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Multiply the alpha channel of `c` by `a`.  Used to fade the track /
/// thumb during the hover fade-in / fade-out animation — the colour stays
/// its palette-defined hue and only transparency changes.
fn scale_alpha(c: Color, a: f64) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a * (a as f32).clamp(0.0, 1.0))
}

// ── Runtime constants ────────────────────────────────────────────────────────

/// Pixels at the right edge reserved for the parent window's resize grip.
const RIGHT_EDGE_GUARD: f64 = 4.0;
/// Pixels at the bottom edge reserved for the parent window's resize grip.
const BOTTOM_EDGE_GUARD: f64 = 4.0;
/// Extra hit-margin around the bar so it's easy to grab even when dormant.
const GRAB_MARGIN: f64 = 6.0;

// ── Per-axis state (vertical or horizontal) ──────────────────────────────────
//
// The vertical and horizontal scroll axes share the same computation — we
// factor the state so both reuse `clamp_offset` / `thumb_metrics` logic.

#[derive(Clone, Copy)]
struct AxisState {
    enabled: bool,
    offset: f64,
    content: f64,
    hovered_bar: bool,
    hovered_thumb: bool,
    dragging: bool,
    drag_thumb_offset: f64,
    hover_anim: crate::animation::Tween,
    /// Alpha tween for the fade-in / fade-out animation when a
    /// `Floating + VisibleWhenNeeded` bar appears on hover.  For every
    /// other visibility/kind combination the bar is painted at full
    /// opacity, so this tween stays at 1.0 and does nothing.
    visibility_anim: crate::animation::Tween,
}

impl Default for AxisState {
    fn default() -> Self {
        Self {
            enabled: false,
            offset: 0.0,
            content: 0.0,
            hovered_bar: false,
            hovered_thumb: false,
            dragging: false,
            drag_thumb_offset: 0.0,
            hover_anim: crate::animation::Tween::new(0.0, 0.12),
            visibility_anim: crate::animation::Tween::new(0.0, 0.18),
        }
    }
}

impl AxisState {
    fn max_scroll(&self, viewport: f64) -> f64 {
        (self.content - viewport).max(0.0)
    }

    /// Returns `true` when the bar is in the "expanded" interaction state.
    fn interact(&self) -> bool {
        self.hovered_bar || self.hovered_thumb || self.dragging
    }
}

pub struct ScrollView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always 0 or 1
    base: WidgetBase,

    v: AxisState,
    h: AxisState,

    /// Keep the scrollbar glued to the bottom as content grows (while the
    /// user hasn't scrolled away from the end).
    stick_to_bottom: bool,
    was_at_bottom: bool,

    /// How to render the scrollbar.
    bar_visibility: ScrollBarVisibility,
    /// `true` when the caller supplied an explicit per-instance visibility via
    /// [`ScrollView::with_bar_visibility`].  When `false` and
    /// `visibility_cell` is unset, the global visibility from
    /// [`current_scroll_visibility`] is re-read each layout.
    visibility_explicit: bool,
    style: ScrollBarStyle,
    /// `true` when the caller supplied an explicit per-instance style via
    /// [`ScrollView::with_style`].  When `false` and `style_cell` is unset,
    /// the global style from [`current_scroll_style`] is re-read each layout.
    style_explicit: bool,

    // ── External cell bindings ──
    offset_cell: Option<Rc<Cell<f64>>>,
    max_scroll_cell: Option<Rc<Cell<f64>>>,
    h_offset_cell: Option<Rc<Cell<f64>>>,
    h_max_scroll_cell: Option<Rc<Cell<f64>>>,
    visibility_cell: Option<Rc<Cell<ScrollBarVisibility>>>,
    style_cell: Option<Rc<Cell<ScrollBarStyle>>>,
    /// Visible viewport rect in content-space Y-up coordinates, written each
    /// layout.  Children doing virtual rendering read this cell.
    viewport_cell: Option<Rc<Cell<Rect>>>,

    middle_dragging: bool,
    middle_last_pos: Point,
}

impl ScrollView {
    pub fn new(content: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![content],
            base: WidgetBase::new(),
            v: AxisState {
                enabled: true,
                ..AxisState::default()
            },
            h: AxisState::default(),
            stick_to_bottom: false,
            was_at_bottom: false,
            bar_visibility: current_scroll_visibility(),
            visibility_explicit: false,
            style: current_scroll_style(),
            style_explicit: false,
            offset_cell: None,
            max_scroll_cell: None,
            h_offset_cell: None,
            h_max_scroll_cell: None,
            visibility_cell: None,
            style_cell: None,
            viewport_cell: None,
            middle_dragging: false,
            middle_last_pos: Point::ORIGIN,
        }
    }

    // ── Axis enable ───────────────────────────────────────────────────────────

    pub fn horizontal(mut self, enabled: bool) -> Self {
        self.h.enabled = enabled;
        self
    }
    pub fn vertical(mut self, enabled: bool) -> Self {
        self.v.enabled = enabled;
        self
    }

    // ── Scroll offset API (vertical for back-compat) ─────────────────────────

    pub fn scroll_offset(&self) -> f64 {
        self.v.offset
    }

    pub fn set_scroll_offset(&mut self, offset: f64) {
        self.v.offset = offset;
        if let Some(c) = &self.offset_cell {
            c.set(offset);
        }
    }

    pub fn max_scroll_value(&self) -> f64 {
        self.v.max_scroll(self.bounds.height)
    }

    pub fn with_offset_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.offset_cell = Some(cell);
        self
    }

    pub fn with_max_scroll_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.max_scroll_cell = Some(cell);
        self
    }

    pub fn with_h_offset_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.h_offset_cell = Some(cell);
        self
    }

    pub fn with_h_max_scroll_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.h_max_scroll_cell = Some(cell);
        self
    }

    pub fn with_stick_to_bottom(mut self, stick: bool) -> Self {
        self.stick_to_bottom = stick;
        self
    }

    pub fn with_bar_visibility(mut self, v: ScrollBarVisibility) -> Self {
        self.bar_visibility = v;
        self.visibility_explicit = true;
        self
    }

    pub fn set_bar_visibility(&mut self, v: ScrollBarVisibility) {
        self.bar_visibility = v;
        self.visibility_explicit = true;
    }

    pub fn with_bar_visibility_cell(mut self, cell: Rc<Cell<ScrollBarVisibility>>) -> Self {
        self.visibility_cell = Some(cell);
        self
    }

    pub fn with_style(mut self, s: ScrollBarStyle) -> Self {
        self.style = s;
        self.style_explicit = true;
        self
    }

    pub fn with_style_cell(mut self, cell: Rc<Cell<ScrollBarStyle>>) -> Self {
        self.style_cell = Some(cell);
        self
    }

    /// Bind a cell that receives the visible content-space viewport rect.
    pub fn with_viewport_cell(mut self, cell: Rc<Cell<Rect>>) -> Self {
        self.viewport_cell = Some(cell);
        self
    }

    // ── Geometry helpers ──────────────────────────────────────────────────────

    fn viewport(&self) -> (f64, f64) {
        // Viewport inside the widget AFTER reserving space for Solid bars.
        let (reserve_x, reserve_y) = self.bar_reserve();
        let w = (self.bounds.width - reserve_x).max(0.0);
        let h = (self.bounds.height - reserve_y).max(0.0);
        (w, h)
    }

    /// Horizontal/vertical space reserved for Solid scrollbars (0 for Floating).
    fn bar_reserve(&self) -> (f64, f64) {
        if self.style.kind != ScrollBarKind::Solid {
            return (0.0, 0.0);
        }
        let span = self.style.bar_width + self.style.outer_margin + self.style.inner_margin;
        let rx = if self.h.enabled && self.h.content > self.bounds.width {
            0.0
        } else {
            0.0
        };
        // We reserve vertical bar width on the right when vertical scrolling
        // is potentially active (has content overflow).
        let need_v = self.v.enabled && self.v.content > self.bounds.height - self.h_bar_thickness();
        let need_h = self.h.enabled && self.h.content > self.bounds.width - self.v_bar_thickness();
        let rx = rx + if need_v { span } else { 0.0 };
        let ry = if need_h { span } else { 0.0 };
        (rx, ry)
    }

    /// Just the bar width + margins (not conditional on overflow).  Used for
    /// hover-zone/paint placement when visibility says "AlwaysVisible".
    fn v_bar_thickness(&self) -> f64 {
        self.style.bar_width + self.style.outer_margin + self.style.inner_margin
    }
    fn h_bar_thickness(&self) -> f64 {
        self.style.bar_width + self.style.outer_margin + self.style.inner_margin
    }

    /// Right-edge X (exclusive) of the vertical scroll bar in local space.
    fn v_bar_right(&self) -> f64 {
        self.bounds.width - RIGHT_EDGE_GUARD - self.style.outer_margin
    }
    /// Bottom-edge Y (exclusive, Y-up) of the horizontal bar — i.e. the lower
    /// edge of the bar stripe, which in Y-up = `outer_margin + BOTTOM_EDGE_GUARD`.
    fn h_bar_bottom(&self) -> f64 {
        BOTTOM_EDGE_GUARD + self.style.outer_margin
    }

    /// Vertical track range [lo, hi] in Y-up.  Accounts for the horizontal bar
    /// reserving a sliver at the bottom when both axes scroll.
    fn v_track_range(&self) -> (f64, f64) {
        let (_, reserve_y) = self.bar_reserve();
        let lo = self.style.inner_margin + reserve_y;
        let hi = (self.bounds.height - self.style.inner_margin).max(lo);
        (lo, hi)
    }

    fn h_track_range(&self) -> (f64, f64) {
        let (reserve_x, _) = self.bar_reserve();
        let lo = self.style.inner_margin;
        let hi = (self.bounds.width - self.style.inner_margin - reserve_x).max(lo);
        (lo, hi)
    }

    /// Vertical thumb `(y_bottom, height)` in local Y-up or `None` if no overflow.
    fn v_thumb_metrics(&self) -> Option<(f64, f64)> {
        let (_, vh) = self.viewport();
        if self.v.content <= vh {
            return None;
        }
        let (lo, hi) = self.v_track_range();
        let track_h = hi - lo;
        let ratio = vh / self.v.content;
        let thumb_h = (track_h * ratio).max(self.style.handle_min_length);
        let travel = (track_h - thumb_h).max(0.0);
        let max_s = self.v.max_scroll(vh);
        let thumb_y = if max_s > 0.0 {
            lo + travel * (1.0 - self.v.offset / max_s)
        } else {
            lo + travel
        };
        Some((thumb_y, thumb_h))
    }

    /// Horizontal thumb `(x_left, width)` in local X.
    fn h_thumb_metrics(&self) -> Option<(f64, f64)> {
        let (vw, _) = self.viewport();
        if self.h.content <= vw {
            return None;
        }
        let (lo, hi) = self.h_track_range();
        let track_w = hi - lo;
        let ratio = vw / self.h.content;
        let thumb_w = (track_w * ratio).max(self.style.handle_min_length);
        let travel = (track_w - thumb_w).max(0.0);
        let max_s = self.h.max_scroll(vw);
        let thumb_x = if max_s > 0.0 {
            lo + travel * (self.h.offset / max_s)
        } else {
            lo
        };
        Some((thumb_x, thumb_w))
    }

    fn pos_on_v_thumb(&self, pos: Point) -> bool {
        let bar_right = self.v_bar_right();
        let bar_left = bar_right - self.style.bar_width;
        let hit_left = bar_left - GRAB_MARGIN;
        if pos.x < hit_left || pos.x >= bar_right {
            return false;
        }
        if let Some((ty, th)) = self.v_thumb_metrics() {
            pos.y >= ty && pos.y <= ty + th
        } else {
            false
        }
    }

    fn pos_on_h_thumb(&self, pos: Point) -> bool {
        let bar_bottom = self.h_bar_bottom();
        let bar_top = bar_bottom + self.style.bar_width;
        let hit_top = bar_top + GRAB_MARGIN;
        if pos.y < bar_bottom || pos.y >= hit_top {
            return false;
        }
        if let Some((tx, tw)) = self.h_thumb_metrics() {
            pos.x >= tx && pos.x <= tx + tw
        } else {
            false
        }
    }

    fn pos_in_v_hover(&self, pos: Point) -> bool {
        let bar_right = self.v_bar_right();
        let bar_left = bar_right - self.style.bar_width - GRAB_MARGIN;
        pos.x >= bar_left && pos.x < bar_right
    }

    fn pos_in_h_hover(&self, pos: Point) -> bool {
        let bar_bottom = self.h_bar_bottom();
        let bar_top = bar_bottom + self.style.bar_width + GRAB_MARGIN;
        pos.y >= bar_bottom && pos.y < bar_top
    }

    fn clamp_offsets(&mut self) {
        let (vw, vh) = self.viewport();
        self.v.offset = self.v.offset.clamp(0.0, self.v.max_scroll(vh)).round();
        self.h.offset = self.h.offset.clamp(0.0, self.h.max_scroll(vw)).round();
    }

    fn publish_offsets(&self) {
        if let Some(c) = &self.offset_cell {
            c.set(self.v.offset);
        }
        if let Some(c) = &self.h_offset_cell {
            c.set(self.h.offset);
        }
    }

    // ── Layout property forwarding ────────────────────────────────────────────

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

    // ── Visibility helper ────────────────────────────────────────────────────

    fn should_paint_v(&self) -> bool {
        let (_, vh) = self.viewport();
        if self.v.content <= vh {
            return false;
        }
        let floating = self.style.kind == ScrollBarKind::Floating;
        match self.bar_visibility {
            ScrollBarVisibility::AlwaysHidden => false,
            ScrollBarVisibility::AlwaysVisible => true,
            // With Floating kind, VisibleWhenNeeded hides until hover/drag —
            // matches egui's floating style.  Solid kind shows unconditionally
            // when content overflows.
            ScrollBarVisibility::VisibleWhenNeeded => {
                !floating || self.v.hovered_bar || self.v.dragging
            }
        }
    }

    fn should_paint_h(&self) -> bool {
        let (vw, _) = self.viewport();
        if self.h.content <= vw {
            return false;
        }
        let floating = self.style.kind == ScrollBarKind::Floating;
        match self.bar_visibility {
            ScrollBarVisibility::AlwaysHidden => false,
            ScrollBarVisibility::AlwaysVisible => true,
            ScrollBarVisibility::VisibleWhenNeeded => {
                !floating || self.h.hovered_bar || self.h.dragging
            }
        }
    }
}

mod widget_impl;
