//! `PopupMenu` widget rows and widget-local anchoring
//! (`menu/row_widgets.rs`, `widget/popup_local.rs`).
//!
//! A probe widget records every event it receives and paints a solid fill,
//! so the tests can check row layout, event routing (hover, hover-leave,
//! press capture), keyboard navigation skipping widget rows, painting, and
//! that a menu opened at a local anchor follows its host's root origin.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

use super::super::model::{MenuEntry, MenuItem};
use super::super::state::MenuResponse;
use super::PopupMenu;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

fn reset_env() {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
}

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Records the events it receives (in its local coordinates) and paints red.
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    log: Rc<RefCell<Vec<Event>>>,
}

impl Widget for Probe {
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
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        self.log.borrow_mut().push(event.clone());
        EventResult::Consumed
    }
}

fn probe() -> (Box<dyn Widget>, Rc<RefCell<Vec<Event>>>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let p = Probe {
        bounds: Rect::default(),
        children: Vec::new(),
        log: Rc::clone(&log),
    };
    (Box::new(p), log)
}

/// "A" action, a 40 px widget row, "B" action — opened at (20, 300).
fn menu_with_row() -> (PopupMenu, Rc<RefCell<Vec<Event>>>) {
    let (widget, log) = probe();
    let mut menu = PopupMenu::new(vec![MenuItem::action("A", "a").into()]);
    let id = menu.push_widget_row(widget, 40.0);
    menu.items.push(MenuItem::action("B", "b").into());
    assert_eq!(id, 0);
    menu.open_at(Point::new(20.0, 300.0));
    (menu, log)
}

fn row_rect(menu: &PopupMenu, row: usize) -> Rect {
    menu.state.layouts(&menu.items, VIEWPORT)[0].rows[row].rect
}

fn center(r: Rect) -> Point {
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

fn down(pos: Point) -> Event {
    Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

fn up(pos: Point) -> Event {
    Event::MouseUp {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

#[test]
fn widget_row_takes_its_own_height() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (menu, _) = menu_with_row();
    let layout = &menu.state.layouts(&menu.items, VIEWPORT)[0];
    let heights: Vec<f64> = layout.rows.iter().map(|r| r.rect.height).collect();
    assert_eq!(heights, vec![24.0, 40.0, 24.0]);
    assert_eq!(layout.rect.height, 88.0);
    // Rows stack top-down in Y-up space.
    assert_eq!(layout.rows[1].rect.y + 40.0, layout.rows[0].rect.y);
}

#[test]
fn pointer_events_in_a_widget_row_go_to_the_widget() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, log) = menu_with_row();
    let row = row_rect(&menu, 1);
    let p = center(row);

    let (_, response) = menu.handle_event(&Event::MouseMove { pos: p }, VIEWPORT);
    assert_eq!(response, MenuResponse::None);
    assert_eq!(menu.state.hover_path, None, "widget rows are never hovered");

    let (result, response) = menu.handle_event(&down(p), VIEWPORT);
    assert!(result.is_consumed());
    assert_eq!(response, MenuResponse::None);
    assert!(
        menu.is_open(),
        "a click in a widget row keeps the menu open"
    );
    menu.handle_event(&up(p), VIEWPORT);

    let log = log.borrow();
    assert_eq!(log.len(), 3, "move, down, up: {log:?}");
    // Events arrive in the widget's local space (row origin = (0, 0)).
    let local = Point::new(row.width * 0.5, row.height * 0.5);
    assert!(matches!(log[0], Event::MouseMove { pos } if pos == local));
    assert!(matches!(log[1], Event::MouseDown { pos, .. } if pos == local));
    assert!(matches!(log[2], Event::MouseUp { pos, .. } if pos == local));
}

#[test]
fn leaving_a_widget_row_sends_an_outside_move() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, log) = menu_with_row();
    let row = row_rect(&menu, 1);
    menu.handle_event(&Event::MouseMove { pos: center(row) }, VIEWPORT);
    let a = center(row_rect(&menu, 0));
    menu.handle_event(&Event::MouseMove { pos: a }, VIEWPORT);

    assert_eq!(
        menu.state.hover_path,
        Some(vec![0]),
        "text rows still hover"
    );
    let log = log.borrow();
    assert_eq!(log.len(), 2);
    let Event::MouseMove { pos } = log[1] else {
        panic!("expected a leave move, got {:?}", log[1]);
    };
    assert!(
        pos.y > row.height,
        "leave move lies outside the row: {pos:?}"
    );
}

#[test]
fn a_press_in_a_widget_row_captures_until_release() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, log) = menu_with_row();
    let p = center(row_rect(&menu, 1));
    menu.handle_event(&down(p), VIEWPORT);
    // Dragging over row "A" and releasing there must not activate "A".
    let a = center(row_rect(&menu, 0));
    let (_, r1) = menu.handle_event(&Event::MouseMove { pos: a }, VIEWPORT);
    let (_, r2) = menu.handle_event(&up(a), VIEWPORT);
    assert_eq!((r1, r2), (MenuResponse::None, MenuResponse::None));
    assert!(menu.is_open());
    assert_eq!(menu.state.hover_path, None);
    let kinds: Vec<&str> = log
        .borrow()
        .iter()
        .map(|e| match e {
            Event::MouseDown { .. } => "down",
            Event::MouseMove { .. } => "move",
            Event::MouseUp { .. } => "up",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["down", "move", "up"]);
}

#[test]
fn text_rows_still_activate_and_keyboard_skips_widget_rows() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, log) = menu_with_row();
    let key = |k| Event::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
    };
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    assert_eq!(menu.state.hover_path, Some(vec![0]));
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    assert_eq!(menu.state.hover_path, Some(vec![2]), "row 1 is skipped");

    let b = center(row_rect(&menu, 2));
    let (_, response) = menu.handle_event(&down(b), VIEWPORT);
    assert_eq!(response, MenuResponse::Action("b".to_string()));
    assert!(log.borrow().is_empty(), "the widget saw nothing");
}

#[test]
fn widget_rows_paint_in_their_row_and_nest_in_submenus() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, _) = menu_with_row();
    let row = row_rect(&menu, 1);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        menu.paint(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    let px = |x: f64, y: f64| {
        let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
        [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
    };
    let c = center(row);
    assert_eq!(px(c.x, c.y), [255, 0, 0], "row widget painted in its row");
    let a = center(row_rect(&menu, 0));
    assert_ne!(px(a.x, a.y), [255, 0, 0], "text row not covered");

    // A widget row inside a submenu is laid out and routed the same way.
    let (widget, log) = probe();
    let mut nested = PopupMenu::new(vec![MenuItem::submenu(
        "More",
        vec![MenuItem::widget_row(7, 30.0).into()],
    )
    .into()]);
    nested.set_row_widget(7, widget);
    nested.open_at(Point::new(20.0, 300.0));
    nested.state.open_path = vec![0];
    let layouts = nested.state.layouts(&nested.items, VIEWPORT);
    let sub_row = layouts[1].rows[0].rect;
    assert_eq!(sub_row.height, 30.0);
    nested.handle_event(&down(center(sub_row)), VIEWPORT);
    assert_eq!(log.borrow().len(), 1);
    assert!(nested.with_row_widget(7, |w| w.bounds().height).is_some());
    assert!(nested.remove_row_widget(7).is_some());
    assert!(matches!(nested.items[0], MenuEntry::Item(_)));
}

#[test]
fn open_at_local_follows_the_host_root_origin() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut menu = PopupMenu::new(vec![
        MenuItem::action("A", "a").into(),
        MenuItem::action("B", "b").into(),
    ]);
    menu.set_root_origin(Point::new(100.0, 250.0));
    menu.open_at_local(Point::new(0.0, 0.0));
    assert_eq!(menu.state.anchor, Point::new(100.0, 250.0));
    assert_eq!(menu.local_anchor(), Some(Point::ORIGIN));

    // The host moved: the open menu moves with it.
    menu.set_root_origin(Point::new(110.0, 260.0));
    assert_eq!(menu.state.anchor, Point::new(110.0, 260.0));

    // Local events are shifted into root space before hit-testing.
    let b_root = center(row_rect(&menu, 1));
    let b_local = Point::new(b_root.x - 110.0, b_root.y - 260.0);
    assert!(menu.body_contains_local(b_local, VIEWPORT));
    let (_, response) = menu.handle_local_event(&down(b_local), VIEWPORT);
    assert_eq!(response, MenuResponse::Action("b".to_string()));

    // `open_at` (root coordinates) drops the local anchor.
    menu.open_at(Point::new(5.0, 300.0));
    assert_eq!(menu.local_anchor(), None);
    menu.set_root_origin(Point::new(0.0, 0.0));
    assert_eq!(menu.state.anchor, Point::new(5.0, 300.0));
}

#[test]
fn sync_root_origin_reads_the_paint_transform_and_paint_local_matches() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (widget, _) = probe();
    let mut menu = PopupMenu::new(Vec::new()).with_widget_row(widget, 40.0);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        ctx.translate(30.0, 200.0);
        menu.sync_root_origin(&ctx);
        menu.open_at_local(Point::new(10.0, 0.0));
        menu.paint_local(&mut ctx, test_font(), 14.0, VIEWPORT);
    }
    assert_eq!(menu.root_origin(), Point::new(30.0, 200.0));
    let row = row_rect(&menu, 0);
    assert_eq!((row.x, row.y + row.height), (40.0, 200.0));
    let c = center(row);
    let i = ((c.y as u32 * fb.width() + c.x as u32) * 4) as usize;
    assert_eq!(
        &fb.pixels()[i..i + 3],
        &[255, 0, 0],
        "painted at the root anchor"
    );
}
