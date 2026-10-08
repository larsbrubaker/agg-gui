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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{App, Widget};
use crate::widgets::tooltip::controller;
use crate::widgets::tooltip::{reset_tooltip_test_state, tooltip_timings};

use super::super::geometry::{hit_test, MenuHit};
use super::super::model::{MenuEntry, MenuItem};
use super::PopupMenu;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Desktop input baseline, a pinned tooltip clock, a system font for the tip
/// painter, and a clean controller; restores the shared state on drop.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        reset_tooltip_test_state();
        controller::reset();
        crate::font_settings::set_system_font(None);
    }
}
fn pin() -> Guard {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
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
        let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        self.app.paint(&mut ctx);
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
