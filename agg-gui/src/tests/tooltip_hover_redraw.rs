//! A reactive shell paints only when the app asks (`App::wants_draw`), and
//! the central tooltip controller runs only in a paint. So a pointer move
//! that changes which widget supplies the tip must itself ask for a frame:
//! otherwise moving between widgets that repaint nothing on hover (disabled
//! toolbar buttons) leaves the last tip on screen over the wrong widget, and
//! a tip for the widget now under the pointer is never armed. A move that
//! keeps the same tipped widget asks for nothing, so a still window stays
//! idle while the pointer wanders over one control.

use crate::geometry::{Point, Rect};
use crate::layout_props::WidgetBase;
use crate::widgets::tooltip::{
    controller, reset_tooltip_test_state, set_content_factory, tooltip_timings,
};
use crate::{App, Size, Widget};

use super::tool_tip_placement::{Fixed, Restore};

/// A 200×200 window: "A" tipped over x 0..80, "B" tipped over x 80..160,
/// and untipped window space from x 160 on.
fn two_tipped_app() -> App {
    let tipped = |x: f64, tip: &str| -> Box<dyn Widget> {
        let mut w: Box<dyn Widget> = Box::new(Fixed {
            bounds: Rect::new(x, 0.0, 80.0, 200.0),
            children: Vec::new(),
            size: None,
            base: WidgetBase::new(),
        });
        w.set_tooltip_text(Some(tip.to_string()));
        w
    };
    let mut app = App::new(Box::new(Fixed {
        bounds: Rect::default(),
        children: vec![tipped(0.0, "A"), tipped(80.0, "B")],
        size: None,
        base: WidgetBase::new(),
    }));
    app.layout(Size::new(200.0, 200.0));
    app
}

fn pin() -> Restore {
    reset_tooltip_test_state();
    controller::reset();
    crate::clock::start_virtual();
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
    // A fixed body, so a tip shows without a system font installed.
    set_content_factory(Some(Box::new(|_text: &str| -> Box<dyn Widget> {
        Box::new(Fixed {
            bounds: Rect::default(),
            children: Vec::new(),
            size: Some(Size::new(40.0, 20.0)),
            base: WidgetBase::new(),
        })
    })));
    Restore
}

/// Move the pointer to `p` (Y-up) without painting, as a shell delivers a
/// move, and report whether the app now asks a reactive shell for a frame.
fn move_asks_for_frame(app: &mut App, p: Point) -> bool {
    crate::animation::clear_draw_request();
    app.on_mouse_move(p.x, 200.0 - p.y);
    app.wants_draw()
}

/// The frame a reactive shell paints when asked: the tooltip pass.
fn frame(app: &App) {
    app.update_tooltips_for_test();
}

#[test]
fn moving_to_another_tipped_widget_asks_for_the_frame_that_changes_the_tip() {
    let _restore = pin();
    let mut app = two_tipped_app();

    assert!(move_asks_for_frame(&mut app, Point::new(20.0, 100.0)));
    frame(&app);
    crate::clock::advance(tooltip_timings().initial_delay);
    frame(&app);
    assert_eq!(controller::current_text(), "A");

    assert!(
        !move_asks_for_frame(&mut app, Point::new(40.0, 100.0)),
        "a move within the tipped widget asks for nothing"
    );

    assert!(
        move_asks_for_frame(&mut app, Point::new(120.0, 100.0)),
        "moving onto B asks for the frame that takes A's tip down and arms B's"
    );
    frame(&app);
    assert_eq!(controller::current_text(), "", "A's tip is down");
    crate::clock::advance(tooltip_timings().initial_delay);
    frame(&app);
    assert_eq!(controller::current_text(), "B");
}

#[test]
fn moving_onto_or_off_a_tipped_widget_asks_for_a_frame() {
    let _restore = pin();
    let mut app = two_tipped_app();

    move_asks_for_frame(&mut app, Point::new(180.0, 100.0));
    frame(&app);
    assert!(
        !move_asks_for_frame(&mut app, Point::new(190.0, 100.0)),
        "untipped space to untipped space asks for nothing"
    );
    assert!(
        move_asks_for_frame(&mut app, Point::new(120.0, 100.0)),
        "moving onto B asks for the frame that arms its tip"
    );
    frame(&app);
    crate::clock::advance(tooltip_timings().initial_delay);
    frame(&app);
    assert_eq!(controller::current_text(), "B");

    assert!(
        move_asks_for_frame(&mut app, Point::new(180.0, 100.0)),
        "leaving B asks for the frame that takes its tip down"
    );
    frame(&app);
    assert_eq!(controller::current_text(), "");
}

/// A widget that tips the item under the pointer learns which item that is
/// from the move itself: a tree view that repaints nothing on hover, with a
/// tip on each row, still asks for a frame when the pointer moves to another
/// row, and the tip becomes that row's.
#[test]
fn moving_to_another_row_of_a_tree_that_tips_its_rows_asks_for_a_frame() {
    use crate::widgets::tree_view::{NodeIcon, TreeView};
    const ROW: f64 = 20.0;
    let _restore = pin();
    let font = std::sync::Arc::new(crate::text::Font::from_slice(super::TEST_FONT).expect("font"));
    let mut tree = TreeView::new(font)
        .with_row_height(ROW)
        .with_hover_repaint(false);
    let first = tree.add_root("First", NodeIcon::File);
    let second = tree.add_root("Second", NodeIcon::File);
    tree.set_node_tooltip(first, Some("first row".into()));
    tree.set_node_tooltip(second, Some("second row".into()));
    let mut app = App::new(Box::new(tree));
    app.layout(Size::new(200.0, 200.0));
    // Row `i`'s middle, Y-up.
    let row = |i: f64| Point::new(30.0, 200.0 - (i + 0.5) * ROW);

    move_asks_for_frame(&mut app, row(0.0));
    frame(&app);
    crate::clock::advance(tooltip_timings().initial_delay);
    frame(&app);
    assert_eq!(controller::current_text(), "first row");

    assert!(
        move_asks_for_frame(&mut app, row(1.0)),
        "moving to the second row asks for the frame that changes the tip"
    );
    frame(&app);
    crate::clock::advance(tooltip_timings().initial_delay);
    frame(&app);
    assert_eq!(controller::current_text(), "second row");
}
