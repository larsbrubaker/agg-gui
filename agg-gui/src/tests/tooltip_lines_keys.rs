//! Central tooltip controller: multi-line tips and per-item tooltip keys.
//!
//! Two gaps reported by a treemap-style widget (HDTreeMap) that shows one tip
//! per rectangle through [`Widget::tooltip_text`]:
//!
//! * **Multi-line** — the controller (fed by `WidgetBase::tooltip` /
//!   `tooltip_text`) must split its text on `\n` exactly like the `Tooltip`
//!   wrapper does, instead of handing one string with embedded newlines to
//!   `fill_text` (which paints missing-glyph boxes).
//! * **Per-item keys** — a single widget showing many items reports which one
//!   is hovered through [`Widget::tooltip_key`]. A key change while hovering
//!   behaves as if the pointer entered a new widget: the visible tip hides and
//!   the hover delay re-arms, with the quick reshow delay when a tip was
//!   recently visible.
//!
//! Every test drives the production path: `App` layout, real pointer moves,
//! the virtual tooltip clock, and `App::paint` into a `PaintRecorder`.

use super::keyboard_lift_harness::{ScaleGuard, SystemFontGuard, TooltipGuard};
use super::paint_recorder::PaintRecorder;
use super::*;

use crate::geometry::Rect;
use crate::layout_props::WidgetBase;
use crate::text::Font;
use crate::widgets::tooltip::{controller, tooltip_timings};
use crate::{DrawCtx, Event, EventResult};
use std::sync::Arc;
use std::time::Duration;

const VP: Size = Size {
    width: 400.0,
    height: 300.0,
};

/// A viewport-filling leaf that shows two "items": the left half and the
/// right half, each with its own tip and key — a two-cell treemap. The hovered
/// item follows the last `MouseMove`. `keyed: false` reports no key (the
/// pre-existing behaviour: the text changes, the identity does not).
struct TwoItems {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    tips: [String; 2],
    hovered: usize,
    keyed: bool,
}

impl TwoItems {
    fn new(left: &str, right: &str, keyed: bool) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            tips: [left.to_string(), right.to_string()],
            hovered: 0,
            keyed,
        }
    }
}

impl Widget for TwoItems {
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
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseMove { pos } = event {
            self.hovered = usize::from(pos.x >= self.bounds.width * 0.5);
        }
        EventResult::Ignored
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn tooltip_text(&self) -> Option<&str> {
        Some(&self.tips[self.hovered])
    }
    fn tooltip_key(&self) -> Option<u64> {
        self.keyed.then_some(self.hovered as u64 + 1000)
    }
}

/// Move the pointer to logical `(x, y)` (Y-up) at scale 1.
fn hover(app: &mut App, x: f64, y: f64) {
    app.on_mouse_move(x, VP.height - y);
}

fn paint(app: &mut App) -> PaintRecorder {
    let mut ctx = PaintRecorder::new();
    app.paint(&mut ctx);
    ctx
}

fn guards() -> (ScaleGuard, TooltipGuard, SystemFontGuard) {
    (
        ScaleGuard::set(1.0, 1.0),
        TooltipGuard::new(),
        SystemFontGuard::set(Arc::new(Font::from_slice(TEST_FONT).expect("test font"))),
    )
}

/// Hover `(x, y)`, arm on one frame, let the full initial delay elapse, and
/// return the frame that shows the tip.
fn show_at(app: &mut App, x: f64, y: f64) -> PaintRecorder {
    hover(app, x, y);
    paint(app);
    crate::clock::advance(tooltip_timings().initial_delay + Duration::from_millis(10));
    paint(app)
}

/// A `\n`-separated `WidgetBase` tip paints one text run per line — never a
/// run containing a newline — and its panel is three lines tall.
#[test]
fn central_tip_splits_text_on_newlines() {
    let _g = guards();
    let tip = "Folder: src\nSize: 12 MB\nFiles: 340";
    let mut app = App::new(Box::new(TwoItems::new(tip, tip, false)));
    app.layout(VP);

    let frame = show_at(&mut app, 100.0, 150.0);
    assert_eq!(controller::visible_text().as_deref(), Some(tip));

    let painted: Vec<&str> = frame.texts.iter().map(|(t, _, _)| t.as_str()).collect();
    assert!(
        painted.iter().all(|t| !t.contains('\n')),
        "no text run may carry a newline (it paints as a missing glyph): {painted:?}"
    );
    for line in tip.lines() {
        assert!(
            painted.contains(&line),
            "line {line:?} painted: {painted:?}"
        );
    }
    // Lines stack top to bottom (Y-up: each baseline below the previous).
    let ys: Vec<f64> = tip
        .lines()
        .map(|l| frame.texts.iter().find(|(t, _, _)| t == l).unwrap().2)
        .collect();
    assert!(ys[0] > ys[1] && ys[1] > ys[2], "baselines descend: {ys:?}");

    // Panel geometry agrees with the painter: three 12 × 1.45 lines + 6 px
    // padding top and bottom.
    let r = controller::visible_rect().expect("visible tip has a rect");
    let want_h = 3.0 * 12.0 * 1.45 + 12.0;
    assert!(
        (r.height - want_h).abs() < 1e-9,
        "3-line panel height: {r:?}"
    );
}

/// Without a key, moving between items inside one widget only swaps the text
/// (the identity is the widget's path) — the pre-existing contract stays.
#[test]
fn unkeyed_item_change_keeps_tip_up_with_new_text() {
    let _g = guards();
    let mut app = App::new(Box::new(TwoItems::new("Left", "Right", false)));
    app.layout(VP);

    show_at(&mut app, 100.0, 150.0);
    assert_eq!(controller::visible_text().as_deref(), Some("Left"));
    hover(&mut app, 300.0, 150.0);
    paint(&mut app);
    assert_eq!(controller::visible_text().as_deref(), Some("Right"));
}

/// With a key, moving to another item while its tip is visible hides the tip
/// and re-arms: the new tip waits the quick reshow delay (a tip was just
/// visible), not the full initial delay, and then shows the new item's text.
#[test]
fn keyed_item_change_rearms_with_reshow_delay() {
    let _g = guards();
    let t = tooltip_timings();
    let mut app = App::new(Box::new(TwoItems::new("Left", "Right", true)));
    app.layout(VP);

    show_at(&mut app, 100.0, 150.0);
    assert_eq!(controller::visible_text().as_deref(), Some("Left"));

    hover(&mut app, 300.0, 150.0);
    paint(&mut app);
    assert!(
        !controller::is_visible(),
        "a new item key hides the tip like entering a new widget"
    );

    crate::clock::advance(t.reshow_delay / 2);
    paint(&mut app);
    assert!(!controller::is_visible(), "still inside the reshow delay");

    crate::clock::advance(t.reshow_delay / 2 + Duration::from_millis(1));
    paint(&mut app);
    assert!(
        t.reshow_delay < t.initial_delay,
        "precondition: reshow is quicker"
    );
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("Right"),
        "the new item's tip shows after only the reshow delay"
    );
}

/// With a key, hovering the same item across frames does not re-arm: the tip
/// shows after the initial delay even while the pointer keeps moving within it.
#[test]
fn keyed_same_item_does_not_rearm() {
    let _g = guards();
    let t = tooltip_timings();
    let mut app = App::new(Box::new(TwoItems::new("Left", "Right", true)));
    app.layout(VP);

    hover(&mut app, 100.0, 150.0);
    paint(&mut app);
    crate::clock::advance(t.initial_delay / 2);
    hover(&mut app, 120.0, 140.0);
    paint(&mut app);
    crate::clock::advance(t.initial_delay / 2 + Duration::from_millis(1));
    hover(&mut app, 110.0, 160.0);
    paint(&mut app);
    assert_eq!(controller::visible_text().as_deref(), Some("Left"));
}

/// With a key and no tip recently visible, an item change re-arms the full
/// initial delay (cold), exactly as entering a new widget would.
#[test]
fn keyed_item_change_when_cold_waits_full_initial_delay() {
    let _g = guards();
    let t = tooltip_timings();
    let mut app = App::new(Box::new(TwoItems::new("Left", "Right", true)));
    app.layout(VP);

    hover(&mut app, 100.0, 150.0);
    paint(&mut app);
    crate::clock::advance(t.initial_delay / 2);
    hover(&mut app, 300.0, 150.0);
    paint(&mut app);
    // The left item's timer would have fired here; the right item re-armed.
    crate::clock::advance(t.initial_delay / 2 + Duration::from_millis(1));
    paint(&mut app);
    assert!(
        !controller::is_visible(),
        "the right item re-armed the delay"
    );
    crate::clock::advance(t.initial_delay / 2);
    paint(&mut app);
    assert_eq!(controller::visible_text().as_deref(), Some("Right"));
}
