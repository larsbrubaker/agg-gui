//! A `Splitter` with a pull tab (`Splitter::with_collapse_tab`): the tab and
//! the bar's double click collapse and reopen a side, a plain drag on the
//! bar only resizes, the bar stays at the edge while collapsed, hover
//! darkens a translucent bar, and the shared `SplitterCollapsed` cell
//! carries the state both ways.
//!
//! The splitters are 400 x 200 with the default 6-wide bar: open at a 0.5
//! ratio the bar spans x 197..203 and the tab (8 deep, 48 long, centred)
//! x 189..197, y 76..124; collapsed (the right side) the bar spans
//! x 394..400 and the tab x 386..394.

use crate::widgets::{BarTab, CollapseSide, SplitterCollapsed};
use crate::{
    App, Color, Event, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Rect, Size, SizedBox,
    Splitter, Widget,
};

const W: f64 = 400.0;
const H: f64 = 200.0;

/// A left|right splitter whose right side collapses, laid out at `W` x `H`.
fn splitter(state: &SplitterCollapsed) -> Splitter {
    let mut s = Splitter::new(Box::new(SizedBox::new()), Box::new(SizedBox::new()))
        .with_collapse_tab(CollapseSide::Second, state.clone())
        .with_collapse_tooltips("Hide the side panel", "Show the side panel");
    relayout(&mut s);
    s
}

fn relayout(s: &mut Splitter) {
    s.layout(Size::new(W, H));
    s.set_bounds(Rect::new(0.0, 0.0, W, H));
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

/// A click on the tab collapses the side; a click on the tab where it then
/// sits (at the edge) brings it back at the same width.
#[test]
fn a_click_on_the_tab_toggles_the_side() {
    let state = SplitterCollapsed::new(false);
    let mut s = splitter(&state);
    click(&mut s, 193.0, 100.0);
    assert!(state.get(), "the tab click collapses the side");
    relayout(&mut s);
    assert_eq!(s.children()[1].bounds().width, 0.0);
    click(&mut s, 390.0, 100.0);
    assert!(!state.get(), "the tab at the edge reopens it");
    relayout(&mut s);
    assert_eq!(s.ratio, 0.5, "the side comes back at the width it had");
    assert_eq!(s.bar_rect().x, 197.0);
}

/// A double click on the bar toggles the side (through the App, which
/// carries the click count); a single click there does not.
#[test]
fn a_double_click_on_the_bar_toggles_the_side() {
    let state = SplitterCollapsed::new(false);
    let mut app = App::new(Box::new(splitter(&state)));
    app.layout(Size::new(W, H));
    let m = Modifiers::default();
    // Y-down screen coordinates: y 170 is local y 30, away from the tab.
    app.on_mouse_down_clicks(200.0, 170.0, MouseButton::Left, m, 1);
    app.on_mouse_up(200.0, 170.0, MouseButton::Left, m);
    assert!(!state.get(), "a single click on the bar does not toggle");
    app.on_mouse_down_clicks(200.0, 170.0, MouseButton::Left, m, 2);
    app.on_mouse_up(200.0, 170.0, MouseButton::Left, m);
    assert!(state.get(), "the double click collapses the side");
    app.layout(Size::new(W, H));
    app.on_mouse_down_clicks(397.0, 170.0, MouseButton::Left, m, 1);
    app.on_mouse_up(397.0, 170.0, MouseButton::Left, m);
    app.on_mouse_down_clicks(397.0, 170.0, MouseButton::Left, m, 2);
    app.on_mouse_up(397.0, 170.0, MouseButton::Left, m);
    assert!(
        !state.get(),
        "a double click on the bar at the edge reopens it"
    );
}

/// A press and drag on the bar resizes and never collapses.
#[test]
fn a_drag_on_the_bar_only_resizes() {
    let state = SplitterCollapsed::new(false);
    let mut s = splitter(&state);
    down(&mut s, 200.0, 30.0);
    move_to(&mut s, 100.0, 30.0);
    up(&mut s, 100.0, 30.0);
    assert!(!state.get(), "a drag does not collapse");
    assert!((s.ratio - 0.25).abs() < 1e-9, "got {}", s.ratio);
    // Even a drag that starts beside the tab, along it.
    down(&mut s, 101.0, 100.0);
    move_to(&mut s, 300.0, 100.0);
    up(&mut s, 300.0, 100.0);
    assert!(!state.get());
    assert!((s.ratio - 0.75).abs() < 1e-9, "got {}", s.ratio);
}

/// Collapsed, the side takes no room and the bar stays at the edge with
/// its tab, which still takes the pointer; a drag on that bar pulls the
/// side out: it stays at the edge until the pointer reaches where the open
/// bar can sit, then follows the pointer to where it is let go.
#[test]
fn collapsed_keeps_the_bar_at_the_edge() {
    let state = SplitterCollapsed::new(true);
    let mut s = splitter(&state).with_ratio(0.3);
    relayout(&mut s);
    assert!(s.is_collapsed());
    assert_eq!(s.children()[0].bounds().width, W - 6.0);
    assert_eq!(s.children()[1].bounds().width, 0.0);
    assert_eq!(s.bar_rect(), Rect::new(394.0, 0.0, 6.0, H));
    assert!(
        s.claims_pointer_exclusively(Point::new(390.0, 100.0)),
        "the tab"
    );
    assert!(
        s.claims_pointer_exclusively(Point::new(397.0, 30.0)),
        "the bar"
    );
    assert!(!s.claims_pointer_exclusively(Point::new(300.0, 100.0)));
    down(&mut s, 397.0, 30.0);
    // 390 / 400 is past the drag range's 0.95: the bar cannot sit there
    // open, so it stays at the edge rather than jumping to the old width.
    move_to(&mut s, 390.0, 30.0);
    assert!(state.get(), "not yet where the open bar can sit");
    assert_eq!(s.bar_rect().x, 394.0);
    move_to(&mut s, 300.0, 30.0);
    assert!(!state.get(), "the drag out reopens the side");
    assert!((s.ratio - 0.75).abs() < 1e-9, "the bar is at the pointer");
    // The same press keeps following the pointer.
    move_to(&mut s, 200.0, 30.0);
    up(&mut s, 200.0, 30.0);
    relayout(&mut s);
    assert!(!state.get());
    assert!((s.ratio - 0.5).abs() < 1e-9, "got {}", s.ratio);
    assert_eq!(s.bar_rect().x, 197.0);
    // A drag never collapses: dragged to the edge, the bar stops at the
    // end of the drag range and the side stays open.
    down(&mut s, 200.0, 30.0);
    move_to(&mut s, W, 30.0);
    up(&mut s, W, 30.0);
    assert!(!state.get(), "a drag to the edge does not collapse");
    assert!((s.ratio - 0.95).abs() < 1e-9, "got {}", s.ratio);
}

/// A finger dragged across the tab toward the middle opens the side, toward
/// the edge closes it.
#[test]
fn a_finger_drag_across_the_tab_opens_toward_the_middle_and_closes_toward_the_edge() {
    let state = SplitterCollapsed::new(false);
    let mut s = splitter(&state);
    // The presses below are a finger's.
    crate::touch_emulation::long_press::set_last_press_was_touch_for_testing(true);
    down(&mut s, 193.0, 100.0);
    move_to(&mut s, 230.0, 100.0);
    up(&mut s, 230.0, 100.0);
    assert!(state.get(), "toward the edge closes");
    relayout(&mut s);
    down(&mut s, 390.0, 100.0);
    move_to(&mut s, 350.0, 100.0);
    up(&mut s, 350.0, 100.0);
    assert!(!state.get(), "toward the middle opens");
}

/// The tab's tooltip says what a click does, in the host's words, and only
/// over the tab.
#[test]
fn the_tooltip_is_the_hosts_words_over_the_tab() {
    let state = SplitterCollapsed::new(false);
    let mut s = splitter(&state);
    move_to(&mut s, 200.0, 30.0);
    assert_eq!(s.tooltip_text(), None, "not over the bar");
    move_to(&mut s, 193.0, 100.0);
    assert_eq!(s.tooltip_text(), Some("Hide the side panel"));
    state.set(true);
    assert_eq!(s.tooltip_text(), Some("Show the side panel"));
}

/// The bar's pixel at (`x`, `y`) (local, Y up) as painted by an App.
fn pixel(app: &mut App, x: u32, y: u32) -> [u8; 4] {
    let mut fb = Framebuffer::new(W as u32, H as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        // A white page under the translucent bar.
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        app.paint(&mut ctx);
    }
    let idx = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[idx], p[idx + 1], p[idx + 2], p[idx + 3]]
}

fn luminance(p: [u8; 4]) -> u32 {
    u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])
}

/// Hovering the bar darkens bar and tab together, with a translucent black
/// bar colour (where scaling RGB would change nothing); hovering the tab
/// does too.
#[test]
fn hover_darkens_a_translucent_bar_and_its_tab() {
    let translucent = Color::rgba(0.0, 0.0, 0.0, 60.0 / 255.0);
    let darker = BarTab::darken(translucent);
    assert!(darker.a > translucent.a, "a translucent black gains alpha");
    let opaque = Color::rgba(0.8, 0.8, 0.8, 1.0);
    assert!(BarTab::darken(opaque).r < 0.8, "an opaque colour darkens");
    assert_eq!(BarTab::darken(opaque).a, 1.0);

    let state = SplitterCollapsed::new(false);
    let s = splitter(&state).with_bar_color(translucent);
    let mut app = App::new(Box::new(s));
    app.layout(Size::new(W, H));
    // Bar at local (200, 30); tab at local (192, 100).
    let bar_idle = pixel(&mut app, 200, 30);
    let tab_idle = pixel(&mut app, 192, 100);
    assert_eq!(luminance(bar_idle), luminance(tab_idle), "one colour");
    app.on_mouse_move(200.0, H - 30.0);
    let bar_hover = pixel(&mut app, 200, 30);
    let tab_hover = pixel(&mut app, 192, 100);
    assert!(
        luminance(bar_hover) < luminance(bar_idle),
        "{bar_hover:?} vs {bar_idle:?}"
    );
    assert!(
        luminance(tab_hover) < luminance(tab_idle),
        "the tab darkens with the bar"
    );
    assert_eq!(luminance(bar_hover), luminance(tab_hover));
    app.on_mouse_move(192.0, H - 100.0);
    assert!(luminance(pixel(&mut app, 200, 30)) < luminance(bar_idle));
}

/// The cell is two-way: a host write is taken in at the next layout, and a
/// toggle on the tab is written to it, for every clone.
#[test]
fn the_state_cell_round_trips() {
    let state = SplitterCollapsed::new(false);
    let saved = state.clone();
    let mut s = splitter(&state);
    saved.set(true);
    relayout(&mut s);
    assert!(s.is_collapsed(), "the host's write lands");
    assert_eq!(s.bar_rect().x, 394.0);
    click(&mut s, 390.0, 100.0);
    assert!(!saved.get(), "the tab's toggle reaches the host's clone");
    // Without a tab the splitter has no collapsed side.
    let plain = Splitter::new(Box::new(SizedBox::new()), Box::new(SizedBox::new()));
    assert!(!plain.is_collapsed());
    assert!(plain.collapse_tab().is_none());
}

/// With a mouse, a drag on the tab past the click allowance resizes like a
/// drag on the bar, and from collapsed pulls the side out after the pointer.
#[test]
fn a_mouse_drag_on_the_tab_resizes() {
    let state = SplitterCollapsed::new(false);
    let mut s = splitter(&state);
    down(&mut s, 193.0, 100.0);
    move_to(&mut s, 100.0, 100.0);
    up(&mut s, 100.0, 100.0);
    assert!(!state.get(), "a mouse drag does not toggle");
    assert!((s.ratio - 0.25).abs() < 1e-9, "got {}", s.ratio);

    state.set(true);
    relayout(&mut s);
    down(&mut s, 390.0, 100.0);
    move_to(&mut s, 385.0, 100.0);
    assert!(state.get(), "within the click allowance nothing moves");
    move_to(&mut s, 300.0, 100.0);
    assert!(!state.get(), "the drag pulls the side out");
    assert!((s.ratio - 0.75).abs() < 1e-9, "got {}", s.ratio);
    move_to(&mut s, 200.0, 100.0);
    up(&mut s, 200.0, 100.0);
    assert!((s.ratio - 0.5).abs() < 1e-9, "got {}", s.ratio);
}

/// After a click on the tab moves the bar, hover and tooltip describe the
/// pointer's place at once: it is over the pane now, not the tab.
#[test]
fn hover_and_tooltip_follow_the_bar_after_a_tab_click() {
    let state = SplitterCollapsed::new(false);
    let mut s = splitter(&state);
    move_to(&mut s, 193.0, 100.0);
    assert_eq!(s.tooltip_text(), Some("Hide the side panel"));
    click(&mut s, 193.0, 100.0);
    assert!(state.get());
    let tab = s.collapse_tab().expect("a tab");
    assert!(!tab.hovered(), "the bar left the pointer behind");
    assert_eq!(s.tooltip_text(), None);
}

/// Counts the presses that reach it (the pane under the tab).
struct PressCounter {
    bounds: Rect,
    presses: std::rc::Rc<std::cell::Cell<u32>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for PressCounter {
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
        available
    }
    fn paint(&mut self, _ctx: &mut dyn crate::DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> crate::EventResult {
        if let Event::MouseDown { .. } = event {
            self.presses.set(self.presses.get() + 1);
            return crate::EventResult::Consumed;
        }
        crate::EventResult::Ignored
    }
}

/// A double click on the tab: the first click collapses the side and moves
/// the tab to the edge; the second press, where the tab was, is swallowed,
/// so the pane now there never sees a double click. A later press there is
/// the pane's.
#[test]
fn a_double_click_on_the_tab_never_reaches_the_pane_under_it() {
    let presses = std::rc::Rc::new(std::cell::Cell::new(0));
    let pane = PressCounter {
        bounds: Rect::default(),
        presses: std::rc::Rc::clone(&presses),
        children: Vec::new(),
    };
    let state = SplitterCollapsed::new(false);
    let s = Splitter::new(Box::new(pane), Box::new(SizedBox::new()))
        .with_collapse_tab(CollapseSide::Second, state.clone());
    let mut app = App::new(Box::new(s));
    app.layout(Size::new(W, H));
    let m = Modifiers::default();
    // Local (193, 100) is on the tab; screen Y is down.
    app.on_mouse_down_clicks(193.0, 100.0, MouseButton::Left, m, 1);
    app.on_mouse_up(193.0, 100.0, MouseButton::Left, m);
    assert!(state.get(), "the first click collapses");
    app.layout(Size::new(W, H));
    app.on_mouse_down_clicks(193.0, 100.0, MouseButton::Left, m, 2);
    app.on_mouse_up(193.0, 100.0, MouseButton::Left, m);
    assert_eq!(presses.get(), 0, "the second press is swallowed");
    assert!(state.get(), "and toggles nothing");
    app.on_mouse_down_clicks(193.0, 100.0, MouseButton::Left, m, 1);
    app.on_mouse_up(193.0, 100.0, MouseButton::Left, m);
    assert_eq!(presses.get(), 1, "a later press is the pane's");
}
