//! Tests for [`crate::socket_style`]: each socket shape, the ring, the
//! multi-input spacing and both noodle looks, against MatterCAD's
//! `NoodleRenderer` geometry (outline grown by 1, sqrt 2 at a diamond's
//! points; noodle 7 / 3 with a 5 dot; dashes 8 / 8).

use agg_gui::Color;

use crate::socket_style::*;
use crate::test_recorder::{same, Op, Recorder};

const RED: Color = Color::rgb(1.0, 0.0, 0.0);
const BORDER: Color = Color::rgb(0.0, 0.0, 1.0);

fn paint(shape: SocketShape, stretch: f64, style: NoodleStyle) -> SocketPaint {
    SocketPaint {
        shape,
        color: RED,
        stretch,
        style,
        border: BORDER,
    }
}

fn draw(p: SocketPaint) -> Recorder {
    let mut r = Recorder::default();
    draw_socket(&mut r, [10.0, 20.0], &p);
    r
}

#[test]
fn the_default_socket_keeps_the_original_circle() {
    let r = draw(paint(SocketShape::Circle, 0.0, NoodleStyle::Simple));
    assert_eq!(r.shots.len(), 2);
    assert!(r.shots[0].fill && same(r.shots[0].color, RED));
    assert_eq!(
        r.shots[0].path,
        vec![Op::Circle([10.0, 20.0], crate::draw::SOCKET_RADIUS)]
    );
    assert!(!r.shots[1].fill && same(r.shots[1].color, BORDER));
    assert_eq!(r.shots[1].width, 1.0);
}

#[test]
fn a_node_designer_circle_has_a_dark_outline() {
    let r = draw(paint(SocketShape::Circle, 0.0, NoodleStyle::NodeDesigner));
    let fills: Vec<_> = r.fills().collect();
    assert_eq!(fills.len(), 2);
    assert!(same(fills[0].color, outline_color()));
    assert_eq!(fills[0].path, vec![Op::Circle([10.0, 20.0], 7.0)]);
    assert!(same(fills[1].color, RED));
    assert_eq!(fills[1].path, vec![Op::Circle([10.0, 20.0], 6.0)]);
    assert_eq!(r.strokes().count(), 0);
}

#[test]
fn a_bar_socket_is_an_outlined_upright_rect() {
    let r = draw(paint(SocketShape::Bar, 0.0, NoodleStyle::NodeDesigner));
    let fills: Vec<_> = r.fills().collect();
    assert_eq!(fills[0].path, vec![Op::Rect([6.0, 12.0], [8.0, 16.0])]);
    assert_eq!(fills[1].path, vec![Op::Rect([7.0, 13.0], [6.0, 14.0])]);
    assert!(same(fills[1].color, RED));
}

fn diamond_top(shot: &crate::test_recorder::Shot) -> f64 {
    match shot.path[0] {
        Op::MoveTo([_, y]) => y,
        ref other => panic!("a diamond starts at its top point, got {other:?}"),
    }
}

#[test]
fn a_field_input_is_a_diamond_with_a_centre_dot() {
    let r = draw(paint(
        SocketShape::DiamondDot,
        0.0,
        NoodleStyle::NodeDesigner,
    ));
    let fills: Vec<_> = r.fills().collect();
    assert_eq!(fills.len(), 3);
    assert!((diamond_top(fills[0]) - (20.0 + 7.0 + 2f64.sqrt())).abs() < 1e-9);
    assert_eq!(diamond_top(fills[1]), 27.0);
    assert!(same(fills[1].color, RED));
    assert_eq!(
        fills[2].path,
        vec![Op::Circle([10.0, 20.0], FIELD_DOT_RADIUS)]
    );
    assert!(same(fills[2].color, outline_color()));
}

#[test]
fn a_field_output_is_a_diamond_without_a_dot() {
    let r = draw(paint(SocketShape::Diamond, 0.0, NoodleStyle::NodeDesigner));
    assert_eq!(r.fills().count(), 2);
    assert!(r.fills().all(|s| matches!(s.path[0], Op::MoveTo(_))));
}

#[test]
fn shapes_in_the_simple_style_outline_in_the_border_colour() {
    let r = draw(paint(SocketShape::Bar, 0.0, NoodleStyle::Simple));
    assert!(same(r.shots[0].color, BORDER));
}

#[test]
fn a_multi_input_pill_spaces_its_noodles_ten_apart() {
    assert_eq!(multi_input_stretch(0), 0.0);
    assert_eq!(multi_input_stretch(1), 0.0);
    assert_eq!(multi_input_stretch(3), 10.0);
    // The first noodle lands highest (Y up), each next 10 lower.
    assert_eq!(
        (0..3).map(|i| landing_offset(i, 3)).collect::<Vec<_>>(),
        vec![10.0, 0.0, -10.0]
    );
    assert_eq!(landing_offset(0, 1), 0.0);

    let r = draw(paint(SocketShape::Circle, 5.0, NoodleStyle::NodeDesigner));
    let fills: Vec<_> = r.fills().collect();
    assert_eq!(
        fills[0].path,
        vec![Op::RoundedRect([3.0, 8.0], [14.0, 24.0], 7.0)]
    );
    assert_eq!(
        fills[1].path,
        vec![Op::RoundedRect([4.0, 9.0], [12.0, 22.0], 6.0)]
    );
}

#[test]
fn the_ring_sits_just_outside_the_outline() {
    let mut r = Recorder::default();
    draw_socket_ring(
        &mut r,
        [0.0, 0.0],
        &paint(SocketShape::Circle, 0.0, NoodleStyle::NodeDesigner),
        RED,
    );
    assert_eq!(r.shots.len(), 1);
    assert!(!r.shots[0].fill && same(r.shots[0].color, RED));
    assert_eq!(r.shots[0].width, RING_WIDTH);
    assert_eq!(r.shots[0].path, vec![Op::Circle([0.0, 0.0], 8.0)]);
}

#[test]
fn the_simple_noodle_is_one_two_unit_curve() {
    let mut r = Recorder::default();
    draw_noodle(
        &mut r,
        [0.0, 0.0],
        [100.0, 0.0],
        RED,
        false,
        NoodleStyle::Simple,
    );
    assert_eq!(r.shots.len(), 1);
    assert_eq!(r.shots[0].width, 2.0);
    assert!(r.shots[0].dash.is_empty());
    // Control points a quarter of the length out horizontally.
    assert_eq!(
        r.shots[0].path[1],
        Op::Cubic([25.0, 0.0], [75.0, 0.0], [100.0, 0.0])
    );
}

#[test]
fn the_node_designer_noodle_has_an_edge_a_core_and_a_middle_dot() {
    let mut r = Recorder::default();
    draw_noodle(
        &mut r,
        [0.0, 0.0],
        [100.0, 40.0],
        RED,
        false,
        NoodleStyle::NodeDesigner,
    );
    let strokes: Vec<_> = r.strokes().collect();
    assert_eq!(strokes.len(), 2);
    assert!(same(strokes[0].color, outline_color()) && strokes[0].width == 7.0);
    assert!(same(strokes[1].color, RED) && strokes[1].width == 3.0);
    let fills: Vec<_> = r.fills().collect();
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].path, vec![Op::Circle([50.0, 20.0], 5.0)]);
}

#[test]
fn a_dashed_noodle_dashes_eight_on_eight_off_and_resets() {
    let mut r = Recorder::default();
    draw_noodle(
        &mut r,
        [0.0, 0.0],
        [100.0, 0.0],
        RED,
        true,
        NoodleStyle::NodeDesigner,
    );
    assert!(r.strokes().all(|s| s.dash == vec![8.0, 8.0]));
    draw_noodle(
        &mut r,
        [0.0, 0.0],
        [100.0, 0.0],
        RED,
        false,
        NoodleStyle::Simple,
    );
    assert!(r.shots.last().unwrap().dash.is_empty());
}

#[test]
fn a_dashed_noodle_strokes_with_butt_caps_and_restores_round() {
    let mut r = Recorder::default();
    draw_noodle(
        &mut r,
        [0.0, 0.0],
        [100.0, 0.0],
        RED,
        true,
        NoodleStyle::NodeDesigner,
    );
    assert!(r.strokes().all(|s| s.cap == agg_gui::LineCap::Butt));
    draw_noodle(
        &mut r,
        [0.0, 0.0],
        [100.0, 0.0],
        RED,
        false,
        NoodleStyle::NodeDesigner,
    );
    let last = r.strokes().last().unwrap();
    assert_eq!(last.cap, agg_gui::LineCap::Round);
}

#[test]
fn a_dashed_noodle_shows_its_gaps_in_pixels() {
    // A straight noodle along y = 20: dashes cover x 0-8, 16-24, ...; the
    // gaps 8-16, 24-32, ... must stay empty. A round cap on the 7-unit
    // edge would reach 3.5 into each gap, leaving only x 11.5-12.5 clear.
    let mut fb = agg_gui::Framebuffer::new(110, 40);
    {
        let mut ctx = agg_gui::GfxCtx::new(&mut fb);
        draw_noodle(
            &mut ctx,
            [0.0, 20.0],
            [100.0, 20.0],
            RED,
            true,
            NoodleStyle::NodeDesigner,
        );
    }
    let alpha = |x: u32, y: u32| fb.pixels()[((y * fb.width() + x) * 4 + 3) as usize];
    assert!(alpha(4, 20) > 0, "inside the first dash");
    assert!(alpha(20, 20) > 0, "inside the second dash");
    for x in [9, 10, 11, 12, 13, 14, 25, 26, 27, 28, 29, 30] {
        assert_eq!(alpha(x, 20), 0, "gap pixel x = {x} is painted");
    }
}
