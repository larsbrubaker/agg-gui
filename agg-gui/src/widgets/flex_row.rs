//! `FlexRow`: horizontal flex layout (children left-to-right).
//!
//! Split out of `flex.rs` to keep both files under the workspace 800-line
//! guardrail. Shares the flex algorithm and Y-up conventions documented on
//! [`crate::widgets::flex`]; `FlexColumn` lives there.
//!
//! `FlexRow` reads each child's `v_anchor()` to place it vertically within
//! the row's inner height (see [`place_cross_v`]).

use crate::color::Color;
use crate::device_scale::device_scale;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{resolve_fit_or_stretch, HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

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
    pad_b: f64,
    inner_h: f64,
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

/// Arranges children left-to-right (first child = leftmost).
pub struct FlexRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    flex_factors: Vec<f64>,
    base: WidgetBase,
    pub gap: f64,
    pub inner_padding: Insets,
    pub background: Color,
    /// When `true`, `layout` reports the row's natural content width
    /// (sum of fixed children + gaps + horizontal padding) instead of the
    /// full `available.width`. Mirrors [`crate::widgets::FlexColumn`]'s
    /// `fit_width` — needed when the row is floated by an auto-sized
    /// ancestor (e.g. a `Stack` `add_aligned` overlay) that must hug the
    /// content rather than span the whole stack. Off by default for
    /// backward compatibility.
    pub fit_width: bool,
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
            fit_width: false,
        }
    }

    pub fn with_gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }

    /// Opt into content-fit width — see [`FlexRow::fit_width`].
    pub fn with_fit_width(mut self, fit: bool) -> Self {
        self.fit_width = fit;
        self
    }
    pub fn with_padding(mut self, p: f64) -> Self {
        self.inner_padding = Insets::all(p);
        self
    }
    pub fn with_inner_padding(mut self, p: Insets) -> Self {
        self.inner_padding = p;
        self
    }
    pub fn with_background(mut self, c: Color) -> Self {
        self.background = c;
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

impl Default for FlexRow {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for FlexRow {
    fn type_name(&self) -> &'static str {
        "FlexRow"
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
    fn padding(&self) -> Insets {
        self.inner_padding
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
        let pad_l = self.inner_padding.left;
        let pad_r = self.inner_padding.right;
        let pad_t = self.inner_padding.top;
        let pad_b = self.inner_padding.bottom;
        let gap = self.gap;
        let n = self.children.len();
        if n == 0 {
            return available;
        }

        let inner_w = (available.width - pad_l - pad_r).max(0.0);
        let inner_h = (available.height - pad_t - pad_b).max(0.0);

        let scale = device_scale();
        let margins: Vec<Insets> = self
            .children
            .iter()
            .map(|c| c.margin().scale(scale))
            .collect();

        let total_gap = if n > 1 { gap * (n - 1) as f64 } else { 0.0 };

        // -------------------------------------------------------------------
        // Step 1: measure fixed children on the main (horizontal) axis.
        // -------------------------------------------------------------------
        let mut content_widths = vec![0.0f64; n];
        let mut total_fixed_with_margins = 0.0f64;
        let mut total_flex = 0.0f64;
        let mut total_flex_margin_h = 0.0f64;

        for i in 0..n {
            let m = &margins[i];
            let slot_h = (inner_h - m.bottom - m.top).max(0.0);
            if self.flex_factors[i] == 0.0 {
                // Pass inner_w as available width so the child can report its
                // natural width.
                let desired = self.children[i].layout(Size::new(inner_w, slot_h));
                let clamped_w = desired.width.clamp(
                    self.children[i].min_size().width,
                    self.children[i].max_size().width,
                );
                content_widths[i] = clamped_w;
                total_fixed_with_margins += clamped_w + m.horizontal();
            } else {
                total_flex += self.flex_factors[i];
                total_flex_margin_h += m.horizontal();
            }
        }

        // -------------------------------------------------------------------
        // Step 2: distribute remaining space to flex children.
        // -------------------------------------------------------------------
        let remaining =
            (inner_w - total_fixed_with_margins - total_gap - total_flex_margin_h).max(0.0);
        let flex_unit = if total_flex > 0.0 {
            remaining / total_flex
        } else {
            0.0
        };

        for i in 0..n {
            if self.flex_factors[i] > 0.0 {
                let raw = self.flex_factors[i] * flex_unit;
                content_widths[i] = raw.clamp(
                    self.children[i].min_size().width,
                    self.children[i].max_size().width,
                );
            }
        }

        // -------------------------------------------------------------------
        // Step 3: place children left-to-right with cross-axis anchoring.
        // -------------------------------------------------------------------
        let mut cursor_x = pad_l;
        let mut max_slot_h = 0.0f64; // tallest slot (content + margins)

        for i in 0..n {
            let m = &margins[i];
            let slot_h = (inner_h - m.bottom - m.top).max(0.0);
            let content_w = content_widths[i];

            // Advance past left margin.
            cursor_x += m.left;

            // Layout child to get natural height for cross-axis placement.
            let desired = self.children[i].layout(Size::new(content_w, slot_h));
            let natural_h = desired.height;
            let v_anchor = self.children[i].v_anchor();
            let min_h = self.children[i].min_size().height;
            let max_h = self.children[i].max_size().height;

            let (child_y, child_h) = place_cross_v(
                v_anchor, pad_b, inner_h, m.bottom, m.top, natural_h, min_h, max_h,
            );

            // Round to integers — same reason as FlexColumn (pixel-perfect blits).
            let final_w = content_w.round();
            let final_h = child_h.round();
            // Re-layout at the final assigned box. The measure pass above used
            // the full slot height, so a fit-content child (e.g. a shorter
            // FlexColumn) top-anchored its own children for that taller slot.
            // Without this, those grandchildren keep their tall-slot positions
            // and fall outside the child's now-shorter bounds → clipped away
            // (the "flyout opens but its buttons never paint" bug).
            if (final_h - slot_h).abs() > 0.5 || (final_w - content_w).abs() > 0.5 {
                self.children[i].layout(Size::new(final_w, final_h));
            }
            self.children[i].set_bounds(Rect::new(
                cursor_x.round(),
                child_y.round(),
                final_w,
                final_h,
            ));
            max_slot_h = max_slot_h.max(child_h + m.vertical());

            // Advance past content width, right margin, and inter-child gap.
            cursor_x += content_w + m.right + gap;
        }

        // Return the natural (intrinsic) height to avoid propagating huge
        // heights from ScrollView (which passes f64::MAX/2) through fixed rows.
        let natural_h = max_slot_h + pad_t + pad_b;
        // Width: full available by default (legacy). `fit_width` reports the
        // content extent so an auto-sized parent can hug the row.
        let reported_w = if self.fit_width {
            pad_l + pad_r + total_fixed_with_margins + total_gap
        } else {
            available.width
        };
        Size::new(reported_w, natural_h)
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

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
