//! agg-sharp `ScrollableTouchPanTests` (Tests/Agg.Tests/Agg.UI/
//! ScrollableTouchPanTests.cs): a finger has no hover, so on touch the
//! content itself is the scroll handle. A tap (released within the tap
//! tolerance) is a click where the finger lifts; a drag past it pans the
//! nearest `ScrollView` that can scroll that way, with no press reaching
//! anything; a drag nothing can scroll is the widget's own drag, pressed
//! where the finger went down and never clicking. A mouse drag over the
//! content does not pan.
//!
//! C# feeds touch-typed mouse events to a `SystemWindow`; agg-gui's touch
//! shells call `App::on_touch_*` and `crate::touch_emulation` turns those
//! into mouse events, so these tests drive the same entry points. C#'s
//! window is Y-up; `Finger` flips its positions to the screen's Y-down.
//! `Probe` stands in for C#'s button `GuiWidget` (its `MouseDown` and
//! `Click` counts); `ScrollFromTop` is the offset cell.
//!
//! Converted to agg-gui's multi-touch design: a second finger is not a
//! multi-position mouse move here but a touch of its own, handed to the
//! multi-touch aggregate (`Event::MultiTouch`, `App::active_touch_count`).
//! So `a_second_finger_before_the_gesture_is_decided_presses_at_the_start_
//! and_never_clicks` checks that the aggregate has both fingers and that
//! nothing is pressed, clicked or captured (C# pressed the button so it could
//! read the fingers from mouse moves), `a_lost_release_is_let_go_when_the_
//! next_finger_lands` lands the next finger as a second touch (a dropped
//! release leaves the first one down), and `a_pinch_reaches_a_widget_that_
//! reads_fingers` reads the pinch from `Event::MultiTouch`. The floating
//! bar's `FadeTarget` and `Opacity` are `ScrollView::scroll_bar_properties`'
//! `fade_target` and `opacity`, the latter after a paint on the virtual clock.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use super::touch_pan_fixtures::{
    reset, screen, scroll_at, Board, Counts, Finger, Probe, DEV, FINGER, SECOND_FINGER,
};
use crate::widgets::scroll_view::ScrollBarStyle;
use crate::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Modifiers, MouseButton, Rect,
    ScrollView, Size, Widget,
};

const W: f64 = 200.0;
const H: f64 = 100.0;
const FINGERS: Finger = Finger { height: H };

struct View {
    app: App,
    counts: Rc<Counts>,
    offset: Rc<Cell<f64>>,
}

/// A 200 x 100 window over 500 of content, with a 100 x 40 button showing at
/// (20..120, 20..60); `extra` adds more to the content board.
fn make_with(style: ScrollBarStyle, extra: impl FnOnce(Board) -> Board) -> View {
    reset(1.0);
    let counts = Rc::new(Counts::default());
    let offset = Rc::new(Cell::new(0.0));
    let content = Board::new(W, 500.0).place(
        Rect::new(20.0, 420.0, 100.0, 40.0),
        Box::new(Probe::new(Rc::clone(&counts))),
    );
    let scroll = ScrollView::new(Box::new(extra(content)))
        .with_style(style)
        .with_offset_cell(Rc::clone(&offset));
    let root = Board::new(W, H).place(Rect::new(0.0, 0.0, W, H), Box::new(scroll));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(W, H));
    View {
        app,
        counts,
        offset,
    }
}

fn make() -> View {
    make_with(ScrollBarStyle::solid(), |b| b)
}

fn presses(v: &View) -> u32 {
    v.counts.presses.get()
}

fn clicks(v: &View) -> u32 {
    v.counts.clicks.get()
}

fn mouse_down(app: &mut App, x: f64, y: f64) {
    let (sx, sy) = screen(H, x, y);
    app.on_mouse_down(sx, sy, MouseButton::Left, Modifiers::default());
}

fn mouse_move(app: &mut App, x: f64, y: f64) {
    let (sx, sy) = screen(H, x, y);
    app.on_mouse_move(sx, sy);
}

fn mouse_up(app: &mut App, x: f64, y: f64) {
    let (sx, sy) = screen(H, x, y);
    app.on_mouse_up(sx, sy, MouseButton::Left, Modifiers::default());
}

/// The vertical bar's opacity (agg-sharp `ScrollBar.Opacity`).
fn opacity(app: &mut App) -> f64 {
    scroll_at(app, &[0])
        .scroll_bar_properties()
        .vertical
        .opacity
}

fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(W as u32, H as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

#[test]
fn a_touch_drag_pans_by_the_fingers_travel_and_never_presses() {
    let mut v = make();
    assert!((v.offset.get() - 0.0).abs() <= 1e-9);

    FINGERS.down(&mut v.app, 50.0, 40.0);

    // under the threshold it may still be a tap: nothing moves, and nothing is pressed yet
    FINGERS.move_to(&mut v.app, 50.0, 45.0);
    assert!((v.offset.get() - 0.0).abs() <= 1e-9);
    assert_eq!(presses(&v), 0);

    // past it, the content follows the finger from where it went down - up 30 shows 30 more below
    FINGERS.move_to(&mut v.app, 50.0, 70.0);
    assert!((v.offset.get() - 30.0).abs() <= 1e-9);

    // released over the button, which has moved under the finger: a pan never presses or clicks
    FINGERS.up(&mut v.app);
    assert_eq!(presses(&v), 0);
    assert_eq!(clicks(&v), 0);
    assert!((v.offset.get() - 30.0).abs() <= 1e-9);
}

#[test]
fn a_touch_tap_clicks_where_the_finger_lifts() {
    // down just below the button, lifted on it, 7 px away: the click is at the lift, as in agg-gui
    let mut v = make();
    FINGERS.down(&mut v.app, 50.0, 14.0);
    FINGERS.move_to(&mut v.app, 50.0, 21.0);
    assert_eq!(presses(&v), 0);
    FINGERS.up(&mut v.app);
    assert_eq!(presses(&v), 1);
    assert_eq!(clicks(&v), 1);
    assert!((v.offset.get() - 0.0).abs() <= 1e-9);
}

#[test]
fn a_cancelled_touch_never_clicks() {
    let mut v = make();
    FINGERS.down(&mut v.app, 50.0, 40.0);
    v.app.on_touch_cancel(DEV, FINGER);
    assert_eq!(presses(&v), 0);
    assert_eq!(clicks(&v), 0);
}

#[test]
fn a_mouse_drag_over_the_content_does_not_pan() {
    let mut v = make();
    mouse_down(&mut v.app, 50.0, 40.0);
    assert_eq!(presses(&v), 1);
    mouse_move(&mut v.app, 50.0, 70.0);
    mouse_up(&mut v.app, 50.0, 70.0);
    assert!((v.offset.get() - 0.0).abs() <= 1e-9);

    // and a mouse click still clicks
    mouse_down(&mut v.app, 50.0, 40.0);
    mouse_up(&mut v.app, 50.0, 40.0);
    assert_eq!(clicks(&v), 1);
}

#[test]
fn a_floating_bar_shows_while_touch_panning_and_fades_after() {
    let mut v = make_with(ScrollBarStyle::floating(), |b| b);
    crate::clock::start_virtual();
    let fade_target = |app: &mut App| {
        scroll_at(app, &[0])
            .scroll_bar_properties()
            .vertical
            .fade_target
    };
    assert_eq!(fade_target(&mut v.app), 0.0);

    FINGERS.down(&mut v.app, 50.0, 40.0);
    assert_eq!(fade_target(&mut v.app), 0.0);

    FINGERS.move_to(&mut v.app, 50.0, 70.0);
    assert_eq!(fade_target(&mut v.app), 1.0);
    paint(&mut v.app);
    // agg-sharp `ScrollBar.FadeMs`
    crate::clock::advance(Duration::from_secs_f64(
        crate::widgets::scrollbar::SCROLL_BAR_FADE_SECS,
    ));
    paint(&mut v.app);
    assert_eq!(opacity(&mut v.app), 1.0);

    FINGERS.up(&mut v.app);
    assert_eq!(fade_target(&mut v.app), 0.0);
}

#[test]
fn a_second_finger_stops_the_pan_and_the_gesture_does_not_click() {
    // agg-gui hands a gesture to the multi-touch aggregate the moment a second finger lands
    let mut v = make();
    FINGERS.down(&mut v.app, 50.0, 40.0);
    FINGERS.move_to(&mut v.app, 50.0, 60.0);
    assert!((v.offset.get() - 20.0).abs() <= 1e-9);

    FINGERS.down_id(&mut v.app, SECOND_FINGER, 150.0, 80.0);
    FINGERS.move_to(&mut v.app, 50.0, 80.0);
    FINGERS.move_to(&mut v.app, 50.0, 90.0);
    assert!((v.offset.get() - 20.0).abs() <= 1e-9);

    FINGERS.up(&mut v.app);
    v.app.on_touch_end(DEV, SECOND_FINGER);
    assert_eq!(clicks(&v), 0);
}

#[test]
fn an_inner_view_that_cannot_scroll_passes_the_pan_to_its_parent() {
    // the inner view's content fits, so a vertical drag in it pans the outer view, as agg-gui's ScrollView
    // leaves a drag it cannot scroll to its ancestors
    let mut v = make_with(ScrollBarStyle::solid(), |b| {
        let inner =
            ScrollView::new(Box::new(Board::new(100.0, 40.0))).with_style(ScrollBarStyle::solid());
        b.place(Rect::new(20.0, 300.0, 100.0, 60.0), Box::new(inner))
    });
    scroll_at(&mut v.app, &[0]).set_scroll_offset(150.0);
    v.app.layout(Size::new(W, H));
    // the inner view spans content 300..360; at 150 from the top the window shows content 250..350, so the
    // inner view's centre (content 330) is at window y 80
    let inner_y = 330.0 - (500.0 - 150.0 - H);

    FINGERS.down(&mut v.app, 50.0, inner_y);
    FINGERS.move_to(&mut v.app, 50.0, inner_y + 20.0);
    FINGERS.up(&mut v.app);
    assert!((v.offset.get() - 170.0).abs() <= 1e-9);
}

#[test]
fn a_drag_the_view_cannot_scroll_is_the_widgets_own_drag() {
    // nothing scrolls sideways here, so a sideways finger drag is the widget's drag (a slider, a 3D view):
    // pressed where the finger went down, released without a click
    let mut v = make();
    FINGERS.down(&mut v.app, 50.0, 40.0);
    FINGERS.move_to(&mut v.app, 70.0, 40.0);
    assert_eq!(presses(&v), 1);
    FINGERS.up(&mut v.app);
    assert_eq!(clicks(&v), 0);
    assert!((v.offset.get() - 0.0).abs() <= 1e-9);
}

#[test]
fn a_second_finger_before_the_gesture_is_decided_presses_at_the_start_and_never_clicks() {
    // agg-gui: the undecided gesture goes to the multi-touch aggregate, which reads both fingers; nothing is
    // pressed, so nothing can click or keep the capture
    let mut v = make();
    FINGERS.down(&mut v.app, 50.0, 40.0);
    FINGERS.down_id(&mut v.app, SECOND_FINGER, 90.0, 40.0);
    assert_eq!(v.app.active_touch_count(), 2);
    assert_eq!(presses(&v), 0);

    FINGERS.up(&mut v.app);
    v.app.on_touch_end(DEV, SECOND_FINGER);
    assert_eq!(clicks(&v), 0);
    assert!(!v.app.has_captured_pointer());
}

#[test]
fn the_widgets_own_drag_releases_its_capture() {
    let mut v = make();
    FINGERS.down(&mut v.app, 50.0, 40.0);
    FINGERS.move_to(&mut v.app, 70.0, 40.0);
    // root board > scroll view > content board > button
    assert_eq!(v.app.captured_path(), Some(&[0usize, 0, 0][..]));
    FINGERS.up(&mut v.app);
    assert!(!v.app.has_captured_pointer());
}

#[test]
fn a_lost_release_is_let_go_when_the_next_finger_lands() {
    // a host that dropped the pointerup must not leave the old press holding the capture
    let mut v = make();
    FINGERS.down(&mut v.app, 50.0, 40.0);
    FINGERS.move_to(&mut v.app, 70.0, 40.0);
    assert_eq!(v.app.captured_path(), Some(&[0usize, 0, 0][..]));

    FINGERS.down_id(&mut v.app, SECOND_FINGER, 150.0, 80.0);
    assert!(!v.app.has_captured_pointer());
    assert_eq!(clicks(&v), 0);
}

/// Multiplies its zoom by every pinch delivered to it.
struct PinchWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    zoom: Rc<Cell<f64>>,
}

impl Widget for PinchWidget {
    fn type_name(&self) -> &'static str {
        "PinchWidget"
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
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MultiTouch { info } => {
                self.zoom.set(self.zoom.get() * info.zoom_delta);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

#[test]
fn a_pinch_reaches_a_widget_that_reads_fingers() {
    reset(1.0);
    let zoom = Rc::new(Cell::new(1.0));
    let pinch = PinchWidget {
        bounds: Rect::default(),
        children: Vec::new(),
        zoom: Rc::clone(&zoom),
    };
    let root = Board::new(W, H).place(Rect::new(0.0, 0.0, W, H), Box::new(pinch));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(W, H));

    FINGERS.down(&mut app, 80.0, 50.0);
    FINGERS.down_id(&mut app, SECOND_FINGER, 120.0, 50.0);
    paint(&mut app);
    FINGERS.move_to(&mut app, 60.0, 50.0);
    FINGERS.move_id(&mut app, SECOND_FINGER, 140.0, 50.0);
    paint(&mut app);
    FINGERS.up(&mut app);
    app.on_touch_end(DEV, SECOND_FINGER);
    paint(&mut app);

    assert!((zoom.get() - 2.0).abs() <= 1e-9);
    assert!(!app.has_captured_pointer());
}

#[test]
fn a_faded_out_floating_bar_is_panned_over_but_a_visible_bar_still_drags() {
    // a finger cannot see a faded-out bar, so a drag along the edge pans the content
    let mut v = make_with(ScrollBarStyle::floating(), |b| b);
    scroll_at(&mut v.app, &[0]).set_scroll_offset(200.0);
    v.app.layout(Size::new(W, H));
    assert_eq!(opacity(&mut v.app), 0.0);
    let bar_x = |app: &mut App| {
        let t = scroll_at(app, &[0]).scroll_bar_properties().vertical.track;
        t.x + t.width * 0.5
    };
    let x = bar_x(&mut v.app);

    FINGERS.drag(&mut v.app, (x, 50.0), (x, 80.0));
    assert!((v.offset.get() - 230.0).abs() <= 1e-9);

    // a bar that is showing is the bar's drag, not a pan
    let mut v = make();
    let x = bar_x(&mut v.app);
    scroll_at(&mut v.app, &[0]).set_scroll_offset(200.0);
    v.app.layout(Size::new(W, H));
    FINGERS.drag(&mut v.app, (x, 50.0), (x, 80.0));
    assert_ne!(v.offset.get(), 230.0);
}
