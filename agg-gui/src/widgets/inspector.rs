//! Widget inspector panel — uses the system `TreeView` for tree display.
//!
//! Layout (Y-up, panel fills its full bounds):
//! ```text
//! ┌─────────────────────┐ ← top (HEADER_H)   header (painted monolithically)
//! ├─────────────────────┤
//! │   TreeView          │ ← tree area (TreeView painted here)
//! ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤ ← draggable h-split
//! │   Properties        │ ← props area (painted monolithically)
//! └─────────────────────┘ ← bottom (y=0)
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{InspectorNode, InspectorOverlay, Widget};
use crate::widgets::tree_view::{NodeIcon, TreeNode, TreeView};

// ── InternalPresenceNode ──────────────────────────────────────────────────────

/// Transparent placeholder child representing the inspector's internal `TreeView`
/// in the widget inspector tree.
///
/// This makes `InspectorPanel` appear as an expandable node (with one child) in
/// the inspector rather than a leaf, so the user can see that the panel contains
/// an internal tree.
///
/// Hit-test is always `false` (no event interception).  Paint is a no-op (the
/// real `TreeView` is painted directly by `InspectorPanel`).
/// `contributes_children_to_inspector` returns `false` to stop the inspector
/// from recursing into row_widgets, which would grow the node list exponentially.
///
/// Bounds are kept in sync with the real `TreeView` by `InspectorPanel::layout`.
struct InternalPresenceNode {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    name: &'static str,
}

impl Widget for InternalPresenceNode {
    fn type_name(&self) -> &'static str {
        self.name
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
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn hit_test(&self, _: Point) -> bool {
        false
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn contributes_children_to_inspector(&self) -> bool {
        false
    }
}

// ── geometry constants ────────────────────────────────────────────────────────
const DEFAULT_PROPS_H: f64 = 180.0;
pub(super) const FONT_SIZE: f64 = 12.0;
const HEADER_H: f64 = 30.0;
const SPLIT_HIT: f64 = 5.0;
const MIN_PROPS_H: f64 = 60.0;
const MIN_TREE_H: f64 = 60.0;

// ── light theme colors ────────────────────────────────────────────────────────
// Theme-aware colour helpers — all derive from the active `Visuals` so the
// inspector follows light / dark mode changes without a restart.
fn c_panel_bg(v: &crate::theme::Visuals) -> Color {
    v.panel_fill
}
fn c_header_bg(v: &crate::theme::Visuals) -> Color {
    // Slightly darker than the panel fill.
    let f = if is_dark(v) { 0.80 } else { 0.94 };
    Color::rgba(
        v.panel_fill.r * f,
        v.panel_fill.g * f,
        v.panel_fill.b * f,
        1.0,
    )
}
fn c_props_bg(v: &crate::theme::Visuals) -> Color {
    v.window_fill
}
fn c_split_bg(v: &crate::theme::Visuals) -> Color {
    let t = if is_dark(v) { 1.0 } else { 0.0 };
    Color::rgba(t, t, t, 0.10)
}
pub(super) fn c_border(v: &crate::theme::Visuals) -> Color {
    v.separator
}
pub(super) fn c_text(v: &crate::theme::Visuals) -> Color {
    v.text_color
}
pub(super) fn c_dim_text(v: &crate::theme::Visuals) -> Color {
    v.text_dim
}

fn is_dark(v: &crate::theme::Visuals) -> bool {
    // Panel fill luminance — below 0.5 means we're in a dark palette.
    let lum = 0.299 * v.panel_fill.r + 0.587 * v.panel_fill.g + 0.114 * v.panel_fill.b;
    lum < 0.5
}

// ── event translation helper ──────────────────────────────────────────────────

/// Translate the Y coordinate of a mouse event by subtracting `offset_y`.
/// X is unchanged. Non-mouse events pass through unchanged.
fn translate_event(event: &Event, offset_y: f64) -> Event {
    match event {
        Event::MouseDown {
            pos,
            button,
            modifiers,
        } => Event::MouseDown {
            pos: Point::new(pos.x, pos.y - offset_y),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseMove { pos } => Event::MouseMove {
            pos: Point::new(pos.x, pos.y - offset_y),
        },
        Event::MouseUp {
            pos,
            button,
            modifiers,
        } => Event::MouseUp {
            pos: Point::new(pos.x, pos.y - offset_y),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseWheel {
            pos,
            delta_y,
            delta_x,
            modifiers,
        } => Event::MouseWheel {
            pos: Point::new(pos.x, pos.y - offset_y),
            delta_y: *delta_y,
            delta_x: *delta_x,
            modifiers: *modifiers,
        },
        other => other.clone(),
    }
}

// ── InspectorPanel ────────────────────────────────────────────────────────────

pub struct InspectorPanel {
    bounds: Rect,
    /// Contains exactly one `InternalPresenceNode` (a transparent proxy for the
    /// internal `TreeView`).  This makes InspectorPanel non-leaf in the inspector
    /// so the user can see it has internal structure.
    _children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    font: Arc<Font>,
    nodes: Rc<RefCell<Vec<InspectorNode>>>,
    /// Selected: original node index; synced from TreeView selection.
    selected: Option<usize>,
    props_h: f64,
    split_dragging: bool,
    /// Written by layout(); read by the render loop to draw an overlay.
    pub hovered_bounds: Rc<RefCell<Option<InspectorOverlay>>>,
    /// The tree widget, managed directly (not in children).
    pub(crate) tree_view: TreeView,
    /// Set by `apply_saved_state`; consumed on the next layout rebuild so
    /// restored expand / select flags apply even on the very first frame
    /// (before the user has interacted with the tree).
    pending_expanded: Option<Vec<bool>>,
    pending_selected: Option<Option<usize>>,
    /// When bound, each `layout()` writes the current state into this cell
    /// so the harness can persist it without needing mutable access to the
    /// widget tree.
    snapshot_out: Option<Rc<RefCell<Option<InspectorSavedState>>>>,
    /// Edit queue — clicks on reflected property rows push
    /// [`crate::widget::InspectorEdit`] entries here; the host frame loop
    /// drains and applies them via [`crate::widget::apply_inspector_edit`].
    /// Holds mouse-down coords so the click handler in `on_event` can locate
    /// which property row was hit when the layout next paints.
    #[cfg(feature = "reflect")]
    pub edits: Option<Rc<RefCell<Vec<crate::widget::InspectorEdit>>>>,
    /// Cached row hit-rectangles built during paint (panel-local bounds);
    /// each entry is `(rect, field_name, row_kind)`.  Used by `on_event` to
    /// translate a click to a queued edit.
    prop_hits: Vec<PropHit>,
    /// Fingerprint of the `inspector_nodes` Vec we last rebuilt the
    /// `TreeView` from — `(data ptr as usize, len)`.  When the harness
    /// skips its snapshot pass (e.g. during a window-resize drag), the
    /// Vec is reused and the ptr stays the same; we then skip the
    /// per-frame `tree_view.nodes` rebuild too, so the tree's row
    /// widgets reuse their backbuffers and the resize stays cheap.
    last_inspector_nodes_fingerprint: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "reflect"), allow(dead_code))]
pub(super) struct PropHit {
    pub(super) rect: Rect,
    pub(super) field: String,
    pub(super) kind: PropHitKind,
}

#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "reflect"), allow(dead_code))]
pub(super) enum PropHitKind {
    /// Clicking flips the bool.
    BoolToggle { current: bool },
    /// Clicking the left half decrements, right half increments by `step`.
    NumericStep { current: f64, step: f64 },
}

/// Serializable inspector UI state — apply at startup, snapshot at shutdown.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct InspectorSavedState {
    pub expanded: Vec<bool>,
    pub selected: Option<usize>,
    pub props_h: f64,
}

impl InspectorPanel {
    pub fn new(
        font: Arc<Font>,
        nodes: Rc<RefCell<Vec<InspectorNode>>>,
        hovered_bounds: Rc<RefCell<Option<InspectorOverlay>>>,
    ) -> Self {
        let tree_view = TreeView::new(Arc::clone(&font))
            .with_row_height(20.0)
            .with_font_size(12.0)
            .with_indent_width(14.0)
            .with_hover_repaint(false);
        Self {
            bounds: Rect::default(),
            _children: vec![Box::new(InternalPresenceNode {
                bounds: Rect::default(),
                children: Vec::new(),
                base: WidgetBase::new(),
                name: "TreeView",
            })],
            base: WidgetBase::new(),
            font,
            nodes,
            selected: None,
            props_h: DEFAULT_PROPS_H,
            split_dragging: false,
            hovered_bounds,
            tree_view,
            #[cfg(feature = "reflect")]
            edits: None,
            prop_hits: Vec::new(),
            pending_expanded: None,
            pending_selected: None,
            snapshot_out: None,
            last_inspector_nodes_fingerprint: None,
        }
    }

    /// Bind an output cell that the inspector updates every layout with
    /// the current [`InspectorSavedState`] — use the cell from a harness
    /// that persists app state.
    pub fn with_snapshot_cell(mut self, cell: Rc<RefCell<Option<InspectorSavedState>>>) -> Self {
        self.snapshot_out = Some(cell);
        self
    }

    /// Bind a queue the inspector pushes [`crate::widget::InspectorEdit`]s
    /// into when the user clicks an editable property value.  The host frame
    /// loop is responsible for draining and applying via
    /// [`crate::widget::apply_inspector_edit`] — doing it inline would
    /// violate the immutable-tree-during-event contract.
    #[cfg(feature = "reflect")]
    pub fn with_edit_queue(
        mut self,
        cell: Rc<RefCell<Vec<crate::widget::InspectorEdit>>>,
    ) -> Self {
        self.edits = Some(cell);
        self
    }

    // ── Persistence helpers ──────────────────────────────────────────────────
    //
    // The platform harness calls `saved_state` at shutdown and
    // `apply_saved_state` on startup so the inspector's tree expand /
    // selection / split-bar position survive restarts.  Values are stored
    // by the position they occupy in the flat DFS tree — if the widget
    // tree differs across runs the worst case is a few extra collapsed
    // nodes, never a crash.

    /// Snapshot the current inspector UI state for persistence.
    pub fn saved_state(&self) -> InspectorSavedState {
        InspectorSavedState {
            expanded: self.tree_view.nodes.iter().map(|n| n.is_expanded).collect(),
            selected: self.tree_view.nodes.iter().position(|n| n.is_selected),
            props_h: self.props_h,
        }
    }

    /// Apply a previously-saved state.  Must be called before the first
    /// `layout()` runs — the inspector restores the expand / select flags
    /// from here when it first rebuilds the TreeView, via the `pending_*`
    /// side channels.
    pub fn apply_saved_state(&mut self, s: InspectorSavedState) {
        self.pending_expanded = Some(s.expanded);
        self.pending_selected = Some(s.selected);
        self.props_h = s.props_h.clamp(MIN_PROPS_H, 1024.0);
    }

    // ── geometry helpers ──────────────────────────────────────────────────────

    /// Height of the area below the header (tree + props).
    fn list_area_h(&self) -> f64 {
        (self.bounds.height - HEADER_H).max(0.0)
    }

    /// Y position of the tree/props split line (from panel bottom).
    fn split_y(&self) -> f64 {
        self.props_h.clamp(
            MIN_PROPS_H,
            (self.list_area_h() - MIN_TREE_H).max(MIN_PROPS_H),
        )
    }

    /// Bottom Y of the tree area (just above the split handle).
    fn tree_origin_y(&self) -> f64 {
        self.split_y() + 4.0
    }

    fn on_split_handle(&self, pos: Point) -> bool {
        let sy = self.split_y();
        pos.y >= sy - SPLIT_HIT && pos.y <= sy + SPLIT_HIT
    }

    fn pos_in_tree_area(&self, pos: Point) -> bool {
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        pos.y >= tree_bot && pos.y <= tree_top
    }

    /// Forward event to the TreeView, translating Y into tree-local coordinates.
    fn forward_to_tree(&mut self, event: &Event) -> EventResult {
        // tree_view.bounds().y is tree_origin_y() in panel-local space — subtracting
        // it converts panel-local Y to TreeView-local Y (where y=0 is the bottom of
        // the tree area).
        let offset_y = self.tree_view.bounds().y;
        let translated = translate_event(event, offset_y);
        self.tree_view.on_event(&translated)
    }

    fn update_hovered_bounds_from_tree(&self) {
        let nodes = self.nodes.borrow();
        let next = self
            .tree_view
            .hovered_node_idx()
            .and_then(|i| nodes.get(i))
            .map(|n| InspectorOverlay {
                bounds: n.screen_bounds,
                margin: n.margin,
                padding: n.padding,
            });
        let mut hovered = self.hovered_bounds.borrow_mut();
        if *hovered != next {
            *hovered = next;
            crate::animation::request_draw_without_invalidation();
        }
    }
}

// ── Widget impl ───────────────────────────────────────────────────────────────

impl InspectorPanel {
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

impl Widget for InspectorPanel {
    fn type_name(&self) -> &'static str {
        "InspectorPanel"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self._children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self._children
    }

    fn margin(&self) -> Insets {
        self.base.margin
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
        self.bounds.width = available.width;
        self.bounds.height = available.height;

        let nodes = self.nodes.borrow();
        // Fingerprint of the inspector_nodes Vec.  When the harness skips
        // a snapshot pass (e.g. during a window-resize drag) the Vec is
        // reused, so the data ptr stays the same — we then skip the
        // tree_view.nodes rebuild here.  Combined with TreeView's row
        // caching, this is what makes inspector window resizing cheap.
        let nodes_fingerprint = (nodes.as_ptr() as usize, nodes.len());
        let pending_state =
            self.pending_expanded.is_some() || self.pending_selected.is_some();
        let nodes_unchanged = !pending_state
            && self.last_inspector_nodes_fingerprint == Some(nodes_fingerprint)
            && !self.tree_view.nodes.is_empty();

        if !nodes_unchanged {
            // Preserve expansion/selection state by index before rebuilding.
            // On the very first layout after startup `pending_expanded` /
            // `pending_selected` (set by `apply_saved_state`) seed the vectors
            // so restored state takes effect without an extra click.
            let mut old_expanded: Vec<bool> =
                self.tree_view.nodes.iter().map(|n| n.is_expanded).collect();
            let mut old_selected: Vec<bool> =
                self.tree_view.nodes.iter().map(|n| n.is_selected).collect();
            if let Some(pe) = self.pending_expanded.take() {
                old_expanded = pe;
            }
            if let Some(ps) = self.pending_selected.take() {
                old_selected =
                    vec![false; old_expanded.len().max(ps.map(|i| i + 1).unwrap_or(0))];
                if let Some(i) = ps {
                    if i < old_selected.len() {
                        old_selected[i] = true;
                    }
                }
            }

            self.tree_view.nodes.clear();

            // Convert flat InspectorNode list (with depths) to parent-child TreeNode
            // structure. Uses a depth stack: depth_stack[d] = tree node index of the
            // last node placed at depth d.
            let mut depth_stack: Vec<usize> = Vec::new();
            let mut per_parent_counts: std::collections::HashMap<Option<usize>, u32> =
                std::collections::HashMap::new();

            for (orig_idx, node) in nodes.iter().enumerate() {
                let parent = if node.depth == 0 {
                    None
                } else {
                    depth_stack.get(node.depth.saturating_sub(1)).copied()
                };

                let order = {
                    let cnt = per_parent_counts.entry(parent).or_insert(0);
                    let o = *cnt;
                    *cnt += 1;
                    o
                };

                // Label: "TypeName  width×height"
                let b = &node.screen_bounds;
                let label =
                    format!("{}  {:.0}×{:.0}", node.type_name, b.width, b.height);

                let tv_idx = self.tree_view.nodes.len();
                self.tree_view
                    .nodes
                    .push(TreeNode::new(label, NodeIcon::Package, parent, order));

                // Restore or default expansion (default: expanded so tree is open).
                self.tree_view.nodes[tv_idx].is_expanded =
                    old_expanded.get(orig_idx).copied().unwrap_or(true);
                self.tree_view.nodes[tv_idx].is_selected =
                    old_selected.get(orig_idx).copied().unwrap_or(false);

                // Update depth stack.
                if depth_stack.len() <= node.depth {
                    depth_stack.resize(node.depth + 1, 0);
                }
                depth_stack[node.depth] = tv_idx;
            }
            self.last_inspector_nodes_fingerprint = Some(nodes_fingerprint);
        }

        // Sync selected field from TreeView selection.
        self.selected = self.tree_view.nodes.iter().position(|n| n.is_selected);

        // Update hovered_bounds for the render-loop overlay.
        *self.hovered_bounds.borrow_mut() = self
            .tree_view
            .hovered_node_idx()
            .and_then(|i| nodes.get(i))
            .map(|n| InspectorOverlay {
                bounds: n.screen_bounds,
                margin: n.margin,
                padding: n.padding,
            });

        // Layout the TreeView inside the tree area.
        let tree_w = available.width;
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        let tree_h = (tree_top - tree_bot).max(0.0);
        self.tree_view
            .set_bounds(Rect::new(0.0, tree_bot, tree_w, tree_h));
        self.tree_view.layout(Size::new(tree_w, tree_h));

        // Keep the presence node's bounds in sync with the real TreeView so the
        // inspector displays accurate bounds for this proxy entry.
        self._children[0].set_bounds(self.tree_view.bounds());

        // Publish a snapshot for the harness to persist.
        if let Some(cell) = &self.snapshot_out {
            *cell.borrow_mut() = Some(self.saved_state());
        }

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let sy = self.split_y();
        let hdr_y = h - HEADER_H;
        let v = ctx.visuals().clone();

        // ── panel background ─────────────────────────────────────────────────
        ctx.set_fill_color(c_panel_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Left border
        ctx.set_stroke_color(c_border(&v));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, 0.0);
        ctx.line_to(0.0, h);
        ctx.stroke();

        // ── header ──────────────────────────────────────────────────────────
        ctx.set_fill_color(c_header_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, hdr_y, w, HEADER_H);
        ctx.fill();

        ctx.set_stroke_color(c_border(&v));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, hdr_y);
        ctx.line_to(w, hdr_y);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(13.0);
        ctx.set_fill_color(c_text(&v));
        let title = "Widget Inspector";
        if let Some(m) = ctx.measure_text(title) {
            ctx.fill_text(
                title,
                12.0,
                hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent,
            );
        }

        let count_txt = format!("{} widgets", self.nodes.borrow().len());
        ctx.set_font_size(11.0);
        ctx.set_fill_color(c_dim_text(&v));
        if let Some(m) = ctx.measure_text(&count_txt) {
            ctx.fill_text(
                &count_txt,
                w - m.width - 10.0,
                hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent,
            );
        }

        // ── properties pane ──────────────────────────────────────────────────
        ctx.set_fill_color(c_props_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, sy - 2.0);
        ctx.fill();
        self.paint_properties(ctx, sy - 2.0);

        // ── split handle ─────────────────────────────────────────────────────
        ctx.set_fill_color(c_split_bg(&v));
        ctx.begin_path();
        ctx.rect(0.0, sy - 2.0, w, 4.0);
        ctx.fill();
        ctx.set_stroke_color(c_border(&v));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, sy);
        ctx.line_to(w, sy);
        ctx.stroke();

        // ── tree area: clip then paint TreeView ──────────────────────────────
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        let tree_h = (tree_top - tree_bot).max(0.0);
        if tree_h > 0.0 {
            ctx.save();
            ctx.translate(0.0, tree_bot);
            // clip_rect is called AFTER translate so coordinates are in
            // tree-local space (0,0 = tree area bottom-left). The implementation
            // maps these through the CTM to screen space before intersecting.
            ctx.clip_rect(0.0, 0.0, w, tree_h);
            // Use paint_subtree so the framework recurses into TreeRow children.
            crate::widget::paint_subtree(&mut self.tree_view, ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                #[cfg(feature = "reflect")]
                if pos.y < self.split_y() - 2.0 && self.try_emit_edit_from_click(*pos) {
                    return EventResult::Consumed;
                }
                if self.on_split_handle(*pos) {
                    self.split_dragging = true;
                    // No tick: grabbing the split handle produces no visual
                    // change until the cursor moves.  The follow-up
                    // MouseMove handler ticks as the split actually shifts.
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    return self.forward_to_tree(event);
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                if self.split_dragging {
                    self.props_h = pos.y.clamp(
                        MIN_PROPS_H,
                        (self.list_area_h() - MIN_TREE_H).max(MIN_PROPS_H),
                    );
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    let _ = self.forward_to_tree(event);
                    self.update_hovered_bounds_from_tree();
                } else if self.hovered_bounds.borrow().is_some() {
                    *self.hovered_bounds.borrow_mut() = None;
                    crate::animation::request_draw_without_invalidation();
                }
                EventResult::Ignored
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                if self.split_dragging {
                    self.split_dragging = false;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    return self.forward_to_tree(event);
                }
                EventResult::Ignored
            }
            Event::MouseWheel { pos, .. } if self.pos_in_tree_area(*pos) => {
                self.forward_to_tree(event)
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── properties pane ──────────────────────────────────────────────────────────
// Implementation lives in `inspector_props.rs` to keep this file under the
// project line limit.

impl InspectorPanel {
    fn paint_properties(&mut self, ctx: &mut dyn DrawCtx, available_h: f64) {
        let panel_y_offset = 0.0; // properties pane sits at panel-local y=0
        self.prop_hits.clear();
        super::inspector_props::paint_properties(
            ctx,
            available_h,
            panel_y_offset,
            self.bounds.width,
            &self.font,
            self.selected,
            &self.nodes.borrow(),
            &mut self.prop_hits,
        );
    }

    /// Test a panel-local click against the cached property-row rectangles
    /// painted last frame.  Returns true if the click produced a queued edit.
    #[cfg(feature = "reflect")]
    fn try_emit_edit_from_click(&self, pos: Point) -> bool {
        let Some(queue) = &self.edits else { return false };
        let Some(sel_idx) = self.selected else {
            return false;
        };
        let nodes = self.nodes.borrow();
        let Some(node) = nodes.get(sel_idx) else {
            return false;
        };
        let Some(hit) = self
            .prop_hits
            .iter()
            .find(|h| pos.x >= h.rect.x
                && pos.x <= h.rect.x + h.rect.width
                && pos.y >= h.rect.y
                && pos.y <= h.rect.y + h.rect.height)
        else {
            return false;
        };

        let edit = match &hit.kind {
            PropHitKind::BoolToggle { current } => crate::widget::InspectorEdit {
                path: node.path.clone(),
                field_path: hit.field.clone(),
                new_value: Box::new(!*current),
            },
            PropHitKind::NumericStep { current, step } => {
                let mid = hit.rect.x + hit.rect.width * 0.5;
                let new_v = if pos.x < mid {
                    *current - *step
                } else {
                    *current + *step
                };
                crate::widget::InspectorEdit {
                    path: node.path.clone(),
                    field_path: hit.field.clone(),
                    new_value: Box::new(new_v),
                }
            }
        };
        queue.borrow_mut().push(edit);
        crate::animation::request_draw();
        true
    }
}

