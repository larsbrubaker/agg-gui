//! agg-sharp `PopupMenuConformanceTests.ShowMenuClearsArmedTooltips` and
//! `ShowMenuTakesDownAToolTipThatIsAlreadyShowing` (Tests/Agg.Tests/Agg.UI):
//! opening a popup menu takes down the window's tooltip, whether it is only
//! armed by the hover delay or already on screen, through the real `App`.
//!
//! C# hovers a `GuiWidget` with `ToolTipText` in a `SystemWindow` and calls
//! `PopupMenu.ShowMenu`, which calls `ToolTipManager.Clear`. Here the tipped
//! widget is a [`SizedBox`] with [`Widget::with_tooltip`] beside a hosted
//! [`PopupMenu`], the tooltip is the central controller
//! (`widgets/tooltip/controller.rs`), and showing the menu is
//! `PopupMenu::open_at` (every menu opens through `PopupMenuState::open_at`).
//! C#'s "nothing armed" is "the delay passes and still no tip shows".

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{App, Widget};
use crate::widgets::tooltip::{controller, tooltip_timings};
use crate::widgets::SizedBox;

use super::super::model::{MenuEntry, MenuItem};
use super::tests_tooltip::{paint_frame, pin, test_font, VIEWPORT};
use super::PopupMenu;

const TIP: &str = "What this button does";

/// C#'s `GuiWidget(120, 30)` named "Hovered", at (200, 50).
const HOVERED: Rect = Rect {
    x: 200.0,
    y: 50.0,
    width: 120.0,
    height: 30.0,
};

/// A window holding the tipped widget and a (closed) popup menu.
struct Window {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    menu: Rc<RefCell<PopupMenu>>,
    font: Arc<Font>,
}

impl Widget for Window {
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
        for child in &mut self.children {
            child.layout(Size::new(HOVERED.width, HOVERED.height));
            child.set_bounds(HOVERED);
        }
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

/// C#'s `HoverAWidgetWithAToolTip`: the window, with the pointer moved over
/// the tipped widget through the App's real pointer path.
fn hover_a_widget_with_a_tool_tip() -> (App, Rc<RefCell<PopupMenu>>) {
    let items: Vec<MenuEntry> = vec![MenuItem::action("Open", "open").into()];
    let menu = Rc::new(RefCell::new(PopupMenu::new(items)));
    let window = Window {
        bounds: Rect::default(),
        children: vec![Box::new(SizedBox::new().with_tooltip(TIP))],
        menu: Rc::clone(&menu),
        font: test_font(),
    };
    let mut app = App::new(Box::new(window));
    app.layout(VIEWPORT);
    let centre = Point::new(
        HOVERED.x + HOVERED.width * 0.5,
        HOVERED.y + HOVERED.height * 0.5,
    );
    app.on_mouse_move(centre.x, VIEWPORT.height - centre.y);
    paint_frame(&mut app);
    (app, menu)
}

#[test]
fn show_menu_clears_armed_tooltips() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let (mut app, menu) = hover_a_widget_with_a_tool_tip();
    assert!(
        !controller::is_visible(),
        "the hover only arms the tip; the delay has not passed"
    );

    menu.borrow_mut().open_at(Point::new(20.0, 380.0));
    crate::clock::advance(tooltip_timings().initial_delay);
    paint_frame(&mut app);

    assert!(menu.borrow().is_open());
    assert!(
        !controller::is_visible(),
        "the armed tip must not pop up over the menu"
    );
}

#[test]
fn show_menu_takes_down_a_tool_tip_that_is_already_showing() {
    let _lock = crate::input_profile::profile_test_lock();
    let _g = pin();
    let (mut app, menu) = hover_a_widget_with_a_tool_tip();
    crate::clock::advance(tooltip_timings().initial_delay);
    paint_frame(&mut app);
    assert!(controller::is_visible());
    assert_eq!(controller::current_text(), TIP);

    menu.borrow_mut().open_at(Point::new(20.0, 380.0));

    assert!(!controller::is_visible());
    assert_eq!(controller::current_text(), "");
    // and it stays down while the pointer rests on the widget
    paint_frame(&mut app);
    assert_eq!(controller::current_text(), "");
}
