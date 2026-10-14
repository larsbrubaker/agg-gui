//! Rust-only tests of the name pick (agg-gui-automation `src/name_pick.rs`
//! over `src/paint_geometry.rs`): a lookup by name sees what painting shows
//! (the overlay pass, slot clips, pixel snapping), aims at the visible part
//! of a widget whose centre a clip hides, takes the first of equal-sized
//! widgets despite rounding, prefers a widget whose centre shows, and
//! answers in one look from a runner (`AutomationRunner::find_now`).

mod support;

use agg_gui::{Point, Rect, Size};
use agg_gui_automation::name_pick::{aim_point, find_on_screen, hit_placement};
use agg_gui_automation::tree_query::{placement, screen_rect, WidgetHandle};
use agg_gui_automation::{AutomationConfig, AutomationRunner, HeadlessDriver, WaitOpts};
use support::Switchable;

const VIEWPORT: Size = Size {
    width: 300.0,
    height: 200.0,
};

fn sw(name: &str, x: f64, y: f64, w: f64, h: f64) -> Switchable {
    Switchable::new(name, Rect::new(x, y, w, h))
}

fn root_with(children: Vec<Switchable>) -> Switchable {
    children
        .into_iter()
        .fold(sw("root", 0.0, 0.0, 300.0, 200.0), |root, child| {
            root.with_child(Box::new(child))
        })
}

/// A box holding a modal host placed outside the box's clip, with an "OK"
/// button in it.
fn modal_outside_its_container(defers: bool) -> Switchable {
    let host = sw("modal host", 150.0, 0.0, 100.0, 100.0)
        .with_child(Box::new(sw("OK", 10.0, 10.0, 20.0, 20.0)));
    let host = if defers {
        host.deferring_paint_to_overlay()
    } else {
        host
    };
    root_with(vec![
        sw("box", 0.0, 0.0, 100.0, 100.0).with_child(Box::new(host))
    ])
}

#[test]
fn rust_only_a_modal_painted_in_the_overlay_pass_escapes_its_ancestors_clips() {
    let root = modal_outside_its_container(true);
    let ok = find_on_screen(&root, "OK", VIEWPORT).expect("the modal's button shows");
    assert_eq!(ok.handle.path(), &[0, 0, 0]);
    assert_eq!(
        aim_point(&root, &ok, VIEWPORT),
        Some(Point::new(170.0, 20.0))
    );

    // The same host painting in place is clipped away by the box.
    let root = modal_outside_its_container(false);
    assert!(find_on_screen(&root, "OK", VIEWPORT).is_none());
}

#[test]
fn rust_only_a_slot_clip_cuts_a_child_as_painting_does() {
    let row = sw("row", 0.0, 0.0, 200.0, 50.0)
        .clipping_child(0, Rect::new(0.0, 0.0, 50.0, 50.0))
        .with_child(Box::new(sw("wide", 0.0, 0.0, 100.0, 50.0)));
    let root = root_with(vec![row]);
    let wide = WidgetHandle::new(&root, &[0, 0]).expect("wide");
    let placed = placement(&root, &wide, VIEWPORT).expect("attached");
    assert_eq!(placed.screen_rect, Rect::new(0.0, 0.0, 100.0, 50.0));
    assert_eq!(placed.clipped_rect, Rect::new(0.0, 0.0, 50.0, 50.0));
}

#[test]
fn rust_only_a_widget_with_integer_bounds_is_found_where_it_paints() {
    // The outer offset leaves a half pixel; the inner widget's exact offset
    // puts it at (10.9, 20.6), and its transform goes to the nearest pixel
    // (11, 21), as painting snaps it.
    let outer = sw("outer", 0.5, 0.0, 100.0, 100.0).with_child(Box::new(
        sw("snapped", 10.4, 20.6, 10.0, 10.0).with_integer_bounds(),
    ));
    let root = root_with(vec![outer]);
    let snapped = WidgetHandle::new(&root, &[0, 0]).expect("snapped");
    assert_eq!(
        screen_rect(&root, &snapped),
        Some(Rect::new(11.0, 21.0, 10.0, 10.0))
    );
}

#[test]
fn rust_only_a_widget_whose_centre_is_clipped_is_aimed_at_in_its_visible_part() {
    // A row half scrolled out of the bottom of its list: 10 of its 40 units
    // show, its centre (y = -10) is hidden.
    let list =
        sw("list", 0.0, 0.0, 200.0, 100.0).with_child(Box::new(sw("row", 0.0, -30.0, 200.0, 40.0)));
    let root = root_with(vec![list]);
    let row = find_on_screen(&root, "row", VIEWPORT).expect("part of the row shows");
    assert_eq!(row.offset_hint, Point::new(100.0, 35.0));
    assert_eq!(
        aim_point(&root, &row, VIEWPORT),
        Some(Point::new(100.0, 5.0))
    );

    // A row whose centre shows is aimed at its centre, as C# aims.
    let list =
        sw("list", 0.0, 0.0, 200.0, 100.0).with_child(Box::new(sw("row", 0.0, -10.0, 200.0, 40.0)));
    let root = root_with(vec![list]);
    let row = find_on_screen(&root, "row", VIEWPORT).expect("the row shows");
    assert_eq!(row.offset_hint, Point::new(100.0, 20.0));
}

#[test]
fn rust_only_the_first_of_equal_sized_widgets_wins_despite_rounding() {
    // Equal widgets: moved to window space, the second's width comes out a
    // few ulps larger ((0.1 + 0.2) - 0.1 > 0.2).
    let root = root_with(vec![
        sw("twin", 0.0, 0.0, 0.2, 10.0),
        sw("twin", 0.1, 50.0, 0.2, 10.0),
    ]);
    let areas: Vec<f64> = [0, 1]
        .iter()
        .map(|&i| {
            let hit = agg_gui_automation::NamedHit::widget(
                WidgetHandle::new(&root, &[i]).expect("twin"),
                Rect::new(0.0, 0.0, 0.2, 10.0),
            );
            hit_placement(&root, &hit, VIEWPORT)
                .expect("attached")
                .visible_area()
        })
        .collect();
    assert!(areas[1] > areas[0], "the setup has rounding noise");
    let twin = find_on_screen(&root, "twin", VIEWPORT).expect("both show");
    assert_eq!(twin.handle.path(), &[0]);
}

#[test]
fn rust_only_a_widget_whose_centre_shows_is_preferred_to_a_larger_clipped_one() {
    // The panel's own bounds take a press at the first one's centre, but its
    // narrower children clip hides that centre.
    let panel = sw("panel", 0.0, 0.0, 100.0, 100.0)
        .clipping_children_to(Rect::new(0.0, 0.0, 100.0, 40.0))
        .with_child(Box::new(sw("dup", 0.0, 0.0, 100.0, 100.0)));
    let root = root_with(vec![panel, sw("dup", 150.0, 0.0, 20.0, 20.0)]);
    let dup = find_on_screen(&root, "dup", VIEWPORT).expect("both show");
    assert_eq!(dup.handle.path(), &[1]);
}

#[test]
fn rust_only_find_now_answers_in_one_look_inside_a_wait() {
    let list =
        sw("list", 0.0, 0.0, 200.0, 100.0).with_child(Box::new(sw("row", 0.0, -30.0, 200.0, 40.0)));
    let root = root_with(vec![list, sw("hidden", 0.0, 300.0, 10.0, 10.0)]);
    let mut runner = AutomationRunner::attach(
        HeadlessDriver::new(Box::new(root), 300, 200),
        AutomationConfig::default(),
    );
    let frames = runner.driver().frames();
    let row = runner.find_now("row").expect("the row shows");
    assert_eq!(runner.driver().frames(), frames, "no frame was pumped");
    assert!(runner.find_now("hidden").is_none());
    assert!(runner.find_now("missing").is_none());
    assert_eq!(runner.aim_point(&row), Some(Point::new(100.0, 5.0)));

    // The waiting lookup makes the same choice and aims the same way.
    let waited = runner
        .get_widget_hit_by_name("row", &WaitOpts::secs(0.0))
        .expect("found");
    assert_eq!(waited, row);

    let seen = runner.wait_until(|r| r.find_now("row").is_some(), 1.0, 10);
    assert!(seen);
    runner.mark_test_complete();
}
