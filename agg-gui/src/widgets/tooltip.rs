//! `Tooltip` — a wrapper widget that shows egui-style hover help.
//!
//! Tooltips are submitted during the normal widget paint pass, but drawn at the
//! end of the frame by [`crate::widget::App`].  That makes them true floating
//! overlays instead of child-local decorations, so they can escape scroll-area
//! clips and window content clips.
//!
//! # Usage
//!
//! ```ignore
//! Tooltip::new(
//!     Box::new(Button::new("Hover me", font.clone()).on_click(|| {})),
//!     "This is a tooltip",
//!     font.clone(),
//! )
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{current_mouse_world, Widget};

/// Number of consecutive hovered frames before the tooltip appears.
/// At ~60 fps this gives an egui-like short hover delay.
const HOVER_DELAY_FRAMES: u32 = 18;
const TOOLTIP_FONT_SIZE: f64 = 12.0;
const TOOLTIP_PAD_X: f64 = 8.0;
const TOOLTIP_PAD_Y: f64 = 6.0;
const TOOLTIP_GAP: f64 = 4.0;
const SCREEN_MARGIN: f64 = 4.0;

#[derive(Clone)]
enum TooltipLineKind {
    Text,
    Code,
    Link,
}

#[derive(Clone)]
struct TooltipLine {
    text: String,
    kind: TooltipLineKind,
}

struct TooltipRequest {
    font: Arc<Font>,
    lines: Vec<TooltipLine>,
    anchor: Point,
    at_pointer: bool,
}

thread_local! {
    static TOOLTIP_QUEUE: RefCell<Vec<TooltipRequest>> = const { RefCell::new(Vec::new()) };
}

/// A wrapper widget that shows a text tooltip on hover.
pub struct Tooltip {
    bounds: Rect,
    /// The wrapped child widget is stored in `children[0]`.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,

    /// Hover-frame counter: increments while cursor is over the child.
    hover_frames: u32,
    /// Whether the cursor is currently inside the widget bounds.
    hovered: bool,
    /// Last known cursor position in local coordinates.
    cursor: Point,

    font: Arc<Font>,
    lines: Vec<TooltipLine>,
    disabled_lines: Vec<TooltipLine>,
    disabled_when: Option<Rc<dyn Fn() -> bool>>,
    at_pointer: bool,
}

impl Tooltip {
    /// Create a new `Tooltip` wrapping `child` with `text` as the tip message.
    pub fn new(child: Box<dyn Widget>, text: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            hover_frames: 0,
            hovered: false,
            cursor: Point::ORIGIN,
            font,
            lines: text_to_lines(text),
            disabled_lines: Vec::new(),
            disabled_when: None,
            at_pointer: false,
        }
    }

    /// Add another hover text block, matching egui's ability to chain
    /// `.on_hover_text(...)` calls.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.lines.extend(text_to_lines(text));
        self
    }

    /// Add a code-styled line to the tooltip.
    pub fn with_code_line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(TooltipLine {
            text: text.into(),
            kind: TooltipLineKind::Code,
        });
        self
    }

    /// Add a link-styled line to the tooltip.  Tooltip overlays are
    /// informational; the line is styled like a link but does not receive
    /// pointer events.
    pub fn with_link_line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(TooltipLine {
            text: text.into(),
            kind: TooltipLineKind::Link,
        });
        self
    }

    /// Place the tooltip relative to the mouse cursor instead of the widget.
    pub fn at_pointer(mut self) -> Self {
        self.at_pointer = true;
        self
    }

    /// Use alternate tooltip text while `disabled_when` returns true.
    pub fn with_disabled_text(
        mut self,
        text: impl Into<String>,
        disabled_when: impl Fn() -> bool + 'static,
    ) -> Self {
        self.disabled_lines = text_to_lines(text);
        self.disabled_when = Some(Rc::new(disabled_when));
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

    fn show_tip(&self) -> bool {
        self.hovered && self.hover_frames >= HOVER_DELAY_FRAMES
    }

    fn active_lines(&self) -> Vec<TooltipLine> {
        if self.disabled_when.as_ref().map(|f| f()).unwrap_or(false)
            && !self.disabled_lines.is_empty()
        {
            self.disabled_lines.clone()
        } else {
            self.lines.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::MouseButton;
    use crate::text::Font;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");

    struct ClickChild {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        clicks: Arc<AtomicUsize>,
    }

    impl ClickChild {
        fn new(clicks: Arc<AtomicUsize>) -> Self {
            Self { bounds: Rect::default(), children: Vec::new(), clicks }
        }
    }

    impl Widget for ClickChild {
        fn bounds(&self) -> Rect { self.bounds }
        fn set_bounds(&mut self, bounds: Rect) { self.bounds = bounds; }
        fn children(&self) -> &[Box<dyn Widget>] { &self.children }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
        fn type_name(&self) -> &'static str { "ClickChild" }
        fn layout(&mut self, available: Size) -> Size {
            self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
            available
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, event: &Event) -> EventResult {
            if let Event::MouseUp { button: MouseButton::Left, .. } = event {
                self.clicks.fetch_add(1, Ordering::SeqCst);
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
    }

    #[test]
    fn tooltip_forwards_clicks_to_wrapped_child() {
        let clicks = Arc::new(AtomicUsize::new(0));
        let font = Arc::new(Font::from_bytes(FONT_BYTES.to_vec()).expect("bundled font"));
        let mut tooltip = Tooltip::new(Box::new(ClickChild::new(clicks.clone())), "tip", font);
        tooltip.layout(Size::new(20.0, 20.0));
        let event = Event::MouseUp {
            pos: Point::new(10.0, 10.0),
            button: MouseButton::Left,
            modifiers: Default::default(),
        };
        assert_eq!(tooltip.on_event(&event), EventResult::Consumed);
        assert_eq!(clicks.load(Ordering::SeqCst), 1);
    }
}

impl Widget for Tooltip {
    fn type_name(&self) -> &'static str {
        "Tooltip"
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

    fn is_focusable(&self) -> bool {
        self.children
            .first()
            .map(|c| c.is_focusable())
            .unwrap_or(false)
    }

    fn layout(&mut self, available: Size) -> Size {
        let s = if let Some(child) = self.children.first_mut() {
            let cs = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, cs.width, cs.height));
            cs
        } else {
            available
        };
        self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
        s
    }

    fn paint(&mut self, _: &mut dyn DrawCtx) {}

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if self.hovered {
            self.hover_frames = self.hover_frames.saturating_add(1);
            if !self.show_tip() {
                crate::animation::request_draw();
            }
        }

        if !self.show_tip() {
            return;
        }

        let mut anchor = if self.at_pointer {
            current_mouse_world().unwrap_or(self.cursor)
        } else {
            let mut x = self.bounds.width * 0.5;
            let mut y = self.bounds.height;
            ctx.root_transform().transform(&mut x, &mut y);
            Point::new(x, y)
        };
        if self.at_pointer {
            anchor.x += 14.0;
            anchor.y += 14.0;
        }

        submit_tooltip(TooltipRequest {
            font: Arc::clone(&self.font),
            lines: self.active_lines(),
            anchor,
            at_pointer: self.at_pointer,
        });
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                self.cursor = *pos;
                if !self.hovered {
                    self.hover_frames = 0;
                }
                if self.hovered != was {
                    crate::animation::request_draw();
                }
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            Event::MouseWheel { .. } => {
                self.hovered = false;
                self.hover_frames = 0;
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            _ => self
                .children
                .first_mut()
                .map(|child| child.on_event(event))
                .unwrap_or(EventResult::Ignored),
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

fn text_to_lines(text: impl Into<String>) -> Vec<TooltipLine> {
    text.into()
        .lines()
        .map(|line| TooltipLine {
            text: line.to_owned(),
            kind: TooltipLineKind::Text,
        })
        .collect()
}

fn submit_tooltip(request: TooltipRequest) {
    TOOLTIP_QUEUE.with(|q| q.borrow_mut().push(request));
}

pub(crate) fn begin_tooltip_frame() {
    TOOLTIP_QUEUE.with(|q| q.borrow_mut().clear());
}

pub(crate) fn paint_global_tooltips(ctx: &mut dyn DrawCtx, viewport: Size) {
    let requests = TOOLTIP_QUEUE.with(|q| q.borrow_mut().drain(..).collect::<Vec<_>>());
    for request in requests {
        paint_request(ctx, viewport, request);
    }
}

fn paint_request(ctx: &mut dyn DrawCtx, viewport: Size, request: TooltipRequest) {
    if request.lines.is_empty() {
        return;
    }

    let v = ctx.visuals();
    ctx.set_font(Arc::clone(&request.font));
    ctx.set_font_size(TOOLTIP_FONT_SIZE);

    let line_h = TOOLTIP_FONT_SIZE * 1.45;
    let mut max_w = 0.0_f64;
    for line in &request.lines {
        if let Some(m) = ctx.measure_text(&line.text) {
            max_w = max_w.max(m.width);
        }
    }

    let panel_w = (max_w + TOOLTIP_PAD_X * 2.0).max(64.0);
    let panel_h = request.lines.len() as f64 * line_h + TOOLTIP_PAD_Y * 2.0;
    let mut panel_x = if request.at_pointer {
        request.anchor.x
    } else {
        request.anchor.x - panel_w * 0.5
    };
    let mut panel_y = request.anchor.y + TOOLTIP_GAP;

    if panel_x + panel_w > viewport.width - SCREEN_MARGIN {
        panel_x = viewport.width - panel_w - SCREEN_MARGIN;
    }
    if panel_y + panel_h > viewport.height - SCREEN_MARGIN {
        panel_y = request.anchor.y - panel_h - TOOLTIP_GAP * 3.0;
    }
    panel_x = panel_x.clamp(
        SCREEN_MARGIN,
        (viewport.width - panel_w - SCREEN_MARGIN).max(SCREEN_MARGIN),
    );
    panel_y = panel_y.clamp(
        SCREEN_MARGIN,
        (viewport.height - panel_h - SCREEN_MARGIN).max(SCREEN_MARGIN),
    );

    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.20));
    ctx.begin_path();
    ctx.rounded_rect(panel_x + 1.0, panel_y - 1.0, panel_w, panel_h, 5.0);
    ctx.fill();

    ctx.set_fill_color(v.window_fill);
    ctx.begin_path();
    ctx.rounded_rect(panel_x, panel_y, panel_w, panel_h, 5.0);
    ctx.fill();

    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(panel_x, panel_y, panel_w, panel_h, 5.0);
    ctx.stroke();

    for (i, line) in request.lines.iter().enumerate() {
        let y = panel_y + panel_h - TOOLTIP_PAD_Y - (i as f64 + 1.0) * line_h + 2.0;
        match line.kind {
            TooltipLineKind::Text => {
                ctx.set_fill_color(v.text_color);
                ctx.fill_text(&line.text, panel_x + TOOLTIP_PAD_X, y);
            }
            TooltipLineKind::Code => {
                if let Some(m) = ctx.measure_text(&line.text) {
                    ctx.set_fill_color(v.track_bg);
                    ctx.begin_path();
                    ctx.rounded_rect(
                        panel_x + TOOLTIP_PAD_X - 3.0,
                        y - 3.0,
                        m.width + 6.0,
                        line_h,
                        3.0,
                    );
                    ctx.fill();
                }
                ctx.set_fill_color(v.text_color);
                ctx.fill_text(&line.text, panel_x + TOOLTIP_PAD_X, y);
            }
            TooltipLineKind::Link => {
                ctx.set_fill_color(v.text_link);
                ctx.fill_text(&line.text, panel_x + TOOLTIP_PAD_X, y);
                if let Some(m) = ctx.measure_text(&line.text) {
                    ctx.set_stroke_color(v.text_link);
                    ctx.set_line_width(1.0);
                    ctx.begin_path();
                    ctx.move_to(panel_x + TOOLTIP_PAD_X, y - 2.0);
                    ctx.line_to(panel_x + TOOLTIP_PAD_X + m.width, y - 2.0);
                    ctx.stroke();
                }
            }
        }
    }
}
