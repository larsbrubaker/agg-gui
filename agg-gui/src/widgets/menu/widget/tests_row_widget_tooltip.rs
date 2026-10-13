//! The tooltip of a widget hosted in a popup-menu row (`PopupMenu::push_widget_row`): row widgets
//! live outside the root widget tree, so the menu offers their `tooltip_text` to the central
//! tooltip controller itself (`RowWidgets::tip_at`). The deepest tipped widget under the pointer
//! wins, and a part of the row with no tip shows none.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{App, Widget};
use crate::widgets::tooltip::controller;
use crate::widgets::tooltip::tooltip_timings;
use crate::WidgetBase;

use super::tests_tooltip::{paint_frame, pin, test_font, VIEWPORT};
use super::PopupMenu;

/// A tipped box: a left half and a right half, only the right one carrying a tip.
struct Halves {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

struct Half {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

macro_rules! plain_widget {
    () => {
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
        fn widget_base(&self) -> Option<&WidgetBase> {
            Some(&self.base)
        }
        fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
            Some(&mut self.base)
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }
    };
}

impl Widget for Half {
    plain_widget!();
    fn layout(&mut self, available: Size) -> Size {
        available
    }
}

impl Widget for Halves {
    plain_widget!();
    fn layout(&mut self, available: Size) -> Size {
        let half = available.width / 2.0;
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(half, available.height));
            child.set_bounds(Rect::new(half * i as f64, 0.0, half, available.height));
        }
        available
    }
}

fn half(tip: Option<&str>) -> Box<dyn Widget> {
    let mut base = WidgetBase::new();
    base.tooltip = tip.map(str::to_string);
    Box::new(Half {
        bounds: Rect::default(),
        children: Vec::new(),
        base,
    })
}

struct Host {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    menu: Rc<RefCell<PopupMenu>>,
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
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
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

/// A 40 px row of two halves (the left tipless, the right tipped "Right half") in an open menu.
fn open_menu() -> (App, Rc<RefCell<PopupMenu>>) {
    let halves = Halves {
        bounds: Rect::default(),
        children: vec![half(None), half(Some("Right half"))],
        base: WidgetBase::new(),
    };
    let mut popup = PopupMenu::new(Vec::new()).with_widget_row(Box::new(halves), 40.0);
    popup.open_at(Point::new(40.0, 360.0));
    let menu = Rc::new(RefCell::new(popup));
    let host = Host {
        bounds: Rect::default(),
        children: Vec::new(),
        menu: Rc::clone(&menu),
        font: test_font(),
    };
    let mut app = App::new(Box::new(host));
    app.layout(VIEWPORT);
    (app, menu)
}

fn hover(app: &mut App, p: Point) {
    app.on_mouse_move(p.x, VIEWPORT.height - p.y);
}

#[test]
fn a_tipped_widget_in_a_row_shows_its_tip_after_the_hover_delay() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let (mut app, menu) = open_menu();
    paint_frame(&mut app);
    let row = menu.borrow().state.layouts(&menu.borrow().items, VIEWPORT)[0].rows[0].rect;
    let right = Point::new(row.x + row.width * 0.75, row.y + row.height * 0.5);

    hover(&mut app, right);
    paint_frame(&mut app);
    assert!(!controller::is_visible(), "no tip before the hover delay");
    crate::clock::advance(tooltip_timings().initial_delay);
    paint_frame(&mut app);
    assert_eq!(controller::visible_text().as_deref(), Some("Right half"));
}

#[test]
fn the_part_of_a_row_without_a_tip_shows_none() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let (mut app, menu) = open_menu();
    paint_frame(&mut app);
    let row = menu.borrow().state.layouts(&menu.borrow().items, VIEWPORT)[0].rows[0].rect;
    let left = Point::new(row.x + row.width * 0.25, row.y + row.height * 0.5);

    hover(&mut app, left);
    paint_frame(&mut app);
    crate::clock::advance(tooltip_timings().initial_delay);
    paint_frame(&mut app);
    assert!(!controller::is_visible());
}
