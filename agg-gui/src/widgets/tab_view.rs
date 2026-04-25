//! `TabView` — a tabbed container with a clickable tab bar.
//!
//! An optional action button can be placed at the right end of the tab bar.
//! An optional sidebar widget can be shown to the right of the content area
//! via [`with_sidebar`], separated by a draggable vertical divider.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::primitives::Spacer;

const ACTION_BTN_W: f64 = 100.0;
const DIVIDER_W: f64 = 6.0;
const MIN_SIDEBAR_W: f64 = 160.0;

/// A tabbed panel container.
///
/// `children[0]` = active tab content.
/// `children[1]` = sidebar widget (optional, always stored even when hidden).
pub struct TabView {
    bounds: Rect,
    /// children[0]=active content, children[1]=sidebar (if any)
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    tab_contents: Vec<Box<dyn Widget>>,
    tab_labels: Vec<String>,
    active_tab: usize,
    tab_bar_height: f64,
    font: Arc<Font>,
    font_size: f64,
    hovered_tab: Option<usize>,
    action_label: Option<String>,
    action_hovered: bool,
    on_action: Option<Box<dyn Fn()>>,
    action_active: bool,
    // Sidebar state
    show_sidebar: Option<Rc<Cell<bool>>>,
    sidebar_w: f64,
    sidebar_dragging: bool,
    /// When set, writes `active_tab` on every tab switch AND is re-read
    /// each layout so external code (state persistence) can drive the
    /// selection too.  Pattern mirrors ScrollView / ToggleSwitch cells.
    active_tab_cell: Option<Rc<Cell<usize>>>,
}

impl TabView {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
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
            show_sidebar: None,
            sidebar_w: 320.0,
            sidebar_dragging: false,
            active_tab_cell: None,
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

    /// Bind the active tab index to a shared cell.  The cell's current
    /// value seeds the initial selection on the next layout (so a
    /// persisted choice rehydrates); later user clicks write back
    /// through the cell.
    pub fn with_active_tab_cell(mut self, cell: Rc<Cell<usize>>) -> Self {
        self.active_tab_cell = Some(cell);
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

    /// Add an action button at the right end of the tab bar.
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
    pub fn set_action_active(&mut self, active: bool) {
        self.action_active = active;
    }

    /// Add a tab with a label and its content widget.
    pub fn add_tab(mut self, label: impl Into<String>, content: Box<dyn Widget>) -> Self {
        let idx = self.tab_labels.len();
        self.tab_labels.push(label.into());
        if idx == 0 {
            // Content always lives at children[0].
            self.children.insert(0, content);
            self.tab_contents.push(Box::new(Spacer::new()));
        } else {
            self.tab_contents.push(content);
        }
        self
    }

    /// Attach a sidebar widget shown to the right of the content area when
    /// `show.get()` is true.  The divider between content and sidebar is
    /// user-draggable.  Call this AFTER all `add_tab` calls.
    pub fn with_sidebar(mut self, widget: Box<dyn Widget>, show: Rc<Cell<bool>>) -> Self {
        self.show_sidebar = Some(show);
        self.children.push(widget); // sidebar always at children[1]
        self
    }

    // ── private helpers ───────────────────────────────────────────────────────

    fn sidebar_showing(&self) -> bool {
        self.show_sidebar.as_ref().map(|s| s.get()).unwrap_or(false)
    }

    fn content_height(&self) -> f64 {
        (self.bounds.height - self.tab_bar_height).max(0.0)
    }

    fn tabs_width(&self) -> f64 {
        if self.action_label.is_some() {
            (self.bounds.width - ACTION_BTN_W).max(0.0)
        } else {
            self.bounds.width
        }
    }

    /// X position of the vertical divider (in content-area local coords).
    fn divider_x(&self) -> f64 {
        (self.bounds.width - self.sidebar_w - DIVIDER_W).max(0.0)
    }

    fn tab_index_at(&self, pos: Point) -> Option<usize> {
        if pos.y < self.content_height() {
            return None;
        }
        if pos.x >= self.tabs_width() {
            return None;
        }
        let n = self.tab_labels.len().max(1);
        let tab_w = self.tabs_width() / n as f64;
        let i = (pos.x / tab_w) as usize;
        if i < self.tab_labels.len() {
            Some(i)
        } else {
            None
        }
    }

    fn action_btn_hit(&self, pos: Point) -> bool {
        self.action_label.is_some() && pos.y >= self.content_height() && pos.x >= self.tabs_width()
    }

    fn switch_to(&mut self, new_idx: usize) {
        if new_idx == self.active_tab || new_idx >= self.tab_labels.len() {
            return;
        }
        // children layout: [content, sidebar?]
        // Pop sidebar first (index 1), then pop content (index 0).
        let old_sidebar = if self.children.len() > 1 {
            self.children.pop()
        } else {
            None
        };
        if let Some(current) = self.children.pop() {
            self.tab_contents[self.active_tab] = current;
        }
        let placeholder: Box<dyn Widget> = Box::new(Spacer::new());
        let new_child = std::mem::replace(&mut self.tab_contents[new_idx], placeholder);
        self.children.push(new_child); // content at index 0
        if let Some(s) = old_sidebar {
            self.children.push(s);
        } // sidebar at index 1
        self.active_tab = new_idx;
        if let Some(cell) = &self.active_tab_cell {
            cell.set(new_idx);
        }
    }
}

impl Widget for TabView {
    fn type_name(&self) -> &'static str {
        "TabView"
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

    fn layout(&mut self, available: Size) -> Size {
        // Honour a persisted tab selection.  Done here (rather than in
        // `new`) because `add_tab` runs after `new`, so the child vector
        // isn't populated until the builder chain is complete.
        if let Some(cell) = self.active_tab_cell.clone() {
            let want = cell.get();
            if want != self.active_tab && want < self.tab_labels.len() {
                self.switch_to(want);
            }
        }
        let content_h = (available.height - self.tab_bar_height).max(0.0);
        let showing = self.sidebar_showing();
        let sw = if showing {
            self.sidebar_w.clamp(MIN_SIDEBAR_W, available.width * 0.8)
        } else {
            0.0
        };
        let content_w = if showing {
            (available.width - sw - DIVIDER_W).max(0.0)
        } else {
            available.width
        };

        // Content at children[0]
        if let Some(child) = self.children.get_mut(0) {
            child.layout(Size::new(content_w, content_h));
            child.set_bounds(Rect::new(0.0, 0.0, content_w, content_h));
        }
        // Sidebar at children[1]
        if let Some(sidebar) = self.children.get_mut(1) {
            if showing {
                sidebar.layout(Size::new(sw, content_h));
                sidebar.set_bounds(Rect::new(content_w + DIVIDER_W, 0.0, sw, content_h));
            } else {
                sidebar.layout(Size::new(0.0, 0.0));
                // Place off-screen so hit_test never fires
                sidebar.set_bounds(Rect::new(available.width + 1.0, 0.0, 0.0, 0.0));
            }
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

        let v = ctx.visuals();

        // Tab bar background
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, bar_y, w, tab_h);
        ctx.fill();

        // Bottom separator line
        ctx.set_stroke_color(v.separator);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, bar_y);
        ctx.line_to(w, bar_y);
        ctx.stroke();

        // Honour the thread-local system-font override so changes in the
        // System window re-style tab titles live.
        let font =
            crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font));
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(self.font_size);

        // Tab labels
        for (i, label) in self.tab_labels.iter().enumerate() {
            let tx = i as f64 * tab_w;
            let is_active = i == self.active_tab;
            let is_hovered = self.hovered_tab == Some(i);

            if is_hovered && !is_active {
                ctx.set_fill_color(v.widget_bg_hovered);
                ctx.begin_path();
                ctx.rect(tx, bar_y, tab_w, tab_h);
                ctx.fill();
            }
            if is_active {
                ctx.set_fill_color(v.accent);
                ctx.begin_path();
                ctx.rect(tx, h - 2.5, tab_w, 2.5);
                ctx.fill();
            }
            let label_color = if is_active {
                v.accent
            } else if is_hovered {
                v.text_color
            } else {
                v.text_dim
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
                Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.18)
            } else if self.action_hovered {
                v.widget_bg_hovered
            } else {
                Color::transparent()
            };
            if bg.a > 0.0 {
                ctx.set_fill_color(bg);
                ctx.begin_path();
                ctx.rect(bx, bar_y, ACTION_BTN_W, tab_h);
                ctx.fill();
            }
            ctx.set_stroke_color(v.separator);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(bx, bar_y + 6.0);
            ctx.line_to(bx, h - 6.0);
            ctx.stroke();

            let lc = if self.action_active {
                v.accent
            } else {
                v.text_dim
            };
            ctx.set_fill_color(lc);
            if let Some(m) = ctx.measure_text(label) {
                let lx = bx + (ACTION_BTN_W - m.width) * 0.5;
                let ly = bar_y + (tab_h - (m.ascent + m.descent)) * 0.5 + m.descent;
                ctx.fill_text(label, lx, ly);
            }
        }

        // Vertical sidebar divider (painted in content area, under children)
        if self.sidebar_showing() {
            let div_x = self.divider_x();
            let div_color = if self.sidebar_dragging {
                Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.55)
            } else {
                v.separator
            };
            ctx.set_fill_color(div_color);
            ctx.begin_path();
            ctx.rect(div_x, 0.0, DIVIDER_W, content_h);
            ctx.fill();

            // Grip dots
            if content_h > 30.0 {
                let grip = if self.sidebar_dragging {
                    Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.8)
                } else {
                    v.text_dim
                };
                ctx.set_fill_color(grip);
                let cx = div_x + DIVIDER_W * 0.5;
                let cy = content_h * 0.5;
                for i in -1i32..=1 {
                    ctx.begin_path();
                    ctx.circle(cx, cy + i as f64 * 5.0, 1.5);
                    ctx.fill();
                }
            }
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        // Capture all mouse events during sidebar drag, even if cursor leaves bounds.
        if self.sidebar_dragging {
            return true;
        }
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was_tab = self.hovered_tab;
                let was_act = self.action_hovered;
                self.hovered_tab = self.tab_index_at(*pos);
                self.action_hovered = self.action_btn_hit(*pos);
                if self.sidebar_dragging {
                    // Resize: sidebar_w = window_width - cursor_x - divider
                    let new_w = self.bounds.width - pos.x;
                    self.sidebar_w = new_w.clamp(MIN_SIDEBAR_W, self.bounds.width * 0.8);
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }
                if was_tab != self.hovered_tab || was_act != self.action_hovered {
                    crate::animation::request_tick();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                if self.action_btn_hit(*pos) {
                    self.action_active = !self.action_active;
                    if let Some(ref cb) = self.on_action {
                        cb();
                    }
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }
                // Divider drag — only in the content area (y < content_h)
                if self.sidebar_showing() && pos.y < self.content_height() {
                    let div_x = self.divider_x();
                    if pos.x >= div_x - 2.0 && pos.x <= div_x + DIVIDER_W + 2.0 {
                        self.sidebar_dragging = true;
                        crate::animation::request_tick();
                        return EventResult::Consumed;
                    }
                }
                if let Some(i) = self.tab_index_at(*pos) {
                    self.switch_to(i);
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.sidebar_dragging {
                    self.sidebar_dragging = false;
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}
