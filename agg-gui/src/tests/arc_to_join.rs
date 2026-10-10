//! `DrawCtx::arc_to` keeps the pen down.
//!
//! Like HTML canvas `arc()` and agg-sharp's `VertexStorage` arc joining, an
//! arc appended after an open subpath continues that subpath (its start point
//! is reached with a line), instead of starting a new one.  These tests fill
//! and stroke paths through the software [`GfxCtx`] and read pixels back, and
//! check the shared path helper ([`crate::draw_ctx::defaults::append_arc`])
//! every backend's `arc_to` goes through.

use std::f64::consts::{FRAC_PI_2, PI};

use agg_rust::basics::{is_move_to, is_stop, VertexSource};
use agg_rust::path_storage::PathStorage;

use super::*;
use crate::draw_ctx::defaults::{append_arc, append_circle};
use crate::draw_ctx::DrawCtx;

const RADIUS: f64 = 12.0;
const LEFT: f64 = 8.0;
const RIGHT: f64 = 56.0;
const BOTTOM: f64 = 8.0;
const TOP: f64 = 48.0;

fn is_blue(pixel: [u8; 4]) -> bool {
    pixel[2] > 200 && pixel[0] < 50 && pixel[1] < 50
}

/// The tab outline from mattercad's side-panel tabs: a rectangle open at the
/// bottom whose top corners are rounded with `arc_to`.
fn top_rounded_rect(ctx: &mut dyn DrawCtx) {
    ctx.move_to(LEFT, BOTTOM);
    ctx.line_to(LEFT, TOP - RADIUS);
    ctx.arc_to(LEFT + RADIUS, TOP - RADIUS, RADIUS, PI, FRAC_PI_2, false);
    ctx.line_to(RIGHT - RADIUS, TOP);
    ctx.arc_to(RIGHT - RADIUS, TOP - RADIUS, RADIUS, FRAC_PI_2, 0.0, false);
    ctx.line_to(RIGHT, BOTTOM);
    ctx.close_path();
}

/// Number of `move_to` commands (subpaths) in `path`.
fn subpath_count(path: &mut PathStorage) -> usize {
    path.rewind(0);
    let (mut x, mut y) = (0.0, 0.0);
    let mut count = 0;
    loop {
        let cmd = path.vertex(&mut x, &mut y);
        if is_stop(cmd) {
            return count;
        }
        if is_move_to(cmd) {
            count += 1;
        }
    }
}

#[test]
fn arc_to_continues_the_subpath_when_filling_a_top_rounded_rect() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        top_rounded_rect(ctx);
        ctx.fill();
    }
    assert!(is_blue(sample(&fb, 10, 28)), "just inside the left middle");
    assert!(
        is_blue(sample(&fb, 32, 10)),
        "just inside the bottom middle"
    );
    assert!(is_blue(sample(&fb, 32, 28)), "centre");
    assert!(is_blue(sample(&fb, 54, 28)), "just inside the right middle");
    // Outside the rounded top-left corner stays background.
    assert!(is_red(sample(&fb, 9, 47)), "outside the rounded corner");
}

#[test]
fn arc_to_strokes_a_line_from_the_pen_to_the_arc_start() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.set_stroke_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.set_line_width(4.0);
        ctx.begin_path();
        // Pen at (10, 10); the arc starts at (30, 40).  Canvas draws the
        // connecting line, which passes through (20, 25).
        ctx.move_to(10.0, 10.0);
        ctx.arc_to(40.0, 40.0, 10.0, PI, 0.0, false);
        ctx.stroke();
    }
    assert!(is_blue(sample(&fb, 20, 25)), "the pen-down connecting line");
}

#[test]
fn append_arc_joins_an_open_subpath_into_one() {
    let mut path = PathStorage::new();
    path.move_to(LEFT, BOTTOM);
    path.line_to(LEFT, TOP - RADIUS);
    append_arc(
        &mut path,
        LEFT + RADIUS,
        TOP - RADIUS,
        RADIUS,
        PI,
        FRAC_PI_2,
        false,
    );
    path.line_to(RIGHT - RADIUS, TOP);
    append_arc(
        &mut path,
        RIGHT - RADIUS,
        TOP - RADIUS,
        RADIUS,
        FRAC_PI_2,
        0.0,
        false,
    );
    path.line_to(RIGHT, BOTTOM);
    assert_eq!(subpath_count(&mut path), 1);
}

#[test]
fn append_arc_starts_a_subpath_on_an_empty_path_and_after_a_close() {
    let mut path = PathStorage::new();
    append_arc(&mut path, 0.0, 0.0, 5.0, 0.0, PI, true);
    assert_eq!(subpath_count(&mut path), 1, "empty path");
    path.close_polygon(agg_rust::basics::PATH_FLAGS_NONE);
    append_arc(&mut path, 20.0, 0.0, 5.0, 0.0, PI, true);
    assert_eq!(subpath_count(&mut path), 2, "after close_path");
}

#[test]
fn append_circle_always_starts_its_own_subpath() {
    let mut path = PathStorage::new();
    path.move_to(0.0, 0.0);
    path.line_to(10.0, 0.0);
    append_circle(&mut path, 30.0, 30.0, 5.0);
    assert_eq!(subpath_count(&mut path), 2);
}

#[test]
fn circle_after_an_open_subpath_fills_only_the_circle() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        // An open, zero-area segment, then a circle far away.  If the circle
        // were joined, the fill would include a wedge back to (2, 2).
        ctx.move_to(2.0, 2.0);
        ctx.line_to(2.0, 60.0);
        ctx.circle(44.0, 44.0, 8.0);
        ctx.fill();
    }
    assert!(is_blue(sample(&fb, 44, 44)), "circle centre");
    assert!(
        is_red(sample(&fb, 20, 30)),
        "no wedge between segment and circle"
    );
}
