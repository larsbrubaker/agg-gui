//! [`BrowserSplitter`]: the browser's left|right split, the folder tree on the left and the list
//! on the right. Unlike agg-gui's ratio splitter it keeps the left panel a fixed width when the
//! window resizes, so the tree stays as wide as the user left it; dragging the bar moves that
//! width and remembers it when the drag ends (`BrowserMemory::tree_width`, in the app's own
//! store). The bar is [`SPLITTER_WIDTH`] wide in the look's splitter colour, and never lets the
//! tree go narrower than [`MIN_TREE_WIDTH`]. A drag that loses the pointer without a release (a
//! finger lifting) ends as a release does.

use std::rc::Rc;

use agg_gui::cursor::{set_cursor_icon, CursorIcon};
use agg_gui::{Color, DrawCtx, Event, EventResult, MouseButton, Rect, Size, Widget};

use crate::memory::BrowserMemory;

/// The bar's width.
pub const SPLITTER_WIDTH: f64 = 6.0;
/// The tree's width before the user moves the bar, in device pixels (divided by the device
/// scale for the logical width).
pub const DEFAULT_TREE_WIDTH: f64 = 254.0;
/// The narrowest the tree can be dragged.
pub const MIN_TREE_WIDTH: f64 = 20.0;

/// The split with the tree on the left and the list on the right (see the module header).
pub struct BrowserSplitter {
    distance: f64,
    bar_color: Color,
    memory: Rc<dyn BrowserMemory>,
    name: String,
    /// The pointer's x at the press and the distance then, while the bar drags.
    drag: Option<(f64, f64)>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl BrowserSplitter {
    /// `left` and `right` split by a bar named `name` in `bar_color`, the left as wide as `memory`
    /// remembers (else the default).
    pub fn new(
        left: Box<dyn Widget>,
        right: Box<dyn Widget>,
        bar_color: Color,
        memory: Rc<dyn BrowserMemory>,
        name: &str,
    ) -> Self {
        let scale = agg_gui::device_scale::device_scale();
        let distance = memory.tree_width().unwrap_or(DEFAULT_TREE_WIDTH / scale);
        Self {
            distance,
            bar_color,
            memory,
            name: name.to_string(),
            drag: None,
            bounds: Rect::default(),
            children: vec![left, right],
        }
    }

    /// The left panel's width.
    pub fn distance(&self) -> f64 {
        self.distance
    }

    fn on_bar(&self, x: f64) -> bool {
        x >= self.distance && x <= self.distance + SPLITTER_WIDTH
    }
}

impl Widget for BrowserSplitter {
    fn type_name(&self) -> &'static str {
        "Splitter"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let left = self.distance.min(available.width - SPLITTER_WIDTH).max(0.0);
        let right_x = left + SPLITTER_WIDTH;
        let right = (available.width - right_x).max(0.0);
        if let [first, second] = self.children.as_mut_slice() {
            first.layout(Size::new(left, available.height));
            first.set_bounds(Rect::new(0.0, 0.0, left, available.height));
            second.layout(Size::new(right, available.height));
            second.set_bounds(Rect::new(right_x, 0.0, right, available.height));
        }
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(self.bar_color);
        ctx.begin_path();
        ctx.rect(self.distance, 0.0, SPLITTER_WIDTH, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } if self.on_bar(pos.x) => {
                self.drag = Some((pos.x, self.distance));
                set_cursor_icon(CursorIcon::ResizeHorizontal);
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                if let Some((start, distance)) = self.drag {
                    let max = (self.bounds.width - SPLITTER_WIDTH).max(MIN_TREE_WIDTH);
                    self.distance = (distance + pos.x - start).clamp(MIN_TREE_WIDTH, max);
                    agg_gui::animation::request_layout();
                }
                if self.drag.is_some() || self.on_bar(pos.x) {
                    set_cursor_icon(CursorIcon::ResizeHorizontal);
                }
                match self.drag.is_some() {
                    true => EventResult::Consumed,
                    false => EventResult::Ignored,
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            }
            | Event::MouseCaptureLost
                if self.drag.is_some() =>
            {
                self.drag = None;
                self.memory.set_tree_width(self.distance);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("distance", self.distance.to_string())]
    }
}
