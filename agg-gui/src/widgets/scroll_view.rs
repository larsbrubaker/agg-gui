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
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
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
    fn default() -> Self { Self::VisibleWhenNeeded }
}

/// Whether the bar reserves layout space (Solid) or floats over content (Floating).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollBarKind { Solid, Floating }

impl Default for ScrollBarKind {
    fn default() -> Self { Self::Floating }
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
    fn default() -> Self { Self::Background }
}

/// Full scrollbar appearance configuration — mirrors egui's `style.spacing.scroll`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBarStyle {
    /// Width of the full-size bar in pixels.  This is the bar width when the
    /// user is hovering or interacting with it.
    pub bar_width:         f64,
    /// Thin width shown when the bar is dormant (not hovered, not dragging).
    /// Matches egui's `floating_width`.  On hover the bar grows from this to
    /// [`Self::bar_width`].  Set equal to `bar_width` to disable the expand
    /// effect.  Only takes effect when smaller than `bar_width`.
    pub floating_width:    f64,
    /// Minimum length of the draggable thumb.
    pub handle_min_length: f64,
    /// Space between the bar and the panel's outer edge.
    pub outer_margin:      f64,
    /// Space between the bar and the content area.
    pub inner_margin:      f64,
    /// Space between sibling content and the bar area (applied when `kind = Solid`
    /// and as a decorative inset when `Floating`).
    pub content_margin:    f64,
    /// `true` = use one value for both axes; `false` = each axis may differ
    /// (we keep a single value here for brevity and apply it to both).
    pub margin_same:       bool,
    /// Bar kind — Solid reserves space in layout, Floating overlays content.
    pub kind:              ScrollBarKind,
    /// Which colour role the bar uses.
    pub color:             ScrollBarColor,
    /// Alpha of the fade-out region along the scroll-axis edges, 0..1.
    pub fade_strength:     f64,
    /// Length of the fade region in pixels at each end.
    pub fade_size:         f64,
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
        let t    = t.clamp(0.0, 1.0);
        from + (self.bar_width - from) * t
    }
}

impl Default for ScrollBarStyle {
    fn default() -> Self {
        Self {
            bar_width:         15.0,
            floating_width:    15.0,
            handle_min_length: 10.0,
            outer_margin:       5.0,
            inner_margin:       7.0,
            content_margin:     5.0,
            margin_same:        true,
            kind:               ScrollBarKind::default(),
            color:              ScrollBarColor::default(),
            fade_strength:      1.0,
            fade_size:         45.0,
        }
    }
}

impl ScrollBarStyle {
    /// Preset matching egui's `ScrollStyle::solid` — always-visible bar, solid
    /// layout, fills reserved space.  Solid bars don't expand on hover so
    /// `floating_width` equals `bar_width`.
    pub fn solid() -> Self {
        Self {
            bar_width:         8.0,
            floating_width:    8.0,
            handle_min_length: 12.0,
            outer_margin:      0.0,
            inner_margin:      4.0,
            content_margin:    0.0,
            margin_same:       true,
            kind:              ScrollBarKind::Solid,
            color:             ScrollBarColor::Foreground,
            fade_strength:     0.0,
            fade_size:         0.0,
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
            bar_width:         10.0,
            floating_width:    4.0,
            handle_min_length: 12.0,
            outer_margin:      2.0,
            inner_margin:      2.0,
            content_margin:    0.0,
            margin_same:       true,
            kind:              ScrollBarKind::Floating,
            color:             ScrollBarColor::Background,
            fade_strength:     0.0,
            fade_size:         0.0,
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
const RIGHT_EDGE_GUARD:  f64 = 4.0;
/// Pixels at the bottom edge reserved for the parent window's resize grip.
const BOTTOM_EDGE_GUARD: f64 = 4.0;
/// Extra hit-margin around the bar so it's easy to grab even when dormant.
const GRAB_MARGIN:       f64 = 6.0;

// ── Per-axis state (vertical or horizontal) ──────────────────────────────────
//
// The vertical and horizontal scroll axes share the same computation — we
// factor the state so both reuse `clamp_offset` / `thumb_metrics` logic.

#[derive(Clone, Copy)]
struct AxisState {
    enabled:     bool,
    offset:      f64,
    content:     f64,
    hovered_bar: bool,
    hovered_thumb: bool,
    dragging:    bool,
    drag_thumb_offset: f64,
    hover_anim:  crate::animation::Tween,
    /// Alpha tween for the fade-in / fade-out animation when a
    /// `Floating + VisibleWhenNeeded` bar appears on hover.  For every
    /// other visibility/kind combination the bar is painted at full
    /// opacity, so this tween stays at 1.0 and does nothing.
    visibility_anim: crate::animation::Tween,
}

impl Default for AxisState {
    fn default() -> Self {
        Self {
            enabled: false, offset: 0.0, content: 0.0,
            hovered_bar: false, hovered_thumb: false, dragging: false,
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
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,  // always 0 or 1
    base:     WidgetBase,

    v: AxisState,
    h: AxisState,

    /// Keep the scrollbar glued to the bottom as content grows (while the
    /// user hasn't scrolled away from the end).
    stick_to_bottom: bool,
    was_at_bottom:   bool,

    /// How to render the scrollbar.
    bar_visibility: ScrollBarVisibility,
    /// `true` when the caller supplied an explicit per-instance visibility via
    /// [`ScrollView::with_bar_visibility`].  When `false` and
    /// `visibility_cell` is unset, the global visibility from
    /// [`current_scroll_visibility`] is re-read each layout.
    visibility_explicit: bool,
    style:          ScrollBarStyle,
    /// `true` when the caller supplied an explicit per-instance style via
    /// [`ScrollView::with_style`].  When `false` and `style_cell` is unset,
    /// the global style from [`current_scroll_style`] is re-read each layout.
    style_explicit: bool,

    // ── External cell bindings ──
    offset_cell:      Option<Rc<Cell<f64>>>,
    max_scroll_cell:  Option<Rc<Cell<f64>>>,
    visibility_cell:  Option<Rc<Cell<ScrollBarVisibility>>>,
    style_cell:       Option<Rc<Cell<ScrollBarStyle>>>,
    /// Visible viewport rect in content-space Y-up coordinates, written each
    /// layout.  Children doing virtual rendering read this cell.
    viewport_cell:    Option<Rc<Cell<Rect>>>,
}

impl ScrollView {
    pub fn new(content: Box<dyn Widget>) -> Self {
        Self {
            bounds:            Rect::default(),
            children:          vec![content],
            base:              WidgetBase::new(),
            v:                 AxisState { enabled: true, ..AxisState::default() },
            h:                 AxisState::default(),
            stick_to_bottom:   false,
            was_at_bottom:     false,
            bar_visibility:    current_scroll_visibility(),
            visibility_explicit: false,
            style:             current_scroll_style(),
            style_explicit:    false,
            offset_cell:       None,
            max_scroll_cell:   None,
            visibility_cell:   None,
            style_cell:        None,
            viewport_cell:     None,
        }
    }

    // ── Axis enable ───────────────────────────────────────────────────────────

    pub fn horizontal(mut self, enabled: bool) -> Self {
        self.h.enabled = enabled; self
    }
    pub fn vertical(mut self, enabled: bool) -> Self {
        self.v.enabled = enabled; self
    }

    // ── Scroll offset API (vertical for back-compat) ─────────────────────────

    pub fn scroll_offset(&self) -> f64 { self.v.offset }

    pub fn set_scroll_offset(&mut self, offset: f64) {
        self.v.offset = offset;
        if let Some(c) = &self.offset_cell { c.set(offset); }
    }

    pub fn max_scroll_value(&self) -> f64 { self.v.max_scroll(self.bounds.height) }

    pub fn with_offset_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.offset_cell = Some(cell); self
    }

    pub fn with_max_scroll_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.max_scroll_cell = Some(cell); self
    }

    pub fn with_stick_to_bottom(mut self, stick: bool) -> Self {
        self.stick_to_bottom = stick; self
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
        self.visibility_cell = Some(cell); self
    }

    pub fn with_style(mut self, s: ScrollBarStyle) -> Self {
        self.style = s;
        self.style_explicit = true;
        self
    }

    pub fn with_style_cell(mut self, cell: Rc<Cell<ScrollBarStyle>>) -> Self {
        self.style_cell = Some(cell); self
    }

    /// Bind a cell that receives the visible content-space viewport rect.
    pub fn with_viewport_cell(mut self, cell: Rc<Cell<Rect>>) -> Self {
        self.viewport_cell = Some(cell); self
    }

    // ── Geometry helpers ──────────────────────────────────────────────────────

    fn viewport(&self) -> (f64, f64) {
        // Viewport inside the widget AFTER reserving space for Solid bars.
        let (reserve_x, reserve_y) = self.bar_reserve();
        let w = (self.bounds.width  - reserve_x).max(0.0);
        let h = (self.bounds.height - reserve_y).max(0.0);
        (w, h)
    }

    /// Horizontal/vertical space reserved for Solid scrollbars (0 for Floating).
    fn bar_reserve(&self) -> (f64, f64) {
        if self.style.kind != ScrollBarKind::Solid {
            return (0.0, 0.0);
        }
        let span = self.style.bar_width
            + self.style.outer_margin
            + self.style.inner_margin;
        let rx = if self.h.enabled && self.h.content > self.bounds.width  { 0.0 } else { 0.0 };
        // We reserve vertical bar width on the right when vertical scrolling
        // is potentially active (has content overflow).
        let need_v = self.v.enabled && self.v.content > self.bounds.height - self.h_bar_thickness();
        let need_h = self.h.enabled && self.h.content > self.bounds.width  - self.v_bar_thickness();
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
        if self.v.content <= vh { return None; }
        let (lo, hi) = self.v_track_range();
        let track_h  = hi - lo;
        let ratio    = vh / self.v.content;
        let thumb_h  = (track_h * ratio).max(self.style.handle_min_length);
        let travel   = (track_h - thumb_h).max(0.0);
        let max_s    = self.v.max_scroll(vh);
        let thumb_y  = if max_s > 0.0 {
            lo + travel * (1.0 - self.v.offset / max_s)
        } else { lo + travel };
        Some((thumb_y, thumb_h))
    }

    /// Horizontal thumb `(x_left, width)` in local X.
    fn h_thumb_metrics(&self) -> Option<(f64, f64)> {
        let (vw, _) = self.viewport();
        if self.h.content <= vw { return None; }
        let (lo, hi) = self.h_track_range();
        let track_w  = hi - lo;
        let ratio    = vw / self.h.content;
        let thumb_w  = (track_w * ratio).max(self.style.handle_min_length);
        let travel   = (track_w - thumb_w).max(0.0);
        let max_s    = self.h.max_scroll(vw);
        let thumb_x  = if max_s > 0.0 {
            lo + travel * (self.h.offset / max_s)
        } else { lo };
        Some((thumb_x, thumb_w))
    }

    fn pos_on_v_thumb(&self, pos: Point) -> bool {
        let bar_right = self.v_bar_right();
        let bar_left  = bar_right - self.style.bar_width;
        let hit_left  = bar_left - GRAB_MARGIN;
        if pos.x < hit_left || pos.x >= bar_right { return false; }
        if let Some((ty, th)) = self.v_thumb_metrics() {
            pos.y >= ty && pos.y <= ty + th
        } else { false }
    }

    fn pos_on_h_thumb(&self, pos: Point) -> bool {
        let bar_bottom = self.h_bar_bottom();
        let bar_top    = bar_bottom + self.style.bar_width;
        let hit_top    = bar_top + GRAB_MARGIN;
        if pos.y < bar_bottom || pos.y >= hit_top { return false; }
        if let Some((tx, tw)) = self.h_thumb_metrics() {
            pos.x >= tx && pos.x <= tx + tw
        } else { false }
    }

    fn pos_in_v_hover(&self, pos: Point) -> bool {
        let bar_right = self.v_bar_right();
        let bar_left  = bar_right - self.style.bar_width - GRAB_MARGIN;
        pos.x >= bar_left && pos.x < bar_right
    }

    fn pos_in_h_hover(&self, pos: Point) -> bool {
        let bar_bottom = self.h_bar_bottom();
        let bar_top    = bar_bottom + self.style.bar_width + GRAB_MARGIN;
        pos.y >= bar_bottom && pos.y < bar_top
    }

    fn clamp_offsets(&mut self) {
        let (vw, vh) = self.viewport();
        self.v.offset = self.v.offset.clamp(0.0, self.v.max_scroll(vh)).round();
        self.h.offset = self.h.offset.clamp(0.0, self.h.max_scroll(vw)).round();
    }

    // ── Layout property forwarding ────────────────────────────────────────────

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    // ── Visibility helper ────────────────────────────────────────────────────

    fn should_paint_v(&self) -> bool {
        let (_, vh) = self.viewport();
        if self.v.content <= vh { return false; }
        let floating = self.style.kind == ScrollBarKind::Floating;
        match self.bar_visibility {
            ScrollBarVisibility::AlwaysHidden      => false,
            ScrollBarVisibility::AlwaysVisible     => true,
            // With Floating kind, VisibleWhenNeeded hides until hover/drag —
            // matches egui's floating style.  Solid kind shows unconditionally
            // when content overflows.
            ScrollBarVisibility::VisibleWhenNeeded =>
                !floating || self.v.hovered_bar || self.v.dragging,
        }
    }

    fn should_paint_h(&self) -> bool {
        let (vw, _) = self.viewport();
        if self.h.content <= vw { return false; }
        let floating = self.style.kind == ScrollBarKind::Floating;
        match self.bar_visibility {
            ScrollBarVisibility::AlwaysHidden      => false,
            ScrollBarVisibility::AlwaysVisible     => true,
            ScrollBarVisibility::VisibleWhenNeeded =>
                !floating || self.h.hovered_bar || self.h.dragging,
        }
    }
}

impl Widget for ScrollView {
    fn type_name(&self) -> &'static str { "ScrollView" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn hit_test(&self, local_pos: Point) -> bool {
        if self.v.dragging || self.h.dragging { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    fn claims_pointer_exclusively(&self, local_pos: Point) -> bool {
        if self.v.dragging || self.h.dragging { return true; }
        let (vw, vh) = self.viewport();
        if self.v.enabled && self.v.content > vh && self.pos_in_v_hover(local_pos) { return true; }
        if self.h.enabled && self.h.content > vw && self.pos_in_h_hover(local_pos) { return true; }
        false
    }

    fn layout(&mut self, available: Size) -> Size {
        // Pull live state from external cells first.
        if let Some(c) = &self.offset_cell     { self.v.offset = c.get(); }
        if let Some(c) = &self.visibility_cell {
            self.bar_visibility = c.get();
        } else if !self.visibility_explicit {
            self.bar_visibility = current_scroll_visibility();
        }
        if let Some(c) = &self.style_cell {
            self.style = c.get();
        } else if !self.style_explicit {
            // No explicit override → follow the global scroll-bar style so
            // the Appearance demo restyles every `ScrollView` in the app.
            self.style = current_scroll_style();
        }

        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);

        // For horizontal scrolling, content width is unconstrained (the child
        // may return a width larger than our viewport).  For vertical-only, we
        // pin child to the viewport width so wrapping widgets behave.
        let (vw_guess, _vh_guess) = self.viewport();
        let child_in_w = if self.h.enabled { f64::MAX / 2.0 } else { vw_guess };
        let child_in_h = f64::MAX / 2.0;

        if let Some(child) = self.children.first_mut() {
            let natural = child.layout(Size::new(child_in_w, child_in_h));
            self.v.content = natural.height;
            self.h.content = if self.h.enabled { natural.width } else { vw_guess };
        }

        // Re-query viewport now that content dimensions are known (Solid bars
        // may reserve different space once we know overflow).
        let (vw, vh) = self.viewport();

        if self.stick_to_bottom && self.was_at_bottom {
            self.v.offset = self.v.max_scroll(vh);
        }
        self.clamp_offsets();
        self.was_at_bottom = (self.v.max_scroll(vh) - self.v.offset).abs() < 0.5;

        // Publish offsets / max / viewport.
        if let Some(c) = &self.offset_cell     { c.set(self.v.offset); }
        if let Some(c) = &self.max_scroll_cell { c.set(self.v.max_scroll(vh)); }
        if let Some(c) = &self.viewport_cell {
            // Content-space viewport rect in Y-UP content coords:
            //   x = h_offset  (left edge of visible region)
            //   y = (v_content_height - vh - v_offset) if inverting, but we
            //       expose TOP-DOWN coords for easier row math: y = v_offset.
            // We output a rect where (x, y) is the TOP-LEFT of visible content
            // in a conventional top-down space, and (width, height) = viewport.
            c.set(Rect::new(self.h.offset, self.v.offset, vw, vh));
        }

        // Position child inside the widget.
        if let Some(child) = self.children.first_mut() {
            let child_y = vh - self.v.content + self.v.offset;
            let child_x = -self.h.offset;
            child.set_bounds(Rect::new(
                child_x.round(), child_y.round(),
                if self.h.enabled { self.h.content } else { vw },
                self.v.content,
            ));
        }

        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        // Clip children to the VIEWPORT so the content never overpaints the
        // scrollbar gutter or the edge guards.
        let (vw, vh) = self.viewport();
        Some((0.0, self.bounds.height - vh, vw, vh))
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();

        // Drive the fade-in / fade-out alpha animation.  `should_paint_*`
        // returns true exactly when the bar would be shown in the old
        // pop-in behaviour; the tween now smooths that transition so a
        // `Floating + VisibleWhenNeeded` bar dissolves in instead of
        // snapping.  For non-animating combinations the target stays
        // pinned at its terminal value, so the tween is a no-op.
        self.v.visibility_anim.set_target(if self.should_paint_v() { 1.0 } else { 0.0 });
        self.h.visibility_anim.set_target(if self.should_paint_h() { 1.0 } else { 0.0 });
        let v_alpha = self.v.visibility_anim.tick();
        let h_alpha = self.h.visibility_anim.tick();

        // Paint whenever alpha is visible — including the tail of a
        // fade-out after the cursor leaves, so the bar smoothly dissolves
        // instead of vanishing.
        let paint_v = self.v.enabled && self.v.content > self.viewport().1 && v_alpha > 0.001;
        let paint_h = self.h.enabled && self.h.content > self.viewport().0 && h_alpha > 0.001;

        let track_color_base = match self.style.color {
            ScrollBarColor::Background => v.scroll_track,
            ScrollBarColor::Foreground => Color::rgba(
                v.accent.r, v.accent.g, v.accent.b, 0.08),
        };
        let thumb_idle = match self.style.color {
            ScrollBarColor::Background => v.scroll_thumb,
            ScrollBarColor::Foreground => v.accent,
        };

        // ── Vertical bar ──
        if paint_v {
            if let Some((ty, th)) = self.v_thumb_metrics() {
                let bar_right = self.v_bar_right();
                self.v.hover_anim.set_target(if self.v.interact() { 1.0 } else { 0.0 });
                let t         = self.v.hover_anim.tick();
                let bar_w     = self.style.bar_width_at(t);
                let bar_x     = bar_right - bar_w;
                let r         = bar_w * 0.5;

                let (lo, hi) = self.v_track_range();
                ctx.set_fill_color(scale_alpha(track_color_base, v_alpha));
                ctx.begin_path();
                ctx.rounded_rect(bar_x, lo, bar_w, hi - lo, r);
                ctx.fill();

                let tc = if self.v.dragging {
                    v.scroll_thumb_dragging
                } else if self.v.hovered_thumb {
                    v.scroll_thumb_hovered
                } else { thumb_idle };
                ctx.set_fill_color(scale_alpha(tc, v_alpha));
                ctx.begin_path();
                ctx.rounded_rect(bar_x, ty, bar_w, th, r);
                ctx.fill();
            }
        }

        // ── Horizontal bar ──
        if paint_h {
            if let Some((tx, tw)) = self.h_thumb_metrics() {
                let bar_bottom = self.h_bar_bottom();
                self.h.hover_anim.set_target(if self.h.interact() { 1.0 } else { 0.0 });
                let t          = self.h.hover_anim.tick();
                let bar_h      = self.style.bar_width_at(t);
                let r          = bar_h * 0.5;

                let (lo, hi) = self.h_track_range();
                ctx.set_fill_color(scale_alpha(track_color_base, h_alpha));
                ctx.begin_path();
                ctx.rounded_rect(lo, bar_bottom, hi - lo, bar_h, r);
                ctx.fill();

                let tc = if self.h.dragging {
                    v.scroll_thumb_dragging
                } else if self.h.hovered_thumb {
                    v.scroll_thumb_hovered
                } else { thumb_idle };
                ctx.set_fill_color(scale_alpha(tc, h_alpha));
                ctx.begin_path();
                ctx.rounded_rect(tx, bar_bottom, tw, bar_h, r);
                ctx.fill();
            }
        }

        // ── Fade gradient overlay at the scroll-axis edges ──
        //
        // Approximation of egui's fade: draw a translucent stripe of the
        // panel_fill colour at each edge where content is clipped.  Strength
        // * 1.0 = fully opaque at the edge.
        if self.style.fade_strength > 0.001 && self.style.fade_size > 0.5 {
            self.paint_fade(ctx);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            // ── Mouse wheel ───────────────────────────────────────────────────
            Event::MouseWheel { delta_y, delta_x, .. } => {
                let mut consumed = false;
                if self.v.enabled {
                    self.v.offset = self.v.offset + delta_y * 40.0;
                    consumed = true;
                }
                if self.h.enabled {
                    self.h.offset = self.h.offset + delta_x * 40.0;
                    consumed = true;
                }
                self.clamp_offsets();
                let (_, vh) = self.viewport();
                self.was_at_bottom = (self.v.max_scroll(vh) - self.v.offset).abs() < 0.5;
                if let Some(c) = &self.offset_cell { c.set(self.v.offset); }
                if consumed {
                    crate::animation::request_tick();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            // ── Mouse move ────────────────────────────────────────────────────
            Event::MouseMove { pos } => {
                let (vw, vh) = self.viewport();
                let v_scroll = self.v.enabled && self.v.content > vh;
                let h_scroll = self.h.enabled && self.h.content > vw;
                let was_vb = self.v.hovered_bar;
                let was_vt = self.v.hovered_thumb;
                let was_hb = self.h.hovered_bar;
                let was_ht = self.h.hovered_thumb;
                self.v.hovered_bar   = v_scroll && self.pos_in_v_hover(*pos);
                self.v.hovered_thumb = v_scroll && self.pos_on_v_thumb(*pos);
                self.h.hovered_bar   = h_scroll && self.pos_in_h_hover(*pos);
                self.h.hovered_thumb = h_scroll && self.pos_on_h_thumb(*pos);
                if was_vb != self.v.hovered_bar || was_vt != self.v.hovered_thumb
                    || was_hb != self.h.hovered_bar || was_ht != self.h.hovered_thumb
                {
                    crate::animation::request_tick();
                }

                if self.v.dragging {
                    if let Some((_, th)) = self.v_thumb_metrics() {
                        let (lo, hi) = self.v_track_range();
                        let travel = (hi - lo - th).max(1.0);
                        let new_ty = (pos.y - self.v.drag_thumb_offset)
                            .clamp(lo, lo + travel);
                        let frac = 1.0 - (new_ty - lo) / travel;
                        self.v.offset = (frac * self.v.max_scroll(vh)).max(0.0);
                        self.clamp_offsets();
                        self.was_at_bottom =
                            (self.v.max_scroll(vh) - self.v.offset).abs() < 0.5;
                        if let Some(c) = &self.offset_cell { c.set(self.v.offset); }
                    }
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }
                if self.h.dragging {
                    if let Some((_, tw)) = self.h_thumb_metrics() {
                        let (lo, hi) = self.h_track_range();
                        let travel = (hi - lo - tw).max(1.0);
                        let new_tx = (pos.x - self.h.drag_thumb_offset)
                            .clamp(lo, lo + travel);
                        let frac = (new_tx - lo) / travel;
                        self.h.offset = (frac * self.h.max_scroll(vw)).max(0.0);
                        self.clamp_offsets();
                    }
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            // ── Mouse down ────────────────────────────────────────────────────
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                let (vw, vh) = self.viewport();
                let v_scroll = self.v.enabled && self.v.content > vh;
                let h_scroll = self.h.enabled && self.h.content > vw;

                if v_scroll && self.pos_in_v_hover(*pos) {
                    if self.pos_on_v_thumb(*pos) {
                        let ty = self.v_thumb_metrics().map(|(y, _)| y).unwrap_or(0.0);
                        self.v.dragging = true;
                        self.v.drag_thumb_offset = pos.y - ty;
                        // No tick: thumb grab has no visible effect until
                        // the cursor actually moves.
                    } else if let Some((ty, th)) = self.v_thumb_metrics() {
                        // Page step on track click (matches Windows / macOS).
                        // Y-up: cursor ABOVE thumb (higher y) → scroll UP,
                        // cursor BELOW thumb → scroll DOWN.  Step by one
                        // viewport minus a small overlap for continuity.
                        let page = (vh - 16.0).max(20.0);
                        if pos.y > ty + th {
                            self.v.offset = (self.v.offset - page).max(0.0);
                        } else if pos.y < ty {
                            self.v.offset = (self.v.offset + page).min(self.v.max_scroll(vh));
                        }
                        self.clamp_offsets();
                        if let Some(c) = &self.offset_cell { c.set(self.v.offset); }
                        // Offset changed — visible scroll.
                        crate::animation::request_tick();
                    }
                    return EventResult::Consumed;
                }
                if h_scroll && self.pos_in_h_hover(*pos) {
                    if self.pos_on_h_thumb(*pos) {
                        let tx = self.h_thumb_metrics().map(|(x, _)| x).unwrap_or(0.0);
                        self.h.dragging = true;
                        self.h.drag_thumb_offset = pos.x - tx;
                        // No tick — see v-axis thumb grab comment above.
                    } else if let Some((tx, tw)) = self.h_thumb_metrics() {
                        let page = (vw - 16.0).max(20.0);
                        if pos.x < tx {
                            self.h.offset = (self.h.offset - page).max(0.0);
                        } else if pos.x > tx + tw {
                            self.h.offset = (self.h.offset + page).min(self.h.max_scroll(vw));
                        }
                        self.clamp_offsets();
                        crate::animation::request_tick();
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            // ── Mouse up ──────────────────────────────────────────────────────
            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was = self.v.dragging || self.h.dragging;
                self.v.dragging = false;
                self.h.dragging = false;
                if was {
                    crate::animation::request_tick();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            _ => EventResult::Ignored,
        }
    }

    /// Surface the per-axis offsets and the maximum scroll distance as
    /// inspector / test properties.  Tests use these to verify that a
    /// shrunken viewport actually exposes scrollable overflow.
    fn properties(&self) -> Vec<(&'static str, String)> {
        let (vw, vh) = self.viewport();
        vec![
            ("v_enabled",  self.v.enabled.to_string()),
            ("h_enabled",  self.h.enabled.to_string()),
            ("v_offset",   format!("{:.1}", self.v.offset)),
            ("h_offset",   format!("{:.1}", self.h.offset)),
            ("max_scroll", format!("{:.1}", self.v.max_scroll(vh))),
            ("h_max_scroll", format!("{:.1}", self.h.max_scroll(vw))),
            ("v_content",  format!("{:.1}", self.v.content)),
            ("h_content",  format!("{:.1}", self.h.content)),
        ]
    }
}

impl ScrollView {
    /// Paint a gradient fade at the scroll-axis edges using thin horizontal or
    /// vertical strips with linearly interpolated alpha.  The strip closest to
    /// the clip edge is fully opaque; the strip furthest inside the viewport is
    /// fully transparent — giving a smooth dissolve into the background colour.
    fn paint_fade(&self, ctx: &mut dyn DrawCtx) {
        let v        = ctx.visuals();
        let c        = v.panel_fill;
        let (vw, vh) = self.viewport();
        let strength = self.style.fade_strength.clamp(0.0, 1.0) as f32;
        let size     = self.style.fade_size.max(0.0);
        let max_a    = strength;

        // Fade appears only near edges where content is clipped.
        if self.v.enabled {
            if self.v.offset > 0.5 {
                // Top edge (Y-up: high Y).  Gradient transparent→opaque going up.
                Self::fill_v_gradient(ctx, c, max_a, 0.0, self.bounds.height - size, vw, size, false);
            }
            if (self.v.max_scroll(vh) - self.v.offset) > 0.5 {
                // Bottom edge.  Gradient transparent→opaque going down.
                let y_bottom = self.bounds.height - vh;
                Self::fill_v_gradient(ctx, c, max_a, 0.0, y_bottom, vw, size, true);
            }
        }
        if self.h.enabled {
            if self.h.offset > 0.5 {
                // Left edge.  Gradient transparent→opaque going left.
                Self::fill_h_gradient(ctx, c, max_a, 0.0, self.bounds.height - vh, size, vh, true);
            }
            if (self.h.max_scroll(vw) - self.h.offset) > 0.5 {
                // Right edge.  Gradient transparent→opaque going right.
                Self::fill_h_gradient(ctx, c, max_a, vw - size, self.bounds.height - vh, size, vh, false);
            }
        }
    }

    /// Draw a vertical gradient rect using `STEPS` thin strips.
    ///
    /// When `opaque_at_bottom` is `true` the gradient runs opaque→transparent
    /// bottom-to-top (bottom edge fade); when `false` it runs
    /// transparent→opaque bottom-to-top (top edge fade).
    fn fill_v_gradient(
        ctx:             &mut dyn DrawCtx,
        c:               Color,
        max_alpha:       f32,
        x:               f64,
        y:               f64,
        w:               f64,
        h:               f64,
        opaque_at_bottom: bool,
    ) {
        const STEPS: usize = 64;
        let strip_h = h / STEPS as f64;
        for i in 0..STEPS {
            // t = 0 at the transparent end, 1 at the opaque end.
            let t = (i as f32 + 0.5) / STEPS as f32;
            let a = if opaque_at_bottom { 1.0 - t } else { t };
            ctx.set_fill_color(Color::rgba(c.r, c.g, c.b, a * max_alpha));
            ctx.begin_path();
            ctx.rect(x, y + i as f64 * strip_h, w, strip_h + 0.5);
            ctx.fill();
        }
    }

    /// Draw a horizontal gradient rect using `STEPS` thin strips.
    ///
    /// When `opaque_at_left` is `true` the gradient runs opaque→transparent
    /// left-to-right (left edge fade); when `false` it runs
    /// transparent→opaque left-to-right (right edge fade).
    fn fill_h_gradient(
        ctx:           &mut dyn DrawCtx,
        c:             Color,
        max_alpha:     f32,
        x:             f64,
        y:             f64,
        w:             f64,
        h:             f64,
        opaque_at_left: bool,
    ) {
        const STEPS: usize = 64;
        let strip_w = w / STEPS as f64;
        for i in 0..STEPS {
            let t = (i as f32 + 0.5) / STEPS as f32;
            let a = if opaque_at_left { 1.0 - t } else { t };
            ctx.set_fill_color(Color::rgba(c.r, c.g, c.b, a * max_alpha));
            ctx.begin_path();
            ctx.rect(x + i as f64 * strip_w, y, strip_w + 0.5, h);
            ctx.fill();
        }
    }
}
