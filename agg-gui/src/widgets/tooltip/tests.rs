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

// --- Lightweight tooltip timing state machine ----------------------------
//
// These drive the delay/reshow/autopop/press-hide behaviour against an
// injected clock so no real time passes. `ClockGuard` pins the clock at a
// base instant and resets all tooltip timing globals on drop (including on a
// test panic via unwind) so sibling tests are unaffected.

struct ClockGuard;

impl Drop for ClockGuard {
    fn drop(&mut self) {
        reset_tooltip_test_state();
    }
}

/// Pin the tooltip clock at `base` and clear the shared reshow window / timings.
fn pin_clock(base: Instant) -> ClockGuard {
    reset_tooltip_test_state();
    set_tooltip_test_clock(Some(base));
    ClockGuard
}

fn text_tooltip() -> Tooltip {
    let mut t = Tooltip::new(
        Box::new(ClickChild::new(Arc::new(AtomicUsize::new(0)))),
        "tip",
        test_font(),
    );
    t.layout(Size::new(20.0, 20.0));
    t
}

fn move_to(t: &mut Tooltip, x: f64, y: f64) {
    t.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    });
}

#[test]
fn no_tip_before_initial_delay() {
    let _g = pin_clock(Instant::now());
    let timings = tooltip_timings();
    let mut t = text_tooltip();

    move_to(&mut t, 10.0, 10.0); // enter
    assert!(!t.update_visibility());

    // Just short of the initial delay: still hidden.
    advance_tooltip_test_clock(timings.initial_delay - Duration::from_millis(1));
    assert!(!t.update_visibility());
}

#[test]
fn tip_appears_after_initial_delay() {
    let _g = pin_clock(Instant::now());
    let timings = tooltip_timings();
    let mut t = text_tooltip();

    move_to(&mut t, 10.0, 10.0); // enter
    advance_tooltip_test_clock(timings.initial_delay);
    assert!(t.update_visibility());
    assert!(t.tooltip_visible);
}

#[test]
fn reshow_delay_applies_between_controls_within_window() {
    let base = Instant::now();
    let _g = pin_clock(base);
    let timings = tooltip_timings();

    // Control A: hover long enough to show its tip, warming the subsystem.
    let mut a = text_tooltip();
    move_to(&mut a, 10.0, 10.0);
    advance_tooltip_test_clock(timings.initial_delay);
    assert!(a.update_visibility());
    move_to(&mut a, 100.0, 100.0); // leave A (tip hides, window stays warm)

    // A short move to control B, still inside the reshow window.
    advance_tooltip_test_clock(timings.reshow_delay);
    let mut b = text_tooltip();
    move_to(&mut b, 10.0, 10.0); // enter B
    assert_eq!(
        b.pending_delay, timings.reshow_delay,
        "moving between tipped controls should use the reshow delay"
    );

    // B is not shown before the (short) reshow delay elapses...
    assert!(!b.update_visibility());
    // ...but appears once it does — well before a full initial delay.
    advance_tooltip_test_clock(timings.reshow_delay);
    assert!(b.update_visibility());
}

#[test]
fn full_initial_delay_after_reshow_window_expires() {
    let base = Instant::now();
    let _g = pin_clock(base);
    let timings = tooltip_timings();

    // Control A shows and warms the subsystem, then the pointer leaves.
    let mut a = text_tooltip();
    move_to(&mut a, 10.0, 10.0);
    advance_tooltip_test_clock(timings.initial_delay);
    assert!(a.update_visibility());
    move_to(&mut a, 100.0, 100.0); // leave A

    // Idle past the reshow window so the subsystem goes cold again.
    advance_tooltip_test_clock(timings.reshow_window() + Duration::from_millis(1));
    let mut b = text_tooltip();
    move_to(&mut b, 10.0, 10.0);
    assert_eq!(
        b.pending_delay, timings.initial_delay,
        "after the reshow window expires the full initial delay applies again"
    );
}

#[test]
fn press_hides_tip_and_suppresses_reshow() {
    let _g = pin_clock(Instant::now());
    let timings = tooltip_timings();
    let mut t = text_tooltip();

    move_to(&mut t, 10.0, 10.0);
    advance_tooltip_test_clock(timings.initial_delay);
    assert!(t.update_visibility());

    // A press over the control hides the tip immediately...
    t.on_event(&Event::MouseDown {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Left,
        modifiers: Default::default(),
    });
    assert!(!t.tooltip_visible);

    // ...and it stays hidden while still hovering, even after more time.
    t.on_event(&Event::MouseUp {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Left,
        modifiers: Default::default(),
    });
    advance_tooltip_test_clock(timings.initial_delay);
    assert!(!t.update_visibility());
}

#[test]
fn autopop_dismisses_tip_after_timeout() {
    let _g = pin_clock(Instant::now());
    let timings = tooltip_timings();
    let mut t = text_tooltip();

    move_to(&mut t, 10.0, 10.0);
    advance_tooltip_test_clock(timings.initial_delay);
    assert!(t.update_visibility());

    // Pointer just sits there: after the autopop timeout the tip dismisses.
    advance_tooltip_test_clock(timings.autopop);
    assert!(!t.update_visibility());
    assert!(!t.tooltip_visible);

    // It does not re-show while the pointer stays put (suppressed until re-entry).
    advance_tooltip_test_clock(timings.initial_delay);
    assert!(!t.update_visibility());
}

#[test]
fn custom_timings_from_initial_delay_derive_reshow_and_autopop() {
    let initial = Duration::from_millis(300);
    let derived = TooltipTimings::from_initial_delay(initial);
    assert_eq!(derived.reshow_delay, initial / 5);
    assert_eq!(derived.autopop, initial * 10);
}
