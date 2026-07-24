//! `ValueEditorWidget` — the inline editor for a property row.
//!
//! Painting routes through agg-gui's per-`EditorKind` row renderers
//! ([`agg_gui::widgets::paint_row`]). The widget itself owns layout
//! (where the row sits inside the node) and value translation
//! ([`PropertyValue`] → [`RowValue`]); the renderer paints label +
//! editor according to the kind.
//!
//! The widget is intentionally narrow — drag interaction is still
//! routed through `NodeEditor` because the canvas-space hit-testing
//! already exists there. A future pass can move event handling into
//! per-kind widgets too.

use agg_gui::{
    widgets::{paint_editor_only, paint_row, EditorKind, RowValue},
    DrawCtx, Event, EventResult, Rect, Size, Widget, WidgetBase,
};

use super::node_paint_context::NodePaintContext;
use crate::draw::PropLayout;
use crate::model::PropertyValue;

pub struct ValueEditorWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    prop: PropLayout,
    /// `true` when the row is full-width and the renderer owns the
    /// label (unbound property rows). `false` when a sibling
    /// `RowLabelWidget` paints the label next to the socket dot
    /// (bound input rows) — the renderer skips label paint to avoid
    /// double-drawing.
    show_label: bool,
    ctx: NodePaintContext,
}

impl ValueEditorWidget {
    pub(super) fn new(
        prop: PropLayout,
        node_w: f64,
        row_h: f64,
        ctx: NodePaintContext,
        show_label: bool,
    ) -> Self {
        // Full-width row when the renderer owns its own label.
        // Bound-input editors still get a narrow inset (they sit on
        // an input socket's row alongside the socket's `RowLabelWidget`).
        let s = ctx.scale;
        let inset_px = 1.0 * s;
        let (x, w) = if show_label {
            (inset_px, node_w - 2.0 * inset_px)
        } else {
            let width = prop.size[0] * s;
            let row_left = node_w - width - crate::draw::SOCKET_RADIUS * s;
            (row_left, width)
        };
        let bounds = Rect::new(x, inset_px, w, row_h - 2.0 * inset_px);
        Self {
            bounds,
            base: WidgetBase::new(),
            children: Vec::new(),
            prop,
            show_label,
            ctx,
        }
    }

    /// Translate the row's [`PropertyValue`] into the agg-gui
    /// [`RowValue`] borrow form so the dispatcher can paint it.
    fn row_value(&self) -> RowValue<'_> {
        match &self.prop.current {
            PropertyValue::Number(n) => RowValue::Number(*n),
            PropertyValue::Bool(b) => RowValue::Bool(*b),
            PropertyValue::Color(c) => RowValue::Color(*c),
            PropertyValue::Text(s) => RowValue::Text(s.as_str()),
            PropertyValue::Other { display } => RowValue::Display(display.as_str()),
        }
    }

    /// Resolve which [`EditorKind`] this row should paint with. Hosts
    /// that forward a full schema-side kind get exactly what they
    /// declared; rows without one fall back to a sensible default
    /// inferred from the value type.
    fn resolved_editor_kind(&self) -> EditorKind {
        if let Some(kind) = &self.prop.editor_kind {
            return kind.clone();
        }
        match &self.prop.current {
            PropertyValue::Number(_) => EditorKind::NumberDrag(Default::default()),
            PropertyValue::Bool(_) => EditorKind::Toggle,
            PropertyValue::Color(_) => EditorKind::ColorPicker,
            PropertyValue::Text(_) => EditorKind::StringSingleLine,
            PropertyValue::Other { .. } => EditorKind::Display,
        }
    }
}

impl Widget for ValueEditorWidget {
    fn type_name(&self) -> &'static str {
        "ValueEditorWidget"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("property", self.prop.name.clone()),
            ("label", self.prop.label().to_string()),
            ("editor_kind", format!("{:?}", self.resolved_editor_kind())),
        ]
    }
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let area = Rect::new(0.0, 0.0, w, h);
        let kind = self.resolved_editor_kind();
        let value = self.row_value();
        // Bound input rows already have a `RowLabelWidget` painting
        // the label next to the socket dot — skip label paint here
        // so we don't double-draw it. Unbound property rows own the
        // whole row width and ask for the label inside.
        if self.show_label {
            paint_row(ctx, area, self.prop.label(), value, &kind, self.ctx.scale);
        } else {
            paint_editor_only(ctx, area, value, &kind, self.ctx.scale);
        }
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        // Drag-edit dispatch still happens through `NodeEditor` because
        // canvas-space hit-testing already exists there.
        EventResult::Ignored
    }
}
