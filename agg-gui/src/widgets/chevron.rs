//! `ChevronWidget` — a small clickable arrow that toggles a
//! collapsed/expanded state. Composes into title bars, accordion
//! headers, tree rows, anywhere a "fold" affordance is needed.
//!
//! The chevron is a real `Widget`: it has its own bounds, paints
//! itself, and consumes mouse-down events that land inside it. The
//! parent uses standard `children_mut()` + `layout()` to place it,
//! and supplies an `on_click` closure to act on the toggle. Parents
//! that need to share collapse state across multiple widgets pass an
//! `Rc<Cell<bool>>` for the chevron to read each paint — keeping a
//! single source of truth without copy-on-every-frame boilerplate.

use std::cell::Cell;
use std::rc::Rc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::WidgetBase;
use crate::widget::Widget;
use crate::widgets::window::chrome::paint_chevron;

/// Logical size of the chevron's hit / paint region. The arrow itself
/// is ~8 px wide; the surrounding padding gives the user a comfortable
/// click target.
pub const CHEVRON_SIZE: f64 = 16.0;

/// A clickable collapse / expand chevron.
pub struct ChevronWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    /// Shared collapse flag — the chevron reads this each paint to pick
    /// its glyph orientation. The parent writes it when the user (or
    /// any other code path) toggles the fold.
    collapsed: Rc<Cell<bool>>,
    /// Shared glyph colour cell — the parent writes the theme colour
    /// each paint pass; the chevron reads it without needing a typed
    /// downcast through the children Vec. Defaults to white so a
    /// caller that never wires the cell still gets a visible glyph.
    color: Rc<Cell<Color>>,
    /// Invoked on left-click. Parents put their toggle logic in here.
    on_click: Option<Box<dyn FnMut()>>,
}

impl ChevronWidget {
    /// Build a chevron sharing `collapsed` with its parent. The parent
    /// is the source of truth — the chevron only renders + emits clicks.
    pub fn new(collapsed: Rc<Cell<bool>>) -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, CHEVRON_SIZE, CHEVRON_SIZE),
            base: WidgetBase::new(),
            children: Vec::new(),
            collapsed,
            color: Rc::new(Cell::new(Color::white())),
            on_click: None,
        }
    }

    /// Wire a left-click handler. Typical implementations flip the
    /// shared `collapsed` cell and request a redraw / notify their
    /// owner. Builder form — chain at construction.
    pub fn on_click(mut self, f: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }

    /// Hand a shared colour cell to the chevron. The parent keeps a
    /// clone of the returned cell and writes the active theme colour
    /// into it each paint pass; the chevron picks it up automatically.
    pub fn with_color_cell(mut self, c: Rc<Cell<Color>>) -> Self {
        self.color = c;
        self
    }
}

impl Widget for ChevronWidget {
    fn type_name(&self) -> &'static str {
        "ChevronWidget"
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

    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let cx = self.bounds.width * 0.5;
        let cy = self.bounds.height * 0.5;
        paint_chevron(ctx, cx, cy, self.collapsed.get(), self.color.get());
    }

    fn hit_test(&self, local: Point) -> bool {
        local.x >= 0.0
            && local.x <= self.bounds.width
            && local.y >= 0.0
            && local.y <= self.bounds.height
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown {
            button: MouseButton::Left,
            ..
        } = event
        {
            if let Some(cb) = self.on_click.as_mut() {
                cb();
            }
            crate::animation::request_draw();
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}
