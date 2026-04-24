//! Flex layout widgets: `FlexColumn` (vertical) and `FlexRow` (horizontal).
//!
//! # Y-up layout convention
//!
//! `FlexColumn` stacks children **top to bottom** visually, which in Y-up
//! coordinates means the *first* child gets the *highest* Y values. The layout
//! cursor starts at the top of the available area and moves downward.
//!
//! `FlexRow` stacks children **left to right**, as expected.
//!
//! # Flex algorithm
//!
//! Each child has a `flex` factor (stored in a parallel `Vec<f64>`):
//! - `flex = 0.0` → "fixed": the child is laid out at its natural size on
//!   the main axis.
//! - `flex > 0.0` → "growing": the child receives a proportional share of
//!   the remaining space after all fixed children are measured.
//!
//! Children with equal `flex` values split remaining space equally.
//!
//! # Child margin support
//!
//! Each child's `margin()` (scaled by `device_scale`) contributes to the slot
//! size on the main axis and is respected for cross-axis placement.
//! Margins are **additive** — child A's `margin.top` and child B's
//! `margin.bottom` both contribute gap space between those children (in
//! addition to `self.gap`).
//!
//! # Cross-axis anchoring
//!
//! `FlexColumn` reads each child's `h_anchor()` to place it horizontally
//! within the column's inner width.  `FlexRow` reads `v_anchor()` to place
//! children vertically within the row's inner height.

use crate::color::Color;
use crate::device_scale::device_scale;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase, resolve_fit_or_stretch};
use crate::widget::Widget;

// ---------------------------------------------------------------------------
// Cross-axis placement helpers
// ---------------------------------------------------------------------------

/// Compute `(x, actual_width)` for a child in a `FlexColumn` (horizontal
/// cross-axis placement).
///
/// - `pad_l`     — column's left inner-padding offset.
/// - `inner_w`   — column's usable width (after padding, before margins).
/// - `margin_l/r` — child's scaled left/right margins.
/// - `natural_w` — width returned by `child.layout()`.
/// - `min_w/max_w` — child's min/max width constraints.
fn place_cross_h(
    anchor: HAnchor,
    pad_l:    f64,
    inner_w:  f64,
    margin_l: f64,
    margin_r: f64,
    natural_w: f64,
    min_w: f64,
    max_w: f64,
) -> (f64, f64) {
    let slot_w = (inner_w - margin_l - margin_r).max(0.0);

    // Determine width.
    let actual_w = if anchor.is_stretch() {
        // LEFT | RIGHT → fill slot
        slot_w.clamp(min_w, max_w)
    } else if anchor == HAnchor::MAX_FIT_OR_STRETCH {
        resolve_fit_or_stretch(natural_w, slot_w, true).clamp(min_w, max_w)
    } else if anchor == HAnchor::MIN_FIT_OR_STRETCH {
        resolve_fit_or_stretch(natural_w, slot_w, false).clamp(min_w, max_w)
    } else {
        // FIT, LEFT, RIGHT, CENTER, ABSOLUTE — use natural width.
        natural_w.clamp(min_w, max_w)
    };

    // Determine x position.
    let x = if anchor.contains(HAnchor::RIGHT) && !anchor.contains(HAnchor::LEFT) {
        // RIGHT only (not stretch): right-align within margin slot.
        (pad_l + inner_w - margin_r - actual_w).max(pad_l)
    } else if anchor.contains(HAnchor::CENTER) && !anchor.is_stretch() {
        // CENTER: center within margin slot.
        pad_l + margin_l + (slot_w - actual_w) * 0.5
    } else {
        // LEFT, STRETCH, FIT, ABSOLUTE, MIN/MAX_FIT_OR_STRETCH — left-align.
        pad_l + margin_l
    };

    (x, actual_w)
}

/// Compute `(y, actual_height)` for a child in a `FlexRow` (vertical
/// cross-axis placement, Y-up).
///
/// - `pad_b`     — row's bottom inner-padding offset.
/// - `inner_h`   — row's usable height (after padding, before margins).
/// - `margin_b/t` — child's scaled bottom/top margins.
/// - `natural_h` — height returned by `child.layout()`.
/// - `min_h/max_h` — child's min/max height constraints.
fn place_cross_v(
    anchor: VAnchor,
    pad_b:    f64,
    inner_h:  f64,
    margin_b: f64,
    margin_t: f64,
    natural_h: f64,
    min_h: f64,
    max_h: f64,
) -> (f64, f64) {
    let slot_h = (inner_h - margin_b - margin_t).max(0.0);

    // Determine height.
    let actual_h = if anchor.is_stretch() {
        slot_h.clamp(min_h, max_h)
    } else if anchor == VAnchor::MAX_FIT_OR_STRETCH {
        resolve_fit_or_stretch(natural_h, slot_h, true).clamp(min_h, max_h)
    } else if anchor == VAnchor::MIN_FIT_OR_STRETCH {
        resolve_fit_or_stretch(natural_h, slot_h, false).clamp(min_h, max_h)
    } else {
        natural_h.clamp(min_h, max_h)
    };

    // Determine y position (Y-up: BOTTOM = low Y, TOP = high Y).
    let y = if anchor.contains(VAnchor::TOP) && !anchor.contains(VAnchor::BOTTOM) {
        // TOP only: top-align in slot.
        (pad_b + inner_h - margin_t - actual_h).max(pad_b)
    } else if anchor.contains(VAnchor::CENTER) && !anchor.is_stretch() {
        // CENTER: center within margin slot.
        pad_b + margin_b + (slot_h - actual_h) * 0.5
    } else {
        // BOTTOM, STRETCH, FIT, ABSOLUTE — bottom-align.
        pad_b + margin_b
    };

    (y, actual_h)
}

// ---------------------------------------------------------------------------
// FlexColumn
// ---------------------------------------------------------------------------

/// Stacks children top-to-bottom (first child = visually topmost).
pub struct FlexColumn {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Parallel to `children`. 0.0 = fixed; >0 = flex fraction.
    flex_factors: Vec<f64>,
    base: WidgetBase,
    pub gap: f64,
    pub inner_padding: Insets,
    pub background: Color,
    /// When `true`, paint background using `ctx.visuals().panel_fill`
    /// regardless of the stored `background` colour.
    pub use_panel_bg: bool,
    /// When `true`, `layout` reports the column's natural content
    /// width (max over children, + horizontal padding) instead of the
    /// full `available.width`.  Used by auto-sized ancestors that
    /// want the column to shrink-to-content rather than stretch.
    /// Off by default for backward compatibility.
    pub fit_width: bool,
    /// When `true`, children are anchored to the TOP of the column's
    /// inner area, with any extra height appearing as whitespace at
    /// the BOTTOM.  Off by default — legacy callers (e.g. ScrollView
    /// content) rely on the natural-anchored layout where children
    /// occupy the BOTTOM of their slot when oversized.
    pub top_anchor: bool,
}

impl FlexColumn {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            flex_factors: Vec::new(),
            base: WidgetBase::new(),
            gap: 0.0,
            inner_padding: Insets::ZERO,
            background: Color::rgba(0.0, 0.0, 0.0, 0.0),
            use_panel_bg: false,
            fit_width: false,
            top_anchor: false,
        }
    }

    pub fn with_gap(mut self, gap: f64) -> Self { self.gap = gap; self }
    pub fn with_padding(mut self, p: f64) -> Self { self.inner_padding = Insets::all(p); self }
    pub fn with_inner_padding(mut self, p: Insets) -> Self { self.inner_padding = p; self }
    pub fn with_background(mut self, c: Color) -> Self { self.background = c; self }
    /// Use `ctx.visuals().panel_fill` as background instead of the stored color.
    pub fn with_panel_bg(mut self) -> Self { self.use_panel_bg = true; self }

    /// Opt into content-fit width — `layout` reports the widest
    /// child's natural width (+ horizontal padding) instead of the
    /// full available width.  Required when this column is the
    /// content of an auto-sized `Window`; without it, wrapped Labels
    /// claim the full available width and the window grows to the
    /// canvas.  Matches egui's per-column shrink-to-content option.
    pub fn with_fit_width(mut self, fit: bool) -> Self { self.fit_width = fit; self }

    /// Anchor children to the TOP of the inner area rather than the
    /// bottom of the natural content extent.  Default is bottom (the
    /// classic Y-up "natural-anchored" placement) so callers like
    /// `ScrollView` whose layout pass uses `available.height ≈ ∞`
    /// keep working — they need cursor_y to be derived from natural
    /// extent, not from the supplied (huge) available.  Opt in for
    /// containers placed inside a `Resize` widget or other oversized
    /// slot where you want the visible content to start at the top
    /// of the frame and any extra space to appear below.
    pub fn with_top_anchor(mut self, on: bool) -> Self { self.top_anchor = on; self }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    /// Add a fixed-size child (flex = 0).
    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self.flex_factors.push(0.0);
        self
    }

    /// Add a flex child that expands proportionally.
    pub fn add_flex(mut self, child: Box<dyn Widget>, flex: f64) -> Self {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
        self
    }

    /// Push a child directly (for use without builder chaining).
    pub fn push(&mut self, child: Box<dyn Widget>, flex: f64) {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
    }
}

impl Default for FlexColumn { fn default() -> Self { Self::new() } }

impl Widget for FlexColumn {
    fn type_name(&self) -> &'static str { "FlexColumn" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn measure_min_height(&self, available_w: f64) -> f64 {
        // Sum each child's required height (recursing through any
        // FlexColumn / TextArea / Container chains) plus our own
        // padding and inter-child gaps.  Used by ancestor
        // `Window::tight_content_fit` to compute a content-bound
        // height even when one of our children is a flex-fill widget
        // whose `layout` would just return the available slot.
        let pad_l = self.inner_padding.left;
        let pad_r = self.inner_padding.right;
        let pad_t = self.inner_padding.top;
        let pad_b = self.inner_padding.bottom;
        let inner_w = (available_w - pad_l - pad_r).max(0.0);
        let scale   = device_scale();
        let n       = self.children.len();
        let mut total = 0.0_f64;
        for child in self.children.iter() {
            let m = child.margin().scale(scale);
            let slot_w = (inner_w - m.left - m.right).max(0.0);
            total += child.measure_min_height(slot_w) + m.vertical();
        }
        total += pad_t + pad_b;
        if n > 1 { total += self.gap * (n - 1) as f64; }
        total.max(self.base.min_size.height)
    }

    fn layout(&mut self, available: Size) -> Size {
        let pad_l = self.inner_padding.left;
        let pad_r = self.inner_padding.right;
        let pad_t = self.inner_padding.top;
        let pad_b = self.inner_padding.bottom;
        let gap   = self.gap;
        let n     = self.children.len();
        if n == 0 { return available; }

        let inner_w = (available.width  - pad_l - pad_r).max(0.0);
        let inner_h = (available.height - pad_t - pad_b).max(0.0);

        // Scaled margins for all children (physical units).
        let scale    = device_scale();
        let margins: Vec<Insets> = self.children.iter()
            .map(|c| c.margin().scale(scale))
            .collect();

        let total_gap = if n > 1 { gap * (n - 1) as f64 } else { 0.0 };

        // -------------------------------------------------------------------
        // Step 1: measure fixed children on the main (vertical) axis.
        //
        // The slot for each fixed child = content_h + margin_top + margin_bottom.
        // Flex children contribute only their margins to the space budget.
        // -------------------------------------------------------------------
        let mut content_heights         = vec![0.0f64; n];
        let mut total_fixed_with_margins = 0.0f64;
        let mut total_flex               = 0.0f64;
        let mut total_flex_margin_v      = 0.0f64;
        let mut max_child_natural_w      = 0.0f64;

        for i in 0..n {
            let m     = &margins[i];
            let slot_w = (inner_w - m.left - m.right).max(0.0);
            if self.flex_factors[i] == 0.0 {
                // Measure at natural height; pass inner_h as the available
                // height so the child can self-report its natural size.
                let desired    = self.children[i].layout(Size::new(slot_w, inner_h));
                let clamped_h  = desired.height
                    .clamp(self.children[i].min_size().height,
                           self.children[i].max_size().height);
                content_heights[i]       = clamped_h;
                total_fixed_with_margins += clamped_h + m.vertical();
                max_child_natural_w = max_child_natural_w
                    .max(desired.width + m.horizontal());
            } else {
                total_flex          += self.flex_factors[i];
                total_flex_margin_v += m.vertical();
            }
        }

        // -------------------------------------------------------------------
        // Step 2: distribute remaining space to flex children.
        // -------------------------------------------------------------------
        let remaining = (inner_h
            - total_fixed_with_margins
            - total_gap
            - total_flex_margin_v)
            .max(0.0);
        let flex_unit = if total_flex > 0.0 { remaining / total_flex } else { 0.0 };

        for i in 0..n {
            if self.flex_factors[i] > 0.0 {
                let raw = self.flex_factors[i] * flex_unit;
                content_heights[i] = raw
                    .clamp(self.children[i].min_size().height,
                           self.children[i].max_size().height);
            }
        }

        // Natural content height (all-fixed case) determines the column's
        // reported size when there are no flex children.
        let natural_content_h = total_fixed_with_margins + total_gap;
        let effective_h = if total_flex > 0.0 { inner_h } else { natural_content_h };

        // -------------------------------------------------------------------
        // Step 3: place children top-to-bottom.
        //
        // In Y-up coordinates "top" = high Y.  Two cursor seeds:
        //
        //   - **Default** (`top_anchor=false`): start at `pad_b +
        //     effective_h`.  For all-fixed children this is the top
        //     of the natural-content extent; for flex children
        //     (`effective_h = inner_h`) it's the top of the inner
        //     area.  This matches what `ScrollView` expects when it
        //     calls `layout(MAX/2)` to measure natural size — children
        //     get placed at finite y-coords inside the natural area.
        //
        //   - **`top_anchor=true`**: start at the top of the inner
        //     area.  Used by columns embedded inside an oversized
        //     slot (e.g. inside a `Resize` widget) where the content
        //     should hug the TOP of the frame and any extra height
        //     should appear as whitespace below.
        let mut cursor_y = if self.top_anchor {
            available.height - pad_t
        } else {
            pad_b + effective_h
        };

        for i in 0..n {
            let m          = &margins[i];
            let slot_w     = (inner_w - m.left - m.right).max(0.0);
            let content_h  = content_heights[i];

            // Subtract top margin first (moves cursor toward lower Y = downward).
            cursor_y -= m.top;
            let child_bottom = cursor_y - content_h;

            // Layout child to obtain its natural width for cross-axis placement.
            let desired   = self.children[i].layout(Size::new(slot_w, content_h));
            let natural_w = desired.width;
            let h_anchor  = self.children[i].h_anchor();
            let min_w     = self.children[i].min_size().width;
            let max_w     = self.children[i].max_size().width;

            let (child_x, child_w) = place_cross_h(
                h_anchor, pad_l, inner_w, m.left, m.right, natural_w, min_w, max_w,
            );

            // Round to integers so bitmap content (cached text, images) lands on
            // exact pixel boundaries and isn't sub-pixel sampled into blur.
            self.children[i].set_bounds(Rect::new(
                child_x.round(), child_bottom.round(), child_w.round(), content_h.round(),
            ));

            // Advance cursor past bottom margin and inter-child gap.
            cursor_y = child_bottom - m.bottom - gap;
        }

        // Return natural size for all-fixed layouts so ScrollView can read
        // the true content height from layout()'s return value.
        //
        // Width: by default we report the full available width (legacy
        // behaviour many callers rely on).  `fit_width(true)` opts in
        // to reporting the widest non-flex child's natural width +
        // padding — NOT clamped to `available.width` so the parent
        // (typically an auto-sized `Window`) can grow to fit content
        // that exceeds the current slot.
        let reported_w = if self.fit_width {
            max_child_natural_w + pad_l + pad_r
        } else {
            available.width
        };
        if total_flex > 0.0 {
            Size::new(reported_w, available.height)
        } else {
            Size::new(reported_w, natural_content_h + pad_t + pad_b)
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let bg = if self.use_panel_bg {
            Some(ctx.visuals().panel_fill)
        } else if self.background.a > 0.001 {
            Some(self.background)
        } else {
            None
        };
        if let Some(color) = bg {
            let w = self.bounds.width;
            let h = self.bounds.height;
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// FlexRow
// ---------------------------------------------------------------------------

/// Arranges children left-to-right (first child = leftmost).
pub struct FlexRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    flex_factors: Vec<f64>,
    base: WidgetBase,
    pub gap: f64,
    pub inner_padding: Insets,
    pub background: Color,
}

impl FlexRow {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            flex_factors: Vec::new(),
            base: WidgetBase::new(),
            gap: 0.0,
            inner_padding: Insets::ZERO,
            background: Color::rgba(0.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn with_gap(mut self, gap: f64) -> Self { self.gap = gap; self }
    pub fn with_padding(mut self, p: f64) -> Self { self.inner_padding = Insets::all(p); self }
    pub fn with_inner_padding(mut self, p: Insets) -> Self { self.inner_padding = p; self }
    pub fn with_background(mut self, c: Color) -> Self { self.background = c; self }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self.flex_factors.push(0.0);
        self
    }

    pub fn add_flex(mut self, child: Box<dyn Widget>, flex: f64) -> Self {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
        self
    }

    pub fn push(&mut self, child: Box<dyn Widget>, flex: f64) {
        self.children.push(child);
        self.flex_factors.push(flex.max(0.0));
    }
}

impl Default for FlexRow { fn default() -> Self { Self::new() } }

impl Widget for FlexRow {
    fn type_name(&self) -> &'static str { "FlexRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        let pad_l = self.inner_padding.left;
        let pad_r = self.inner_padding.right;
        let pad_t = self.inner_padding.top;
        let pad_b = self.inner_padding.bottom;
        let gap   = self.gap;
        let n     = self.children.len();
        if n == 0 { return available; }

        let inner_w = (available.width  - pad_l - pad_r).max(0.0);
        let inner_h = (available.height - pad_t - pad_b).max(0.0);

        let scale   = device_scale();
        let margins: Vec<Insets> = self.children.iter()
            .map(|c| c.margin().scale(scale))
            .collect();

        let total_gap = if n > 1 { gap * (n - 1) as f64 } else { 0.0 };

        // -------------------------------------------------------------------
        // Step 1: measure fixed children on the main (horizontal) axis.
        // -------------------------------------------------------------------
        let mut content_widths           = vec![0.0f64; n];
        let mut total_fixed_with_margins  = 0.0f64;
        let mut total_flex               = 0.0f64;
        let mut total_flex_margin_h      = 0.0f64;

        for i in 0..n {
            let m      = &margins[i];
            let slot_h = (inner_h - m.bottom - m.top).max(0.0);
            if self.flex_factors[i] == 0.0 {
                // Pass inner_w as available width so the child can report its
                // natural width.
                let desired   = self.children[i].layout(Size::new(inner_w, slot_h));
                let clamped_w = desired.width
                    .clamp(self.children[i].min_size().width,
                           self.children[i].max_size().width);
                content_widths[i]          = clamped_w;
                total_fixed_with_margins   += clamped_w + m.horizontal();
            } else {
                total_flex          += self.flex_factors[i];
                total_flex_margin_h += m.horizontal();
            }
        }

        // -------------------------------------------------------------------
        // Step 2: distribute remaining space to flex children.
        // -------------------------------------------------------------------
        let remaining = (inner_w
            - total_fixed_with_margins
            - total_gap
            - total_flex_margin_h)
            .max(0.0);
        let flex_unit = if total_flex > 0.0 { remaining / total_flex } else { 0.0 };

        for i in 0..n {
            if self.flex_factors[i] > 0.0 {
                let raw = self.flex_factors[i] * flex_unit;
                content_widths[i] = raw
                    .clamp(self.children[i].min_size().width,
                           self.children[i].max_size().width);
            }
        }

        // -------------------------------------------------------------------
        // Step 3: place children left-to-right with cross-axis anchoring.
        // -------------------------------------------------------------------
        let mut cursor_x         = pad_l;
        let mut max_slot_h       = 0.0f64; // tallest slot (content + margins)

        for i in 0..n {
            let m          = &margins[i];
            let slot_h     = (inner_h - m.bottom - m.top).max(0.0);
            let content_w  = content_widths[i];

            // Advance past left margin.
            cursor_x += m.left;

            // Layout child to get natural height for cross-axis placement.
            let desired   = self.children[i].layout(Size::new(content_w, slot_h));
            let natural_h = desired.height;
            let v_anchor  = self.children[i].v_anchor();
            let min_h     = self.children[i].min_size().height;
            let max_h     = self.children[i].max_size().height;

            let (child_y, child_h) = place_cross_v(
                v_anchor, pad_b, inner_h, m.bottom, m.top, natural_h, min_h, max_h,
            );

            // Round to integers — same reason as FlexColumn (pixel-perfect blits).
            self.children[i].set_bounds(Rect::new(
                cursor_x.round(), child_y.round(), content_w.round(), child_h.round(),
            ));
            max_slot_h = max_slot_h.max(child_h + m.vertical());

            // Advance past content width, right margin, and inter-child gap.
            cursor_x += content_w + m.right + gap;
        }

        // Return the natural (intrinsic) height to avoid propagating huge
        // heights from ScrollView (which passes f64::MAX/2) through fixed rows.
        let natural_h = max_slot_h + pad_t + pad_b;
        Size::new(available.width, natural_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.background.a > 0.001 {
            let w = self.bounds.width;
            let h = self.bounds.height;
            ctx.set_fill_color(self.background);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
