//! Popup-menu row tooltips (`MenuItem::tooltip`) through the real `App`.
//!
//! A host widget owns a `PopupMenu` the way `MenuBar` and app dropdowns do
//! (events forwarded, popup painted in `paint_global_overlay`, overlay
//! hit-test while open).  The tests drive the real pointer path
//! (`App::on_mouse_move`) and full `App::paint` frames with the tooltip test
//! clock, then assert on the central tooltip controller
//! (`widgets/tooltip/controller.rs`): a row's tip appears after the hover
//! delay, rows without one show nothing, and the tip clears when the pointer
//! leaves the row or the menu closes.
//!
//! A real `MenuBar` (`bar.rs`) is covered too: it runs its popup in the bar's
//! LOCAL space, so its dropdown rows must offer their tips when the bar sits
//! away from the root origin, and while the on-screen keyboard's lift
//! (`widget/keyboard_scroll.rs`) shifts the painted tree.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{App, Widget};
use crate::widgets::tooltip::controller;
use crate::widgets::tooltip::{reset_tooltip_test_state, tooltip_timings};

use super::super::geometry::{hit_test, MenuHit, BAR_H};
use super::super::model::{MenuEntry, MenuItem};
use super::{MenuBar, PopupMenu, TopMenu};

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Desktop input baseline at scale 1, a pinned tooltip clock, a system font
/// for the tip painter, no keyboard lift, and a clean controller; restores
/// the shared state (scales included) on drop.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        reset_tooltip_test_state();
        controller::reset();
        crate::font_settings::set_system_font(None);
        crate::widget::keyboard_scroll::reset_lift_for_test();
        crate::ux_scale::set_ux_scale(1.0);
        crate::device_scale::set_device_scale(1.0);
    }
}
fn pin() -> Guard {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
    crate::widget::keyboard_scroll::reset_lift_for_test();
    reset_tooltip_test_state();
    controller::reset();
    crate::clock::start_virtual();
    crate::font_settings::set_system_font(Some(test_font()));
    Guard
}

/// Fills the viewport and hosts a shared `PopupMenu` like a real dropdown
/// owner.  `close_next_paint` closes the menu at the start of the next paint
/// without any pointer event, so a test can tell "closed" from "pressed".
struct Host {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    menu: Rc<RefCell<PopupMenu>>,
    close_next_paint: Rc<Cell<bool>>,
    font: Arc<Font>,
}

impl Widget for Host {
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
    fn paint(&mut self, _: &mut dyn DrawCtx) {
        if self.close_next_paint.replace(false) {
            self.menu.borrow_mut().close();
        }
    }
    fn hit_test_global_overlay(&self, local_pos: Point) -> bool {
        let menu = self.menu.borrow();
        menu.is_open() && menu.body_contains(local_pos, VIEWPORT)
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        self.menu.borrow_mut().handle_event(event, VIEWPORT).0
    }
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let font = Arc::clone(&self.font);
        self.menu.borrow_mut().paint(ctx, font, 14.0, VIEWPORT);
    }
}

struct Harness {
    app: App,
    menu: Rc<RefCell<PopupMenu>>,
    close_next_paint: Rc<Cell<bool>>,
}

impl Harness {
    /// An open popup at (40, 360): "Recent A" and "Recent B" carry tips,
    /// "Plain" has none, and "More" opens a submenu whose first row has one.
    fn new() -> Self {
        let items: Vec<MenuEntry> = vec![
            MenuItem::action("Recent A", "open.a")
                .tooltip("C:/designs/a.mcx")
                .into(),
            MenuItem::action("Recent B", "open.b")
                .tooltip("C:/designs/b.mcx")
                .into(),
            MenuItem::action("Plain", "plain").into(),
            MenuItem::submenu(
                "More",
                vec![MenuItem::action("Deep", "deep")
                    .tooltip("C:/designs/deep.mcx")
                    .into()],
            )
            .into(),
        ];
        let mut popup = PopupMenu::new(items);
        popup.open_at(Point::new(40.0, 360.0));
        let menu = Rc::new(RefCell::new(popup));
        let close_next_paint = Rc::new(Cell::new(false));
        let host = Host {
            bounds: Rect::default(),
            children: Vec::new(),
            menu: Rc::clone(&menu),
            close_next_paint: Rc::clone(&close_next_paint),
            font: test_font(),
        };
        let mut app = App::new(Box::new(host));
        app.layout(VIEWPORT);
        Self {
            app,
            menu,
            close_next_paint,
        }
    }

    /// Centre (world, Y-up) of the laid-out row at `path`.
    fn row_center(&self, path: &[usize]) -> Point {
        let menu = self.menu.borrow();
        let layouts = menu.state.layouts(&menu.items, VIEWPORT);
        for layout in &layouts {
            for row in &layout.rows {
                let mut row_path = layout.path_prefix.clone();
                row_path.extend(row.item_index);
                let c = Point::new(
                    row.rect.x + row.rect.width * 0.5,
                    row.rect.y + row.rect.height * 0.5,
                );
                if row.item_index.is_some()
                    && row_path == path
                    && matches!(hit_test(&layouts, c), Some(MenuHit::Item(hit)) if hit == path)
                {
                    return c;
                }
            }
        }
        panic!("row {path:?} is not laid out");
    }

    /// Move the pointer to world `p` (the App takes Y-down screen coords).
    fn hover(&mut self, p: Point) {
        self.app.on_mouse_move(p.x, VIEWPORT.height - p.y);
    }

    /// One full App frame (tree, popup overlay, tooltip resolution).
    fn frame(&mut self) {
        paint_frame(&mut self.app);
    }
}

#[test]
fn hovered_row_tooltip_shows_after_hover_delay() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let mut h = Harness::new();
    let row = h.row_center(&[0]);

    h.hover(row);
    h.frame();
    assert!(!controller::is_visible(), "no tip before the hover delay");

    crate::clock::advance(tooltip_timings().initial_delay);
    h.frame();
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/a.mcx"),
        "the hovered row's tooltip shows once the delay elapses"
    );
    let r = controller::visible_rect().expect("visible tip is placed");
    assert!(
        r.x >= 0.0 && r.x + r.width <= VIEWPORT.width,
        "tip placed inside the viewport: {r:?}"
    );
}

#[test]
fn row_without_tooltip_shows_none() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let mut h = Harness::new();
    let row = h.row_center(&[2]);

    h.hover(row);
    h.frame();
    crate::clock::advance(tooltip_timings().initial_delay);
    h.frame();
    assert!(
        !controller::is_visible(),
        "a row without a tooltip shows none"
    );
}

#[test]
fn moving_to_another_row_switches_and_leaving_clears() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let timings = tooltip_timings();
    let mut h = Harness::new();
    let (a, b, plain) = (h.row_center(&[0]), h.row_center(&[1]), h.row_center(&[2]));

    h.hover(a);
    h.frame();
    crate::clock::advance(timings.initial_delay);
    h.frame();
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/a.mcx")
    );

    // Another tipped row: A's tip goes at once, B's follows after the
    // (warm) reshow delay — never A's text on B.
    h.hover(b);
    h.frame();
    assert!(!controller::is_visible(), "A's tip leaves with the pointer");
    crate::clock::advance(timings.reshow_delay);
    h.frame();
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/b.mcx")
    );

    // A row without a tip clears it.
    h.hover(plain);
    h.frame();
    assert!(!controller::is_visible(), "leaving the row clears the tip");

    // Leaving the menu body entirely keeps it cleared.
    h.hover(Point::new(380.0, 20.0));
    crate::clock::advance(timings.initial_delay);
    h.frame();
    assert!(!controller::is_visible(), "no tip outside the menu");
}

#[test]
fn closing_the_menu_clears_the_tip() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let mut h = Harness::new();
    let row = h.row_center(&[0]);

    h.hover(row);
    h.frame();
    crate::clock::advance(tooltip_timings().initial_delay);
    h.frame();
    assert!(controller::is_visible());

    // Close with the pointer still where the row was: no press involved.
    h.close_next_paint.set(true);
    h.frame();
    assert!(!h.menu.borrow().is_open());
    assert!(
        !controller::is_visible(),
        "a closed menu offers no tip, so it clears"
    );
}

#[test]
fn submenu_row_tooltip_shows() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let mut h = Harness::new();

    // Hovering "More" opens its submenu.
    let more = h.row_center(&[3]);
    h.hover(more);
    h.frame();
    assert_eq!(h.menu.borrow().state.open_path, vec![3]);

    let deep = h.row_center(&[3, 0]);
    h.hover(deep);
    h.frame();
    crate::clock::advance(tooltip_timings().initial_delay);
    h.frame();
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/deep.mcx"),
        "a submenu row's tooltip shows like a top-level row's"
    );
}

/// Root for the menu-bar case: children at fixed root-space rects (like
/// `tests_conformance.rs`'s `Window`), so the bar can sit off the origin.
struct Root {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    rects: Vec<Rect>,
}

impl Widget for Root {
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
        for (child, rect) in self.children.iter_mut().zip(&self.rects) {
            child.layout(Size::new(rect.width, rect.height));
            child.set_bounds(*rect);
        }
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// The `MenuBar` the menu-bar tests place as the root's only child.
fn menu_bar(app: &App) -> &MenuBar {
    app.root().children()[0]
        .as_any()
        .and_then(|any| any.downcast_ref::<MenuBar>())
        .expect("the root's only child is the MenuBar")
}

/// An App whose root holds only a `MenuBar` at `bar_rect` with one "File"
/// menu: "Recent A" and "Recent B" carry tips, "Plain" has none.
fn bar_app(bar_rect: Rect) -> App {
    let items: Vec<MenuEntry> = vec![
        MenuItem::action("Recent A", "open.a")
            .tooltip("C:/designs/a.mcx")
            .into(),
        MenuItem::action("Recent B", "open.b")
            .tooltip("C:/designs/b.mcx")
            .into(),
        MenuItem::action("Plain", "plain").into(),
    ];
    let bar = MenuBar::new(test_font(), vec![TopMenu::new("File", items)], |_| {});
    let mut app = App::new(Box::new(Root {
        bounds: Rect::default(),
        children: vec![Box::new(bar)],
        rects: vec![bar_rect],
    }));
    app.layout(physical_viewport());
    assert_eq!(menu_bar(&app).bounds(), bar_rect);
    app
}

/// World (root, Y-up) centre of the bar's open top-level row `row`.  The
/// bar's popup layouts are bar-local, so the bar's origin is added.
fn bar_row_center(app: &App, row: usize) -> Point {
    let bar = menu_bar(app);
    let layouts = bar
        .popup
        .state
        .layouts(&bar.popup.items, crate::widget::current_viewport());
    let rect = layouts
        .first()
        .and_then(|layout| layout.rows.iter().find(|r| r.item_index == Some(row)))
        .map(|r| r.rect)
        .expect("row is laid out in the open bar menu");
    let local = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    assert!(
        matches!(hit_test(&layouts, local), Some(MenuHit::Item(hit)) if hit == [row]),
        "the bar-local centre hits row {row}"
    );
    let origin = bar.bounds();
    Point::new(local.x + origin.x, local.y + origin.y)
}

/// `VIEWPORT` in physical pixels at the current effective (device × UX)
/// scale: what a shell hands `App::layout` and sizes its framebuffer to.
fn physical_viewport() -> Size {
    let s = crate::ux_scale::effective_scale();
    Size::new(VIEWPORT.width * s, VIEWPORT.height * s)
}

/// The App's (Y-down physical) pointer position for logical tree point
/// `world` while the keyboard lifts the painted tree by `lift`.
fn screen_of(world: Point, lift: f64) -> (f64, f64) {
    let s = crate::ux_scale::effective_scale();
    (world.x * s, (VIEWPORT.height - (world.y + lift)) * s)
}

/// One full App frame (tree, popup overlay, tooltip resolution) into a
/// framebuffer of the physical viewport.
fn paint_frame(app: &mut App) {
    let phys = physical_viewport();
    let mut fb = Framebuffer::new(phys.width as u32, phys.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());
    app.paint(&mut ctx);
}

/// Open the bar's "File" menu with a real press + release on its title.
fn open_bar_menu(app: &mut App, lift: f64) {
    let bar_rect = menu_bar(app).bounds();
    let title = Point::new(bar_rect.x + 8.0, bar_rect.y + BAR_H * 0.5);
    let (x, y) = screen_of(title, lift);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    assert!(
        app.root().children()[0].has_active_modal(),
        "clicking File opens its menu"
    );
    paint_frame(app);
}

/// Hover the bar's top-level row `row` on screen, check the popup itself
/// sees it, then wait `delay` and paint so the tooltip frame resolves.
fn hover_bar_row(app: &mut App, row: usize, lift: f64, delay: Duration) {
    let (x, y) = screen_of(bar_row_center(app, row), lift);
    app.on_mouse_move(x, y);
    paint_frame(app);
    assert_eq!(
        menu_bar(app).popup.state.hover_path.as_deref(),
        Some(&[row][..]),
        "the bar's popup sees the pointer over row {row}"
    );
    crate::clock::advance(delay);
    paint_frame(app);
}

#[test]
fn menu_bar_row_tooltip_shows_when_bar_is_away_from_root_origin() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let timings = tooltip_timings();
    // Off the root origin on both axes, so bar-local != root coordinates.
    let mut app = bar_app(Rect::new(
        60.0,
        VIEWPORT.height - BAR_H - 10.0,
        300.0,
        BAR_H,
    ));
    open_bar_menu(&mut app, 0.0);

    hover_bar_row(&mut app, 0, 0.0, timings.initial_delay);
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/a.mcx"),
        "a menu-bar dropdown row's tooltip shows when the bar is not at the root origin"
    );

    // The next row's tip, never a neighbour's picked by an offset.
    hover_bar_row(&mut app, 1, 0.0, timings.reshow_delay);
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/b.mcx"),
        "moving down a row in the menu-bar dropdown shows that row's tooltip"
    );
}

#[test]
fn menu_bar_row_tooltip_shows_while_the_keyboard_lifts_the_tree() {
    use crate::widget::keyboard_scroll::{current_lift, lift_target_for_test, request_lift};
    // More than a row (24 px) and not a multiple of it: a hit test that
    // drops the lift lands on another row or off the menu.
    const LIFT: f64 = 50.0;
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let timings = tooltip_timings();
    // Low enough that the lifted bar and its dropdown stay on screen.
    let mut app = bar_app(Rect::new(60.0, 250.0, 300.0, BAR_H));
    request_lift(LIFT);
    paint_frame(&mut app);
    crate::clock::advance(Duration::from_secs(1));
    paint_frame(&mut app);
    assert!(
        (current_lift() - LIFT).abs() < 1e-9,
        "the lift settles at {LIFT}, got {}",
        current_lift()
    );
    open_bar_menu(&mut app, LIFT);
    assert_eq!(
        lift_target_for_test(),
        LIFT,
        "opening the menu does not retarget the lift"
    );

    hover_bar_row(&mut app, 0, LIFT, timings.initial_delay);
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/a.mcx"),
        "a menu-bar dropdown row's tooltip shows while the keyboard lifts the tree"
    );

    hover_bar_row(&mut app, 1, LIFT, timings.reshow_delay);
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/b.mcx"),
        "the lifted dropdown's next row shows its own tooltip"
    );
}

#[test]
fn menu_bar_row_tooltip_shows_at_device_and_ux_scale() {
    let _lock = crate::input_profile::profile_test_lock();
    // `Guard` puts both scales back to 1 on drop, even if an assert fails.
    let _g = pin();
    let timings = tooltip_timings();
    // Device 2 × UX 1.5: App paints under ×3, so a hit test that maps the
    // pointer through device pixels instead of logical ones misses the rows.
    crate::device_scale::set_device_scale(2.0);
    crate::ux_scale::set_ux_scale(1.5);
    assert!((crate::ux_scale::effective_scale() - 3.0).abs() < 1e-12);
    let mut app = bar_app(Rect::new(
        60.0,
        VIEWPORT.height - BAR_H - 10.0,
        300.0,
        BAR_H,
    ));
    assert_eq!(crate::widget::current_viewport(), VIEWPORT);
    open_bar_menu(&mut app, 0.0);

    hover_bar_row(&mut app, 0, 0.0, timings.initial_delay);
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/a.mcx"),
        "a menu-bar dropdown row's tooltip shows at device × UX scale 3"
    );

    hover_bar_row(&mut app, 1, 0.0, timings.reshow_delay);
    assert_eq!(
        controller::visible_text().as_deref(),
        Some("C:/designs/b.mcx"),
        "the scaled dropdown's next row shows its own tooltip"
    );
}
