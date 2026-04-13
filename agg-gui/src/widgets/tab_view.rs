//! `TabView` — a tabbed container with a clickable tab bar.
//!
//! An optional action button can be placed at the right end of the tab bar
//! via [`with_action_button`].  The tab labels divide the remaining width.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::primitives::Spacer;

const ACTION_BTN_W: f64 = 100.0;

/// A tabbed panel container.
pub struct TabView {
    bounds: Rect,
    /// The active tab's content widget. At most 1 element.
    children: Vec<Box<dyn Widget>>,
    /// Storage for all tab widgets. The active tab's slot holds a Spacer placeholder.
    tab_contents: Vec<Box<dyn Widget>>,
    tab_labels: Vec<String>,
    active_tab: usize,
    tab_bar_height: f64,
    font: Arc<Font>,
    font_size: f64,
    hovered_tab: Option<usize>,
    // Optional right-side action button
    action_label: Option<String>,
    action_hovered: bool,
    on_action: Option<Box<dyn Fn()>>,
    action_active: bool,   // toggle state for visual feedback
}

impl TabView {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            tab_contents: Vec::new(),
            tab_labels: Vec::new(),
            active_tab: 0,
            tab_bar_height: 36.0,
            font,
            font_size: 13.0,
            hovered_tab: None,
            action_label: None,
            action_hovered: false,
            on_action: None,
            action_active: false,
        }
    }

    pub fn with_tab_bar_height(mut self, h: f64) -> Self {
        self.tab_bar_height = h;
        self
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    /// Add an action button at the right end of the tab bar.
    /// Clicking it fires `on_click` and toggles the visual active state.
    pub fn with_action_button(
        mut self,
        label: impl Into<String>,
        on_click: impl Fn() + 'static,
    ) -> Self {
        self.action_label = Some(label.into());
        self.on_action = Some(Box::new(on_click));
        self
    }

    /// Update the visual active (pressed/on) state of the action button.
    /// Call this each frame to reflect the inspector's current visibility.
    pub fn set_action_active(&mut self, active: bool) {
        self.action_active = active;
    }

    /// Add a tab with a label and its content widget.
    pub fn add_tab(mut self, label: impl Into<String>, content: Box<dyn Widget>) -> Self {
        let idx = self.tab_labels.len();
        self.tab_labels.push(label.into());
        if idx == 0 {
            self.children.push(content);
            self.tab_contents.push(Box::new(Spacer::new()));
        } else {
            self.tab_contents.push(content);
        }
        self
    }

    fn switch_to(&mut self, new_idx: usize) {
        if new_idx == self.active_tab || new_idx >= self.tab_labels.len() { return; }
        if let Some(current) = self.children.pop() {
            self.tab_contents[self.active_tab] = current;
        }
        let placeholder: Box<dyn Widget> = Box::new(Spacer::new());
        let new_child = std::mem::replace(&mut self.tab_contents[new_idx], placeholder);
        self.children.push(new_child);
        self.active_tab = new_idx;
    }

    fn content_height(&self) -> f64 {
        (self.bounds.height - self.tab_bar_height).max(0.0)
    }

    /// Width available to the tab strip (excludes action button area).
    fn tabs_width(&self) -> f64 {
        if self.action_label.is_some() {
            (self.bounds.width - ACTION_BTN_W).max(0.0)
        } else {
            self.bounds.width
        }
    }

    fn tab_index_at(&self, pos: Point) -> Option<usize> {
        if pos.y < self.content_height() { return None; }
        // Exclude action button zone on the right
        if pos.x >= self.tabs_width() { return None; }
        let n = self.tab_labels.len().max(1);
        let tab_w = self.tabs_width() / n as f64;
        let i = (pos.x / tab_w) as usize;
        if i < self.tab_labels.len() { Some(i) } else { None }
    }

    fn action_btn_hit(&self, pos: Point) -> bool {
        self.action_label.is_some()
            && pos.y >= self.content_height()
            && pos.x >= self.tabs_width()
    }
}

impl Widget for TabView {
    fn type_name(&self) -> &'static str { "TabView" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let content_h = (available.height - self.tab_bar_height).max(0.0);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(available.width, content_h));
            child.set_bounds(Rect::new(0.0, 0.0, available.width, content_h));
        }
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let tab_h = self.tab_bar_height;
        let content_h = self.content_height();
        let tabs_w = self.tabs_width();
        let n = self.tab_labels.len().max(1);
        let tab_w = tabs_w / n as f64;
        let bar_y = content_h;

        // Tab bar background
        ctx.set_fill_color(Color::rgb(0.97, 0.97, 0.98));
        ctx.begin_path();
        ctx.rect(0.0, bar_y, w, tab_h);
        ctx.fill();

        // Bottom separator line
        ctx.set_stroke_color(Color::rgba(0.0, 0.0, 0.0, 0.12));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, bar_y);
        ctx.line_to(w, bar_y);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);

        // Tab labels
        for (i, label) in self.tab_labels.iter().enumerate() {
            let tx = i as f64 * tab_w;
            let is_active  = i == self.active_tab;
            let is_hovered = self.hovered_tab == Some(i);

            if is_hovered && !is_active {
                ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.04));
                ctx.begin_path();
                ctx.rect(tx, bar_y, tab_w, tab_h);
                ctx.fill();
            }
            if is_active {
                ctx.set_fill_color(Color::rgb(0.22, 0.45, 0.88));
                ctx.begin_path();
                ctx.rect(tx, h - 2.5, tab_w, 2.5);
                ctx.fill();
            }
            let label_color = if is_active {
                Color::rgb(0.22, 0.45, 0.88)
            } else if is_hovered {
                Color::rgb(0.3, 0.3, 0.35)
            } else {
                Color::rgba(0.0, 0.0, 0.0, 0.55)
            };
            ctx.set_fill_color(label_color);
            if let Some(m) = ctx.measure_text(label) {
                let lx = tx + (tab_w - m.width) * 0.5;
                let ly = bar_y + (tab_h - (m.ascent + m.descent)) * 0.5 + m.descent;
                ctx.fill_text(label, lx, ly);
            }
        }

        // Action button (right side of tab bar)
        if let Some(ref label) = self.action_label.clone() {
            let bx = tabs_w;
            let bg = if self.action_active {
                Color::rgba(0.22, 0.45, 0.88, 0.18)
            } else if self.action_hovered {
                Color::rgba(0.0, 0.0, 0.0, 0.06)
            } else {
                Color::transparent()
            };
            if bg.a > 0.0 {
                ctx.set_fill_color(bg);
                ctx.begin_path();
                ctx.rect(bx, bar_y, ACTION_BTN_W, tab_h);
                ctx.fill();
            }
            // Left separator
            ctx.set_stroke_color(Color::rgba(0.0, 0.0, 0.0, 0.10));
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(bx, bar_y + 6.0);
            ctx.line_to(bx, h - 6.0);
            ctx.stroke();

            let lc = if self.action_active {
                Color::rgb(0.22, 0.45, 0.88)
            } else {
                Color::rgba(0.0, 0.0, 0.0, 0.60)
            };
            ctx.set_fill_color(lc);
            if let Some(m) = ctx.measure_text(label) {
                let lx = bx + (ACTION_BTN_W - m.width) * 0.5;
                let ly = bar_y + (tab_h - (m.ascent + m.descent)) * 0.5 + m.descent;
                ctx.fill_text(label, lx, ly);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered_tab    = self.tab_index_at(*pos);
                self.action_hovered = self.action_btn_hit(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if self.action_btn_hit(*pos) {
                    self.action_active = !self.action_active;
                    if let Some(ref cb) = self.on_action {
                        cb();
                    }
                    return EventResult::Consumed;
                }
                if let Some(i) = self.tab_index_at(*pos) {
                    self.switch_to(i);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}
