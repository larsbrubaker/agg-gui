//! Unit tests for [`super::Tooltip`] — click forwarding, anchoring, and the
//! interactive-tip open/close contract.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::event::{Key, Modifiers, MouseButton};
use crate::text::Font;
use std::sync::atomic::{AtomicUsize, Ordering};
use web_time::Instant;

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_bytes(FONT_BYTES.to_vec()).expect("bundled font"))
}

struct ClickChild {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    clicks: Arc<AtomicUsize>,
}

impl ClickChild {
    fn new(clicks: Arc<AtomicUsize>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            clicks,
        }
    }
}

impl Widget for ClickChild {
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
    fn type_name(&self) -> &'static str {
        "ClickChild"
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseUp {
            button: MouseButton::Left,
            ..
        } = event
        {
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
    let mut tooltip = Tooltip::new(Box::new(ClickChild::new(clicks.clone())), "tip", test_font());
    tooltip.layout(Size::new(20.0, 20.0));
    let event = Event::MouseUp {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Left,
        modifiers: Default::default(),
    };
    assert_eq!(tooltip.on_event(&event), EventResult::Consumed);
    assert_eq!(clicks.load(Ordering::SeqCst), 1);
}

#[test]
fn tooltip_defaults_to_pointer_anchored() {
    let clicks = Arc::new(AtomicUsize::new(0));
    let tooltip = Tooltip::new(Box::new(ClickChild::new(clicks)), "tip", test_font());
    assert!(tooltip.at_pointer);
}

#[test]
fn tooltip_can_opt_into_widget_anchor() {
    let clicks = Arc::new(AtomicUsize::new(0));
    let tooltip = Tooltip::new(Box::new(ClickChild::new(clicks)), "tip", test_font()).at_widget();
    assert!(!tooltip.at_pointer);
}

// --- Interactive-tip behaviour contract ---------------------------------

fn interactive_tooltip(clicks: Arc<AtomicUsize>) -> Tooltip {
    let anchor = Box::new(ClickChild::new(Arc::new(AtomicUsize::new(0))));
    let content = Box::new(ClickChild::new(clicks));
    let mut t = Tooltip::new(anchor, "unused", test_font()).with_interactive_content(content);
    t.layout(Size::new(40.0, 20.0));
    t
}

#[test]
fn interactive_content_sets_flags() {
    let t = interactive_tooltip(Arc::new(AtomicUsize::new(0)));
    assert!(t.interactive);
    assert!(t.content.is_some());
    // Interactive tips anchor to the widget, not the pointer.
    assert!(!t.at_pointer);
}

#[test]
fn interactive_hit_requires_open() {
    let mut t = interactive_tooltip(Arc::new(AtomicUsize::new(0)));
    t.tip_panel_local = Some(Rect::new(0.0, -30.0, 100.0, 20.0));
    // Closed: even a point inside the stored rect is not a hit.
    assert!(!t.interactive_hit(Point::new(10.0, -20.0)));
    t.tip_open = true;
    assert!(t.interactive_hit(Point::new(10.0, -20.0)));
    assert!(!t.interactive_hit(Point::new(200.0, -20.0)));
}

#[test]
fn interactive_tip_opens_after_delay() {
    let mut t = interactive_tooltip(Arc::new(AtomicUsize::new(0)));
    t.hovered = true;
    // Not yet past the delay.
    t.hover_started_at = Some(Instant::now());
    t.update_interactive_state();
    assert!(!t.tip_open);
    // Past the delay.
    t.hover_started_at = Some(Instant::now() - (TOOLTIP_INITIAL_DELAY + Duration::from_millis(20)));
    t.update_interactive_state();
    assert!(t.tip_open);
}

#[test]
fn interactive_tip_stays_open_while_tip_hovered() {
    let mut t = interactive_tooltip(Arc::new(AtomicUsize::new(0)));
    t.tip_open = true;
    t.hovered = false;
    t.tip_hovered = true;
    t.update_interactive_state();
    assert!(t.tip_open);
    assert!(t.close_requested_at.is_none());
}

#[test]
fn interactive_tip_closes_after_grace_when_left() {
    let mut t = interactive_tooltip(Arc::new(AtomicUsize::new(0)));
    t.tip_open = true;
    t.hovered = false;
    t.tip_hovered = false;
    // First tick arms the close timer but keeps it open.
    t.update_interactive_state();
    assert!(t.tip_open);
    assert!(t.close_requested_at.is_some());
    // After the grace period it closes.
    t.close_requested_at =
        Some(Instant::now() - (interactive::TOOLTIP_CLOSE_GRACE + Duration::from_millis(20)));
    t.update_interactive_state();
    assert!(!t.tip_open);
}

#[test]
fn escape_closes_interactive_tip() {
    let mut t = interactive_tooltip(Arc::new(AtomicUsize::new(0)));
    t.tip_open = true;
    assert_eq!(
        t.on_unconsumed_key(&Key::Escape, Modifiers::default()),
        EventResult::Consumed
    );
    assert!(!t.tip_open);
    // With the tip already closed, Escape is ignored (lets other handlers run).
    assert_eq!(
        t.on_unconsumed_key(&Key::Escape, Modifiers::default()),
        EventResult::Ignored
    );
}

#[test]
fn escape_close_sticks_while_still_hovering_anchor() {
    // Regression: Escape closes the tip, but a key press is not followed by a
    // mouse-move, so `hovered` stays true and `hover_started_at` stays in the
    // past. `update_interactive_state` must NOT reopen the tip on the next
    // frame — reopening requires leaving and re-entering the anchor.
    let mut t = interactive_tooltip(Arc::new(AtomicUsize::new(0)));
    t.hovered = true;
    t.hover_started_at = Some(Instant::now() - (TOOLTIP_INITIAL_DELAY + Duration::from_millis(20)));
    t.tip_open = true;

    assert_eq!(
        t.on_unconsumed_key(&Key::Escape, Modifiers::default()),
        EventResult::Consumed
    );
    assert!(!t.tip_open);

    // Next frame's overlay pass: still hovering the anchor, delay already
    // elapsed. Must stay closed.
    t.update_interactive_state();
    assert!(!t.tip_open);
}

#[test]
fn interactive_forwards_click_into_content() {
    let clicks = Arc::new(AtomicUsize::new(0));
    let mut t = interactive_tooltip(clicks.clone());
    // Simulate an open tip whose content occupies the panel origin.
    t.tip_open = true;
    t.layout_content();
    t.content_origin_local = Point::ORIGIN;
    t.tip_panel_local = Some(Rect::new(0.0, 0.0, 100.0, 100.0));

    let click = Event::MouseUp {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Left,
        modifiers: Default::default(),
    };
    assert_eq!(t.on_event(&click), EventResult::Consumed);
    assert_eq!(clicks.load(Ordering::SeqCst), 1);
}
