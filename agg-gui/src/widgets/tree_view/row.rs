//! Compositional row widgets for `TreeView`:
//! `ExpandToggle`, `NodeIconWidget`, and `TreeRow`.
//!
//! `TreeView` builds one `TreeRow` per on-screen row and positions it; the
//! row lays out its parts left to right — indent, expand arrow, icon (a
//! procedural shape, an image, or a font glyph), label — plus optional
//! trailing parts at the right edge: dimmed secondary text and a fraction
//! bar (`row_trailing.rs`).

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::icon_image::IconImage;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;
use crate::widgets::primitives::SizedBox;

use super::node::{NodeGlyph, NodeIcon};
use super::row_trailing::FractionBar;

// ---------------------------------------------------------------------------
// Constants (moved from mod.rs so drag.rs and row.rs share one source)
// ---------------------------------------------------------------------------

pub const EXPAND_W: f64 = 18.0; // space reserved for expand arrow
pub const ICON_W: f64 = 14.0;
pub const ICON_GAP: f64 = 4.0;
/// Space between the label and each trailing part.
pub const TRAILING_GAP: f64 = 6.0;

// ---------------------------------------------------------------------------
// icon_color helper
// ---------------------------------------------------------------------------

/// Return the fill colour for a given node icon type.
pub fn icon_color(icon: NodeIcon) -> Color {
    match icon {
        NodeIcon::Folder => Color::rgb(0.90, 0.72, 0.20),
        NodeIcon::File => Color::rgb(0.55, 0.78, 0.95),
        NodeIcon::Package => Color::rgb(0.70, 0.60, 0.88),
    }
}

// ---------------------------------------------------------------------------
// ExpandToggle
// ---------------------------------------------------------------------------

/// Draws the ▶/▼ expand arrow. **Display-only** — returns `Ignored` for all events.
///
/// Interaction is handled centrally by `TreeView::on_event()`, which uses the
/// `RowMeta::toggle_rect` field (populated from `TreeRow::toggle_local_bounds` during
/// layout) to detect clicks on the toggle area and toggle `TreeNode::is_expanded` directly.
pub struct ExpandToggle {
    bounds: Rect,
    pub has_children: bool,
    pub is_expanded: bool,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

impl ExpandToggle {
    pub fn new(has_children: bool, is_expanded: bool) -> Self {
        Self {
            bounds: Rect::default(),
            has_children,
            is_expanded,
            children: Vec::new(),
            base: WidgetBase::new(),
        }
    }
}

impl Widget for ExpandToggle {
    fn type_name(&self) -> &'static str {
        "ExpandToggle"
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

    fn layout(&mut self, available: Size) -> Size {
        Size::new(EXPAND_W, available.height)
    }

    // The framework has already translated `ctx` to this widget's bottom-left origin.
    // All drawing coordinates are widget-local (0,0 = bottom-left of this widget).
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.has_children {
            return;
        }

        let w = self.bounds.width;
        let h = self.bounds.height;
        let cx = w * 0.5;
        let cy = h * 0.5;

        let v = ctx.visuals();
        ctx.set_fill_color(Color::rgba(
            v.text_color.r,
            v.text_color.g,
            v.text_color.b,
            0.55,
        ));
        ctx.begin_path();
        if self.is_expanded {
            // Down-pointing ▼
            ctx.move_to(cx - 4.5, cy + 2.0);
            ctx.line_to(cx + 4.5, cy + 2.0);
            ctx.line_to(cx, cy - 3.0);
            ctx.close_path();
        } else {
            // Right-pointing ▶
            ctx.move_to(cx - 2.5, cy - 4.5);
            ctx.line_to(cx - 2.5, cy + 4.5);
            ctx.line_to(cx + 3.5, cy);
            ctx.close_path();
        }
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// NodeIconWidget
// ---------------------------------------------------------------------------

/// Draws the coloured icon glyph for a node, or its image icon when one is
/// set ([`NodeIconWidget::with_image`]).
/// Width is `ICON_W + ICON_GAP` (wider for a wider image); height fills the row.
pub struct NodeIconWidget {
    bounds: Rect,
    pub icon: NodeIcon,
    /// Image drawn instead of the procedural `icon` when `Some`.
    pub image: Option<IconImage>,
    /// Glyph (and the font to draw it with) drawn instead of the image or
    /// procedural icon when `Some`.
    pub glyph: Option<(NodeGlyph, Arc<Font>)>,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

impl NodeIconWidget {
    pub fn new(icon: NodeIcon) -> Self {
        Self {
            bounds: Rect::default(),
            icon,
            image: None,
            glyph: None,
            children: Vec::new(),
            base: WidgetBase::new(),
        }
    }

    /// Draw `glyph` with its font (e.g. a Font Awesome code point) instead
    /// of the image or procedural icon.  `None` keeps those.
    pub fn with_glyph(mut self, glyph: Option<(NodeGlyph, Arc<Font>)>) -> Self {
        self.glyph = glyph;
        self
    }

    /// Draw `image` (at its logical size, device-resolution raster) instead
    /// of the procedural icon.  `None` keeps the procedural icon.
    pub fn with_image(mut self, image: Option<IconImage>) -> Self {
        self.image = image;
        self
    }
}

impl Widget for NodeIconWidget {
    fn type_name(&self) -> &'static str {
        "NodeIconWidget"
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

    fn layout(&mut self, available: Size) -> Size {
        let icon_w = match (&self.glyph, &self.image) {
            (Some(_), _) | (None, None) => ICON_W,
            (None, Some(image)) => image.size().width.max(ICON_W),
        };
        Size::new(icon_w + ICON_GAP, available.height)
    }

    // The framework has already translated `ctx` to this widget's bottom-left origin.
    // All drawing coordinates are widget-local (0,0 = bottom-left of this widget).
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        if let Some((glyph, font)) = &self.glyph {
            paint_glyph(ctx, glyph, font, h);
            return;
        }
        if let Some(image) = &self.image {
            image.draw(ctx, 0.0, (h - image.size().height) * 0.5);
            return;
        }
        let iy = (h - ICON_W) * 0.5;

        ctx.set_fill_color(icon_color(self.icon));
        ctx.begin_path();
        ctx.rounded_rect(0.0, iy, ICON_W, ICON_W, 2.0);
        ctx.fill();

        if matches!(self.icon, NodeIcon::Folder) {
            // Folder tab nub
            ctx.begin_path();
            ctx.rounded_rect(0.0, iy + ICON_W * 0.55, ICON_W * 0.45, ICON_W * 0.5, 1.0);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// TreeRow
// ---------------------------------------------------------------------------

/// Compositional row: `SizedBox` (indent) | `ExpandToggle` | `NodeIconWidget` |
/// `Label`, then optionally a dimmed secondary `Label` and a `FractionBar`
/// at the right edge ([`TreeRow::with_trailing`]).
///
/// **Event-routing note:** `TreeRow` and its children all return `EventResult::Ignored`.
/// The containing `TreeView` handles all events (selection, expand/collapse) using its
/// `row_metas: Vec<RowMeta>` which records each row's node_idx and toggle bounds.
///
/// **Hover painting** — the hover background is drawn by `TreeView::paint` from
/// the parent's `hovered_row` state.  Keeping it out of `TreeRow` means a hover
/// flip doesn't need to invalidate / rebuild the row's child label cache: only
/// the `TreeView` body re-rasterises, and the framework re-uses each label's
/// existing backbuffer.
pub struct TreeRow {
    bounds: Rect,
    pub node_idx: usize,
    /// The node's procedural icon, kept so [`TreeRow::with_icon_image`] can
    /// rebuild the icon cell.
    icon: NodeIcon,
    /// Bounds of the `ExpandToggle` in row-local coordinates (set in `layout()`).
    /// For leaf nodes (`has_children = false`), this field is `Rect::default()` (all zeros)
    /// and is never read — `TreeView` uses `None` for the corresponding `RowMeta::toggle_rect`.
    pub toggle_local_bounds: Rect,
    is_selected: bool,
    focused: bool,
    /// Index in `children` of the trailing secondary-text label.
    secondary_idx: Option<usize>,
    /// Index in `children` of the trailing fraction bar.
    fraction_idx: Option<usize>,
    /// Font and size of the main label, reused for the secondary text.
    font: Arc<Font>,
    font_size: f64,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

impl TreeRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_idx: usize,
        depth: u32,
        has_children: bool,
        is_expanded: bool,
        is_selected: bool,
        focused: bool,
        icon: NodeIcon,
        label: impl Into<String>,
        font: Arc<Font>,
        font_size: f64,
        indent_width: f64,
        row_height: f64,
    ) -> Self {
        let indent_px = depth as f64 * indent_width;
        let children: Vec<Box<dyn Widget>> = vec![
            Box::new(SizedBox::fixed(indent_px, row_height)),
            Box::new(ExpandToggle::new(has_children, is_expanded)),
            Box::new(NodeIconWidget::new(icon)),
            Box::new(Label::new(label, Arc::clone(&font)).with_font_size(font_size)),
        ];

        Self {
            bounds: Rect::default(),
            node_idx,
            icon,
            toggle_local_bounds: Rect::default(),
            is_selected,
            focused,
            secondary_idx: None,
            fraction_idx: None,
            font,
            font_size,
            children,
            base: WidgetBase::new(),
        }
    }

    /// Show `image` in the icon cell instead of the procedural icon.
    pub fn with_icon_image(mut self, image: Option<IconImage>) -> Self {
        if image.is_some() {
            self.children[2] = Box::new(NodeIconWidget::new(self.icon).with_image(image));
        }
        self
    }

    /// Show a font glyph in the icon cell (takes precedence over an image).
    pub fn with_icon_glyph(mut self, glyph: Option<(NodeGlyph, Arc<Font>)>) -> Self {
        if glyph.is_some() {
            self.children[2] = Box::new(NodeIconWidget::new(self.icon).with_glyph(glyph));
        }
        self
    }

    /// Add right-aligned dimmed `secondary` text and / or a fraction bar
    /// (`0..=1`) at the row's trailing edge.  The main label gives up the
    /// width they take.
    pub fn with_trailing(mut self, secondary: Option<String>, fraction: Option<f32>) -> Self {
        if let Some(text) = secondary {
            let label = Label::new(text, Arc::clone(&self.font))
                .with_font_size(self.font_size)
                .with_dim(true);
            self.secondary_idx = Some(self.children.len());
            self.children.push(Box::new(label));
        }
        if let Some(f) = fraction {
            self.fraction_idx = Some(self.children.len());
            self.children.push(Box::new(FractionBar::new(f)));
        }
        self
    }
}

impl Widget for TreeRow {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "TreeRow"
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

    fn layout(&mut self, available: Size) -> Size {
        let h = available.height;
        let total_w = available.width;

        // Children 0, 1, 2 get their natural width.
        // Child 3 (Label) gets the remaining width.
        let mut x = 0.0;

        // Child 0: SizedBox (indent)
        let s0 = self.children[0].layout(Size::new(total_w, h));
        self.children[0].set_bounds(Rect::new(x, 0.0, s0.width, h));
        x += s0.width;

        // Child 1: ExpandToggle — cache its x for toggle hit-testing
        let s1 = self.children[1].layout(Size::new(total_w - x, h));
        self.children[1].set_bounds(Rect::new(x, 0.0, s1.width, h));
        self.toggle_local_bounds = Rect::new(x, 0.0, s1.width, h);
        x += s1.width;

        // Child 2: NodeIconWidget
        let s2 = self.children[2].layout(Size::new(total_w - x, h));
        self.children[2].set_bounds(Rect::new(x, 0.0, s2.width, h));
        x += s2.width;

        // Trailing parts, right to left: fraction bar, then secondary text.
        let mut right = total_w;
        for idx in [self.fraction_idx, self.secondary_idx]
            .into_iter()
            .flatten()
        {
            let s = self.children[idx].layout(Size::new((right - x).max(0.0), h));
            right -= s.width;
            self.children[idx].set_bounds(Rect::new(right, 0.0, s.width, h));
            right -= TRAILING_GAP;
        }

        // Child 3: Label — remaining width
        let label_w = (right - x).max(0.0);
        let s3 = self.children[3].layout(Size::new(label_w, h));
        self.children[3].set_bounds(Rect::new(x, 0.0, s3.width, h));

        Size::new(total_w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let v = ctx.visuals();

        if self.is_selected {
            let c = if self.focused {
                // Accent-tinted overlay — same colour in both themes so the
                // selection reads as "selected" regardless of palette.
                Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.25)
            } else {
                // Theme-neutral dim overlay: subtle tint of the text color.
                Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.12)
            };
            ctx.set_fill_color(c);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Draw `glyph` with `font`, centred in the `ICON_W` cell of a row `h` tall.
/// Drawn as text every paint, so it follows the display scale.
fn paint_glyph(ctx: &mut dyn DrawCtx, glyph: &NodeGlyph, font: &Arc<Font>, h: f64) {
    let text = glyph.glyph.to_string();
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(ICON_W);
    ctx.set_fill_color(glyph.color);
    if let Some(m) = ctx.measure_text(&text) {
        let x = ((ICON_W - m.width) * 0.5).max(0.0);
        // Centre the ascent-to-descent box vertically (Y up, baseline at y).
        let y = (h - (m.ascent + m.descent)) * 0.5 + m.descent;
        ctx.fill_text(&text, x, y);
    }
}
