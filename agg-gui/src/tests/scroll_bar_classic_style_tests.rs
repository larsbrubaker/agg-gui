//! [`ScrollBarStyle`]'s options for agg-sharp's classic `ScrollBar` look:
//! `corner_radius` (`Some(0.0)` for its square track and thumb) and
//! `snap_on_hover` (the thumb grows to full width the moment it is hovered,
//! with no ease). Rust-only: agg-sharp's classic bar is the reference these
//! options reproduce, so the tests drive a [`ScrollView`] through the
//! production hover and paint paths, as `scroll_bar_appearance_tests` does,
//! and read back its geometry and pixels.

use crate::geometry::Point;
use crate::{
    clock, Event, Framebuffer, GfxCtx, ScrollBarKind, ScrollBarProperties, ScrollBarStyle,
    ScrollBarVisibility, ScrollThumbRest, ScrollView, Size, SizedBox, Widget,
};

/// agg-sharp's `ScrollBar.GrowThumbBy` at device scale 1.
const GROW_THUMB_BY: f64 = 3.0;

/// agg-sharp's classic bar: 15 wide, Solid, flush against the edge, the
/// resting thumb inset by `GrowThumbBy` on both sides of a full-width track.
fn classic() -> ScrollBarStyle {
    ScrollBarStyle {
        bar_width: 15.0,
        floating_width: 15.0,
        handle_min_length: 0.0,
        outer_margin: 0.0,
        inner_margin: 0.0,
        kind: ScrollBarKind::Solid,
        fade_strength: 0.0,
        edge_guard: 0.0,
        thumb_rest: ScrollThumbRest::Inset {
            grow_thumb_by: GROW_THUMB_BY,
        },
        ..ScrollBarStyle::default()
    }
}

/// A 200 x 100 view over 100 x 500 content.
fn make(style: ScrollBarStyle) -> ScrollView {
    let content = SizedBox::new().with_width(100.0).with_height(500.0);
    let mut scroll = ScrollView::new(Box::new(content))
        .with_style(style)
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    scroll.set_bounds(crate::Rect::new(0.0, 0.0, 200.0, 100.0));
    scroll.layout(Size::new(200.0, 100.0));
    scroll
}

fn props(scroll: &ScrollView) -> ScrollBarProperties {
    scroll.scroll_bar_properties()
}

/// One frame of the bars into a fresh transparent 200 x 100 buffer.
fn paint(scroll: &mut ScrollView) -> Framebuffer {
    let mut fb = Framebuffer::new(200, 100);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        scroll.paint_overlay(&mut ctx);
    }
    fb
}

fn mouse_move(scroll: &mut ScrollView, x: f64, y: f64) {
    scroll.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    });
}

fn rgba(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
}

/// The track's bottom-left corner pixel and the middle of its bottom row
/// (track only: the thumb is at the top while the offset is 0).
fn corner_and_edge(scroll: &mut ScrollView) -> ([u8; 4], [u8; 4]) {
    let track = props(scroll).vertical.track;
    let fb = paint(scroll);
    let (x, y) = (track.x as u32, track.y as u32);
    let mid = (track.x + track.width / 2.0) as u32;
    (rgba(&fb, x, y), rgba(&fb, mid, y))
}

#[test]
fn rust_only_corner_radius_defaults_to_half_the_thumb_width() {
    let style = classic();
    assert_eq!(style.corner_radius, None);
    assert_eq!(style.corner_radius_at(0.0), 4.5);
    assert_eq!(style.corner_radius_at(1.0), 7.5);
    let square = ScrollBarStyle {
        corner_radius: Some(0.0),
        ..style
    };
    assert_eq!(square.corner_radius_at(0.0), 0.0);
    assert_eq!(square.corner_radius_at(1.0), 0.0);
}

#[test]
fn rust_only_square_corner_radius_paints_a_square_track() {
    let _clock = clock::scoped_virtual(None);
    let mut rounded = make(classic());
    let (corner, edge) = corner_and_edge(&mut rounded);
    assert!(edge[3] > 0, "the track paints: {edge:?}");
    assert!(
        corner[3] < edge[3] / 2,
        "a rounded corner leaves the corner pixel clear: {corner:?} vs {edge:?}"
    );

    let mut square = make(ScrollBarStyle {
        corner_radius: Some(0.0),
        ..classic()
    });
    let (corner, edge) = corner_and_edge(&mut square);
    assert!(
        close(corner, edge),
        "a square corner paints like the edge: {corner:?} vs {edge:?}"
    );
}

/// Hover the vertical bar and paint one frame with the clock held.
fn hovered_thumb_width(style: ScrollBarStyle) -> (f64, ScrollView) {
    let mut scroll = make(style);
    let strip = props(&scroll).vertical.strip;
    assert_eq!(props(&scroll).vertical.thumb.width, 9.0);
    mouse_move(&mut scroll, strip.x + 7.0, 50.0);
    paint(&mut scroll);
    (props(&scroll).vertical.thumb.width, scroll)
}

#[test]
fn rust_only_the_classic_thumb_eases_to_full_width_by_default() {
    let _clock = clock::scoped_virtual(None);
    let (width, _) = hovered_thumb_width(classic());
    assert_eq!(width, 9.0, "no time passed, so the ease has not moved");
}

#[test]
fn rust_only_snap_on_hover_grows_the_thumb_at_once() {
    let _clock = clock::scoped_virtual(None);
    let style = ScrollBarStyle {
        snap_on_hover: true,
        ..classic()
    };
    let (width, mut scroll) = hovered_thumb_width(style);
    assert_eq!(width, 15.0, "the thumb fills the bar on the first frame");
    assert!(!scroll.needs_draw(), "no ease is left running");

    // leaving shrinks it back at once
    mouse_move(&mut scroll, 20.0, 50.0);
    paint(&mut scroll);
    assert_eq!(props(&scroll).vertical.thumb.width, 9.0);
}
