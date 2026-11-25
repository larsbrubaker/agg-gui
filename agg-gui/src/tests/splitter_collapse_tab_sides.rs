//! A `Splitter` pull tab on the other sides: a left/right split collapsing
//! its first (left) pane, and a top/bottom split collapsing its second
//! (bottom) pane. Each checks the side the tab bulges toward, the collapsed
//! bar at the edge, the tab click, and the drag out that reopens the side
//! from where the open bar can first sit, so a flipped direction in the
//! tab's geometry or the drag's reopening reach fails here.
//!
//! The left/right splitter is 400 x 200, the top/bottom one 200 x 400; both
//! have the default 6-wide bar at a 0.5 ratio (bar at 197..203 along the
//! split axis).

use crate::widgets::{CollapseSide, SplitterCollapsed};
use crate::{Event, Modifiers, MouseButton, Point, Rect, Size, SizedBox, Splitter, Widget};

fn relayout(s: &mut Splitter, w: f64, h: f64) {
    s.layout(Size::new(w, h));
    s.set_bounds(Rect::new(0.0, 0.0, w, h));
}

fn down(s: &mut Splitter, x: f64, y: f64) {
    s.on_event(&Event::MouseDown {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn up(s: &mut Splitter, x: f64, y: f64) {
    s.on_event(&Event::MouseUp {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn move_to(s: &mut Splitter, x: f64, y: f64) {
    s.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    });
}

fn click(s: &mut Splitter, x: f64, y: f64) {
    down(s, x, y);
    up(s, x, y);
}

fn tab_rect(s: &Splitter) -> Rect {
    s.collapse_tab().expect("a tab").tab_rect()
}

/// Left pane collapsible: the tab bulges right, into the pane that stays;
/// collapsed, the bar sits at the left edge; a drag out reopens the side
/// once the pointer reaches the drag range's 0.05.
#[test]
fn a_left_right_split_collapses_its_first_side() {
    let state = SplitterCollapsed::new(false);
    let mut s = Splitter::new(Box::new(SizedBox::new()), Box::new(SizedBox::new()))
        .with_collapse_tab(CollapseSide::First, state.clone());
    relayout(&mut s, 400.0, 200.0);
    assert_eq!(
        tab_rect(&s),
        Rect::new(203.0, 76.0, 8.0, 48.0),
        "bulges right"
    );

    click(&mut s, 207.0, 100.0);
    assert!(state.get(), "the tab click collapses the left side");
    relayout(&mut s, 400.0, 200.0);
    assert_eq!(s.bar_rect(), Rect::new(0.0, 0.0, 6.0, 200.0));
    assert_eq!(s.children()[0].bounds().width, 0.0);
    assert_eq!(s.children()[1].bounds(), Rect::new(6.0, 0.0, 394.0, 200.0));
    assert_eq!(tab_rect(&s), Rect::new(6.0, 76.0, 8.0, 48.0));

    click(&mut s, 10.0, 100.0);
    assert!(!state.get(), "the tab at the edge reopens it");
    relayout(&mut s, 400.0, 200.0);
    assert_eq!(s.bar_rect().x, 197.0, "at the width it had");

    state.set(true);
    relayout(&mut s, 400.0, 200.0);
    down(&mut s, 3.0, 30.0);
    move_to(&mut s, 10.0, 30.0);
    assert!(state.get(), "10 / 400 is short of the drag range");
    assert_eq!(s.bar_rect().x, 0.0);
    move_to(&mut s, 100.0, 30.0);
    assert!(!state.get(), "the drag pulls the side out");
    assert!((s.ratio - 0.25).abs() < 1e-9, "got {}", s.ratio);
    up(&mut s, 100.0, 30.0);
}

/// Bottom pane collapsible: the tab bulges up, into the top pane;
/// collapsed, the bar sits at the bottom edge; a drag up reopens the side
/// once the top pane's share is inside the drag range.
#[test]
fn a_top_bottom_split_collapses_its_second_side() {
    let state = SplitterCollapsed::new(false);
    let mut s = Splitter::vertical(Box::new(SizedBox::new()), Box::new(SizedBox::new()))
        .with_collapse_tab(CollapseSide::Second, state.clone());
    relayout(&mut s, 200.0, 400.0);
    assert_eq!(s.bar_rect(), Rect::new(0.0, 197.0, 200.0, 6.0));
    assert_eq!(tab_rect(&s), Rect::new(76.0, 203.0, 48.0, 8.0), "bulges up");

    click(&mut s, 100.0, 207.0);
    assert!(state.get(), "the tab click collapses the bottom side");
    relayout(&mut s, 200.0, 400.0);
    assert_eq!(s.bar_rect(), Rect::new(0.0, 0.0, 200.0, 6.0));
    assert_eq!(s.children()[1].bounds().height, 0.0);
    assert_eq!(s.children()[0].bounds(), Rect::new(0.0, 6.0, 200.0, 394.0));
    assert_eq!(tab_rect(&s), Rect::new(76.0, 6.0, 48.0, 8.0));

    click(&mut s, 100.0, 10.0);
    assert!(!state.get(), "the tab at the edge reopens it");
    relayout(&mut s, 200.0, 400.0);
    assert_eq!(s.bar_rect().y, 197.0, "at the height it had");

    state.set(true);
    relayout(&mut s, 200.0, 400.0);
    down(&mut s, 100.0, 3.0);
    // The top pane's share at y = 10 is 390 / 400, past the range's 0.95.
    move_to(&mut s, 100.0, 10.0);
    assert!(state.get(), "short of where the open bar can sit");
    assert_eq!(s.bar_rect().y, 0.0);
    move_to(&mut s, 100.0, 300.0);
    assert!(!state.get(), "the drag pulls the side out");
    assert!((s.ratio - 0.25).abs() < 1e-9, "got {}", s.ratio);
    up(&mut s, 100.0, 300.0);
}
