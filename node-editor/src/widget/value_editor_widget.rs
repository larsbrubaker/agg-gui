//! `ValueEditorWidget` — the inline number / colour / bool pill drawn
//! on an input or property row.  Extracted from `nodes.rs` to keep
//! that file under the project's 800-line cap.

use agg_gui::{Color, DrawCtx, Event, EventResult, Rect, Size, Widget, WidgetBase};

use super::node_paint_context::NodePaintContext;
use super::nodes::{LABEL_FONT_SIZE, ROW_PADDING_X};
use crate::draw::{PropLayout, SOCKET_RADIUS};
use crate::model::PropertyValue;

pub struct ValueEditorWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    prop: PropLayout,
    /// When `true` the editor draws its own row label on the left side —
    /// used for unbound property rows that don't have a sibling
    /// `RowLabelWidget`.
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
        // `node_w` / `row_h` are screen-space; the PropLayout still
        // carries canvas-space metrics, so we scale them to match.
        let s = ctx.scale;
        let width = prop.size[0] * s;
        let row_left = node_w - width - SOCKET_RADIUS * s;
        let inset_px = 1.0 * s;
        let (x, w) = if show_label {
            (inset_px, node_w - 2.0 * inset_px)
        } else {
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
            ("value", format_value(&self.prop.current)),
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
        let s = self.ctx.scale;
        let body = self.ctx.palette.node_body;
        let body_lum = 0.299 * body.r + 0.587 * body.g + 0.114 * body.b;
        let pill_bg = if body_lum < 0.5 {
            Color::rgba(0.15, 0.16, 0.20, 0.9)
        } else {
            Color::rgba(0.93, 0.93, 0.94, 0.9)
        };

        ctx.set_fill_color(pill_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 3.0 * s);
        ctx.fill();

        if let PropertyValue::Color(c) = &self.prop.current {
            let inset = 3.0 * s;
            ctx.set_fill_color(Color::rgba(c[0], c[1], c[2], c[3]));
            ctx.begin_path();
            ctx.rounded_rect(
                inset,
                inset,
                (w - 2.0 * inset).max(0.0),
                (h - 2.0 * inset).max(0.0),
                2.0 * s,
            );
            ctx.fill();
            return;
        }

        // Optional left-aligned label (only for unbound property rows).
        if self.show_label {
            ctx.set_fill_color(self.ctx.palette.label_text);
            ctx.set_font_size(LABEL_FONT_SIZE * s);
            ctx.fill_text(&self.prop.name, ROW_PADDING_X * s, h * 0.5 - 4.0 * s);
        }

        let value_str = format_value(&self.prop.current);
        if value_str.is_empty() {
            return;
        }
        ctx.set_fill_color(self.ctx.palette.label_text);
        ctx.set_font_size(LABEL_FONT_SIZE * s);
        let est = (value_str.len() as f64) * 6.0 * s;
        let x = (w - est - 6.0 * s).max(0.0);
        ctx.fill_text(&value_str, x, h * 0.5 - 4.0 * s);
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        // Drag-edit dispatch still happens through `NodeEditor` because
        // canvas-space hit-testing already exists there.
        EventResult::Ignored
    }
}

fn format_value(v: &PropertyValue) -> String {
    match v {
        PropertyValue::Number(n) => {
            if n.fract().abs() < 1e-6 {
                format!("{}", *n as i64)
            } else {
                format!("{:.3}", n)
            }
        }
        PropertyValue::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        PropertyValue::Color(_) => String::new(),
        PropertyValue::Other { display } => display.clone(),
    }
}
