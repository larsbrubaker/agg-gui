//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ScrollBarAppearanceTests.cs`, on
//! [`ScrollView`]: bar widths and margins, the handle minimum, floating
//! (auto-hide, hover growth, the grab margin), the corner between the bars
//! and the edge fade.
//!
//! agg-sharp's bar is a widget whose `Track` and `Thumb` children the tests
//! measure; agg-gui paints its bars, so the same rects come from
//! [`ScrollView::scroll_bar_properties`]. agg-sharp's bar bounds are the
//! query's `strip`, its `ScrollArea.DeviceMargin` the bar's `reserved`, and
//! its bar sits flush against the view's edge, which is agg-gui's
//! `edge_guard: 0` (no room kept for a resize grip). agg-sharp's option names
//! map onto [`ScrollBarStyle`]: `BarWidth` is `bar_width`, `FloatingWidth`
//! `floating_width`, `InnerMargin`/`OuterMargin` the margins,
//! `HandleMinLength` `handle_min_length`, `Floating` the `Floating` kind and
//! `ScrollBarWidth`'s legacy default a `Solid` bar; `ScrollBar.Clock` is the
//! virtual UI clock, and `StepFade` a paint after advancing it.
//! agg-sharp's own defaults (no margins, no handle minimum, no fade) are
//! [`legacy`]; its classic resting thumb, inset by `GrowThumbBy` on both
//! sides of a full-width track, is [`classic`] (`ScrollThumbRest::Inset`),
//! which the tests reading the resting thumb's band use.
//!
//! Behaviour tests where agg-gui's bar differs by design:
//! - `DefaultsKeepTheLegacyGeometry`: the bar adds no children, so the
//!   child-count leg checks the view's one child stays alone.
//! - `AFloatingBarTakesNoRoomAndIsThinUntilHovered`: agg-gui paints nothing
//!   of a dormant auto-hiding bar, so the "no track" leg is its opacity 0.
//! - `AFloatingBarIsHoveredFromItsGrabMarginButNotBeyond`: agg-gui's track
//!   grows with the thumb, so "the track is not widened by the grab margin"
//!   is checked once the hover growth has finished.
//! - `ContentGrowingUnderAFloatingBarDoesNotBringItUp`: agg-gui keeps the
//!   offset from the top, which content growth leaves alone, so the leg that
//!   agg-sharp's `ScrollPosition` moved is that the offset did not.
//! - `TheEdgeFadeIsOptInAndFollowsTheScroll`: the fade is painted, not a
//!   child, so the child-order legs are N/A; the faded edges and the colour
//!   override are checked. Fading to the background without an override is
//!   `tests::scroll_view::test_scroll_fade_uses_window_background`.
//!
//! Not ported: `ColorOverridesPaintTheBar` (agg-gui has no per-bar colour
//! overrides and MatterCAD sets none on a scroll bar: its only `TrackColor` /
//! `ThumbColor` writes are on sliders).

use std::time::Duration;

use super::scroll_fixtures::Resizable;
use crate::geometry::Point;
use crate::{
    clock, Event, Framebuffer, GfxCtx, Modifiers, MouseButton, ScrollBarKind, ScrollBarProperties,
    ScrollBarStyle, ScrollBarVisibility, ScrollThumbRest, ScrollView, Size, SizedBox, Widget,
    SCROLL_BAR_FADE_SECS, SCROLL_BAR_GROW_SECS,
};

/// agg-sharp's `ScrollBar.GrowThumbBy` at device scale 1.
const GROW_THUMB_BY: f64 = 3.0;

/// agg-sharp's `ScrollBar` defaults: a 15 wide Solid bar flush against the
/// view's edge, no margins, no handle minimum and no edge fade.
fn legacy() -> ScrollBarStyle {
    ScrollBarStyle {
        bar_width: 15.0,
        floating_width: 15.0,
        handle_min_length: 0.0,
        outer_margin: 0.0,
        inner_margin: 0.0,
        kind: ScrollBarKind::Solid,
        fade_strength: 0.0,
        edge_guard: 0.0,
        ..ScrollBarStyle::default()
    }
}

/// [`legacy`] with agg-sharp's classic resting thumb (`ScrollBar.ThumbBand`
/// off the floating path).
fn classic() -> ScrollBarStyle {
    ScrollBarStyle {
        thumb_rest: ScrollThumbRest::Inset {
            grow_thumb_by: GROW_THUMB_BY,
        },
        ..legacy()
    }
}

/// agg-sharp's floating test setup: `BarWidth = 10`, `FloatingWidth = 2`,
/// `Floating = true`.
fn floating() -> ScrollBarStyle {
    ScrollBarStyle {
        bar_width: 10.0,
        floating_width: 2.0,
        kind: ScrollBarKind::Floating,
        ..legacy()
    }
}

/// A 200 x 100 view over `content_width` x `content_height` content.
fn make(content_width: f64, content_height: f64, style: ScrollBarStyle) -> ScrollView {
    let content = SizedBox::new()
        .with_width(content_width)
        .with_height(content_height);
    let mut scroll = ScrollView::new(Box::new(content))
        .with_style(style)
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    relayout(&mut scroll);
    scroll
}

fn relayout(scroll: &mut ScrollView) {
    scroll.layout(Size::new(200.0, 100.0));
}

fn restyle(scroll: ScrollView, style: ScrollBarStyle) -> ScrollView {
    let mut scroll = scroll.with_style(style);
    relayout(&mut scroll);
    scroll
}

fn props(scroll: &ScrollView) -> ScrollBarProperties {
    scroll.scroll_bar_properties()
}

/// One frame of the bars (agg-sharp's `StepFade` / `OnDraw`).
fn paint(scroll: &mut ScrollView) {
    let mut fb = Framebuffer::new(200, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    scroll.paint_overlay(&mut ctx);
}

fn advance(secs: f64) {
    clock::advance(Duration::from_secs_f64(secs));
}

fn mouse_move(scroll: &mut ScrollView, x: f64, y: f64) {
    scroll.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    });
}

fn press(scroll: &mut ScrollView, x: f64, y: f64) {
    scroll.on_event(&Event::MouseDown {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn release(scroll: &mut ScrollView, x: f64, y: f64) {
    scroll.on_event(&Event::MouseUp {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

/// Drags the vertical thumb from its middle by `dy` (agg-sharp's `MoveThumb`).
fn move_thumb(scroll: &mut ScrollView, dy: f64) {
    let thumb = props(scroll).vertical.thumb;
    let (x, y) = (thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
    press(scroll, x, y);
    mouse_move(scroll, x, y + dy);
    release(scroll, x, y + dy);
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} within {tolerance}, got {actual}"
    );
}

#[test]
fn defaults_keep_the_legacy_geometry() {
    let scroll = make(100.0, 500.0, classic());
    let p = props(&scroll);
    let bar = p.vertical;
    assert_eq!(classic().kind, ScrollBarKind::Solid);
    assert_eq!(bar.strip.width, classic().bar_width);
    assert_eq!(bar.strip.height, 100.0);
    assert_eq!(bar.thumb.x - bar.strip.x, GROW_THUMB_BY);
    close(bar.thumb.height, 100.0 * 100.0 / 500.0, 1e-9);
    close(bar.reserved, classic().bar_width, 1e-9);
    // the strip is reserved: the content stops where it begins
    close(p.viewport.x + p.viewport.width, bar.strip.x, 1e-9);
    assert_eq!(bar.fade_target, 1.0);
    let mut scroll = scroll;
    paint(&mut scroll);
    assert_eq!(props(&scroll).vertical.opacity, 1.0);
    assert_eq!(scroll.children().len(), 1);
}

#[test]
fn width_and_margins_widen_the_strip_and_reserve_it() {
    let style = ScrollBarStyle {
        bar_width: 20.0,
        inner_margin: 4.0,
        outer_margin: 3.0,
        ..classic()
    };
    let scroll = make(100.0, 500.0, style);
    let p = props(&scroll);
    let bar = p.vertical;
    assert_eq!(bar.strip.width, 27.0);
    close(bar.reserved, 27.0, 1e-9);
    close(p.viewport.x + p.viewport.width, bar.strip.x, 1e-9);

    // the track sits past the inner margin; the outer margin is left clear at the view's edge
    assert_eq!(bar.track.x - bar.strip.x, 4.0);
    assert_eq!(bar.track.x + bar.track.width - bar.strip.x, 24.0);
    assert_eq!(bar.thumb.x - bar.strip.x, 4.0 + GROW_THUMB_BY);
    assert_eq!(
        bar.thumb.x + bar.thumb.width - bar.strip.x,
        24.0 - GROW_THUMB_BY
    );
}

#[test]
fn a_narrow_bar_keeps_a_visible_thumb() {
    // the legacy inset on both sides would eat a bar this thin whole
    let style = ScrollBarStyle {
        bar_width: 2.0 * GROW_THUMB_BY,
        ..classic()
    };
    let scroll = make(100.0, 500.0, style);
    close(props(&scroll).vertical.thumb.width, GROW_THUMB_BY, 1e-9);
}

#[test]
fn handle_min_length_holds_the_thumb_and_the_drag_still_reaches_the_end() {
    let scroll = make(100.0, 10000.0, legacy());
    close(props(&scroll).vertical.thumb.height, 1.0, 1e-9);
    let mut scroll = restyle(
        scroll,
        ScrollBarStyle {
            handle_min_length: 30.0,
            ..legacy()
        },
    );
    assert_eq!(props(&scroll).vertical.thumb.height, 30.0);
    move_thumb(&mut scroll, -1000.0);
    close(scroll.scroll_offset(), scroll.max_scroll_value(), 1e-6);
}

#[test]
fn a_floating_bar_takes_no_room_and_is_thin_until_hovered() {
    let _clock = clock::scoped_virtual(None);
    let mut scroll = make(100.0, 500.0, floating());
    let p = props(&scroll);
    assert_eq!(p.viewport.width, 200.0);
    assert!(p.vertical.showing);

    // dormant: a thin thumb against the outer edge and nothing painted
    let bar = p.vertical;
    assert_eq!(bar.thumb.x - bar.strip.x, 8.0);
    assert_eq!(bar.thumb.x + bar.thumb.width - bar.strip.x, 10.0);
    paint(&mut scroll);
    assert_eq!(props(&scroll).vertical.opacity, 0.0);

    // hovered: it grows to the full thickness over the grow time
    let strip = bar.strip;
    mouse_move(&mut scroll, strip.x + 5.0, 50.0);
    paint(&mut scroll);
    advance(SCROLL_BAR_GROW_SECS);
    paint(&mut scroll);
    let bar = props(&scroll).vertical;
    assert_eq!(bar.thumb.x - bar.strip.x, 0.0);
    assert_eq!(bar.thumb.x + bar.thumb.width - bar.strip.x, 10.0);

    let scroll = restyle(
        scroll,
        ScrollBarStyle {
            kind: ScrollBarKind::Solid,
            ..floating()
        },
    );
    let p = props(&scroll);
    close(p.viewport.x + p.viewport.width, p.vertical.strip.x, 1e-9);
    close(p.vertical.strip.width, 10.0, 1e-9);
}

#[test]
fn a_floating_bar_auto_hides_and_fades_in_on_hover_but_not_on_scroll() {
    let _clock = clock::scoped_virtual(None);
    let style = ScrollBarStyle {
        kind: ScrollBarKind::Floating,
        ..legacy()
    };
    let mut scroll = make(100.0, 500.0, style);
    let strip = props(&scroll).vertical.strip;
    assert_eq!(props(&scroll).vertical.opacity, 0.0);
    assert_eq!(props(&scroll).vertical.fade_target, 0.0);

    // hover fades it in over the fade time, eased out (cubic)
    mouse_move(&mut scroll, strip.x + 5.0, 50.0);
    assert_eq!(props(&scroll).vertical.fade_target, 1.0);
    paint(&mut scroll);
    advance(SCROLL_BAR_FADE_SECS / 2.0);
    paint(&mut scroll);
    close(props(&scroll).vertical.opacity, 0.875, 1e-9);
    advance(SCROLL_BAR_FADE_SECS / 2.0);
    paint(&mut scroll);
    assert!(!scroll.needs_draw());
    assert_eq!(props(&scroll).vertical.opacity, 1.0);

    // leaving fades it back out
    mouse_move(&mut scroll, 500.0, 50.0);
    assert_eq!(props(&scroll).vertical.fade_target, 0.0);
    paint(&mut scroll);

    // a scroll alone does not bring it up: only hover or a drag does
    scroll.set_scroll_offset(50.0);
    relayout(&mut scroll);
    assert_eq!(props(&scroll).vertical.fade_target, 0.0);
    advance(SCROLL_BAR_FADE_SECS);
    paint(&mut scroll);
    assert!(!scroll.needs_draw());
    assert_eq!(props(&scroll).vertical.opacity, 0.0);

    // shown always, a floating bar does not hide
    scroll.set_bar_visibility(ScrollBarVisibility::AlwaysVisible);
    assert_eq!(props(&scroll).vertical.fade_target, 1.0);
}

#[test]
fn a_floating_bar_fades_in_and_out_over_time_even_after_a_long_idle() {
    let _clock = clock::scoped_virtual(None);
    let style = ScrollBarStyle {
        floating_width: 2.0,
        kind: ScrollBarKind::Floating,
        ..legacy()
    };
    let mut scroll = make(100.0, 500.0, style);
    let strip = props(&scroll).vertical.strip;

    // not hovered: drawn, and still hidden and thin
    paint(&mut scroll);
    assert_eq!(props(&scroll).vertical.opacity, 0.0);

    // a long idle, then the pointer comes onto the bar: it starts fading in rather than popping up
    advance(5.0);
    mouse_move(&mut scroll, strip.x + strip.width - 5.0, 50.0);
    paint(&mut scroll);
    assert_eq!(props(&scroll).vertical.opacity, 0.0);
    advance(SCROLL_BAR_FADE_SECS / 2.0);
    paint(&mut scroll);
    let opacity = props(&scroll).vertical.opacity;
    assert!(opacity > 0.5 && opacity < 1.0, "half way in: {opacity}");
    advance(SCROLL_BAR_FADE_SECS);
    paint(&mut scroll);
    let bar = props(&scroll).vertical;
    assert_eq!(bar.opacity, 1.0);
    close(bar.thumb.width, style.bar_width, 1e-9);

    // a long hover, then the pointer leaves: it fades out over the fade time and thins back down
    advance(5.0);
    mouse_move(&mut scroll, 50.0, 50.0);
    paint(&mut scroll);
    assert_eq!(props(&scroll).vertical.opacity, 1.0);
    advance(SCROLL_BAR_FADE_SECS / 2.0);
    paint(&mut scroll);
    let opacity = props(&scroll).vertical.opacity;
    assert!(opacity > 0.0 && opacity < 0.5, "half way out: {opacity}");
    advance(SCROLL_BAR_FADE_SECS);
    paint(&mut scroll);
    let bar = props(&scroll).vertical;
    assert_eq!(bar.opacity, 0.0);
    close(bar.thumb.width, 2.0, 1e-9);
}

#[test]
fn a_floating_bar_is_hovered_from_its_grab_margin_but_not_beyond() {
    // agg-gui's hover zone is the bar plus a 6 px grab margin on the content side, so a 10 px bar shows
    // from 16 px in. The margin widens only what the pointer finds, not where the bar is drawn.
    let _clock = clock::scoped_virtual(None);
    let mut scroll = make(100.0, 500.0, floating());
    let edge = {
        let strip = props(&scroll).vertical.strip;
        strip.x + strip.width
    };

    mouse_move(&mut scroll, edge - 20.0, 50.0);
    assert_eq!(props(&scroll).vertical.fade_target, 0.0);

    mouse_move(&mut scroll, edge - 14.0, 50.0);
    assert_eq!(props(&scroll).vertical.fade_target, 1.0);
    let bar = props(&scroll).vertical;
    assert_eq!(bar.thumb.x + bar.thumb.width - bar.strip.x, 10.0);
    paint(&mut scroll);
    advance(SCROLL_BAR_GROW_SECS);
    paint(&mut scroll);
    let bar = props(&scroll).vertical;
    assert_eq!(bar.track.x - bar.strip.x, 0.0);

    mouse_move(&mut scroll, edge - 20.0, 50.0);
    assert_eq!(props(&scroll).vertical.fade_target, 0.0);
}

#[test]
fn a_press_in_the_grab_margin_beside_the_thumb_drags_it_and_above_it_pages() {
    // agg-gui's pos_on_thumb counts the grab margin: level with the thumb, a press there grabs it. Only a
    // press further along the track pages, towards the press.
    let mut scroll = make(100.0, 500.0, floating());
    let strip = props(&scroll).vertical.strip;
    // the thumb is 20 high at the top of the 100 high track; 14 in from the edge is in the margin
    let x = strip.x + strip.width - 14.0;
    assert_eq!(props(&scroll).vertical.thumb.y, 80.0);

    mouse_move(&mut scroll, x, 90.0);
    press(&mut scroll, x, 90.0);
    assert!(props(&scroll).vertical.dragging);
    assert_eq!(scroll.scroll_offset(), 0.0);
    mouse_move(&mut scroll, x, 70.0);
    assert!(scroll.scroll_offset() > 0.0);
    release(&mut scroll, x, 70.0);
    assert!(!props(&scroll).vertical.dragging);

    // below the thumb, in the margin, pages down rather than up
    scroll.set_scroll_offset(0.0);
    relayout(&mut scroll);
    mouse_move(&mut scroll, x, 30.0);
    press(&mut scroll, x, 30.0);
    assert!(!props(&scroll).vertical.dragging);
    assert!(scroll.scroll_offset() > 0.0);
    release(&mut scroll, x, 30.0);
}

#[test]
fn a_press_in_the_horizontal_grab_margin_beside_the_thumb_drags_it_and_past_it_pages() {
    let content = SizedBox::new().with_width(600.0).with_height(50.0);
    let h_offset = std::rc::Rc::new(std::cell::Cell::new(0.0));
    let mut scroll = ScrollView::new(Box::new(content))
        .horizontal(true)
        .with_h_offset_cell(std::rc::Rc::clone(&h_offset))
        .with_style(floating())
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    relayout(&mut scroll);
    let bar = props(&scroll).horizontal;
    // the thumb spans the first third of the 200 wide track; 14 up from the edge is in the margin
    close(bar.thumb.width, 200.0 / 3.0, 1e-9);
    let y = bar.strip.y + 14.0;

    mouse_move(&mut scroll, 30.0, y);
    press(&mut scroll, 30.0, y);
    assert!(props(&scroll).horizontal.dragging);
    assert_eq!(h_offset.get(), 0.0);
    release(&mut scroll, 30.0, y);

    // past the thumb, in the margin, pages right
    mouse_move(&mut scroll, 150.0, y);
    press(&mut scroll, 150.0, y);
    assert!(!props(&scroll).horizontal.dragging);
    assert!(h_offset.get() > 0.0);
    release(&mut scroll, 150.0, y);
}

#[test]
fn content_growing_under_a_floating_bar_does_not_bring_it_up() {
    // A grown content once read as a scroll, showed the bar and asked for frames to fade it in and out
    // again, so a reactive app redrew after every layout.
    let _clock = clock::scoped_virtual(None);
    let (content, size) = Resizable::new(100.0, 500.0);
    let mut scroll = ScrollView::new(Box::new(content))
        .with_style(ScrollBarStyle {
            kind: ScrollBarKind::Floating,
            ..legacy()
        })
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    relayout(&mut scroll);
    paint(&mut scroll);
    let before = scroll.scroll_offset();

    size.set(Size::new(100.0, 800.0));
    relayout(&mut scroll);

    assert_eq!(scroll.scroll_offset(), before);
    assert_eq!(props(&scroll).vertical.fade_target, 0.0);
    paint(&mut scroll);
    assert!(!scroll.needs_draw());
}

#[test]
fn the_vertical_bar_stops_above_the_horizontal_bar() {
    let content = SizedBox::new().with_width(600.0).with_height(500.0);
    let mut scroll = ScrollView::new(Box::new(content))
        .horizontal(true)
        .with_style(legacy())
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    relayout(&mut scroll);
    let p = props(&scroll);
    let vertical = p.vertical;
    let hbar = p.horizontal.strip.height;
    assert!(p.horizontal.showing);
    assert_eq!(vertical.strip.y, hbar);
    assert_eq!(vertical.track.height, 100.0 - hbar);
    close(
        vertical.thumb.height,
        (100.0 - hbar) * (p.viewport.height / 500.0),
        1e-9,
    );

    // at either end the thumb meets the end of the track, not the view's bottom
    move_thumb(&mut scroll, -1000.0);
    close(props(&scroll).vertical.thumb.y, hbar, 1e-6);
    move_thumb(&mut scroll, 1000.0);
    let thumb = props(&scroll).vertical.thumb;
    close(thumb.y + thumb.height, 100.0, 1e-6);
}

#[test]
fn the_edge_fade_is_opt_in_and_follows_the_scroll() {
    let mut scroll = make(100.0, 500.0, legacy());
    let edges = |s: &ScrollView| {
        let e = props(s).faded_edges;
        [
            (e.top, "Top"),
            (e.bottom, "Bottom"),
            (e.left, "Left"),
            (e.right, "Right"),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, n)| *n)
        .collect::<Vec<_>>()
        .join(",")
    };
    assert_eq!(edges(&scroll), "");

    scroll = restyle(
        scroll,
        ScrollBarStyle {
            fade_strength: 0.5,
            ..legacy()
        },
    );
    assert_eq!(edges(&scroll), "Bottom");
    scroll.set_scroll_offset(50.0);
    relayout(&mut scroll);
    assert_eq!(edges(&scroll), "Top,Bottom");
    let max = scroll.max_scroll_value();
    scroll.set_scroll_offset(max);
    relayout(&mut scroll);
    assert_eq!(edges(&scroll), "Top");

    let cyan = crate::Color::rgb(0.0, 1.0, 1.0);
    let scroll = scroll.with_fade_color(cyan);
    assert_eq!(props(&scroll).fade_color, Some(cyan));
    assert_eq!(
        props(&ScrollView::new(Box::new(SizedBox::new()))).fade_color,
        None
    );
}

/// agg-sharp's classic `ThumbBand`: at rest the thumb is inset on both sides
/// of a full-width track; hovered it fills the bar.
#[test]
fn rust_only_a_classic_thumb_rests_inset_and_fills_the_bar_on_hover() {
    let _clock = clock::scoped_virtual(None);
    let mut scroll = make(100.0, 500.0, classic());
    paint(&mut scroll);
    let bar = props(&scroll).vertical;
    assert_eq!(bar.track.x - bar.strip.x, 0.0);
    assert_eq!(bar.track.width, 15.0);
    assert_eq!(bar.thumb.x - bar.strip.x, GROW_THUMB_BY);
    assert_eq!(bar.thumb.width, 15.0 - 2.0 * GROW_THUMB_BY);

    mouse_move(&mut scroll, bar.strip.x + 5.0, 50.0);
    paint(&mut scroll);
    advance(SCROLL_BAR_GROW_SECS);
    paint(&mut scroll);
    let bar = props(&scroll).vertical;
    assert_eq!(bar.thumb.x - bar.strip.x, 0.0);
    assert_eq!(bar.thumb.width, 15.0);
    assert_eq!(bar.track.width, 15.0);
}

/// `edge_guard: 0` lays a Solid bar out as agg-sharp's classic
/// `ScrollableWidget` does: each strip flush against the view's edge,
/// reserving exactly its strip, the two bars stopping at each other so the
/// corner square between them is neither's. The default style keeps its
/// resize-grip guard.
#[test]
fn rust_only_a_zero_edge_guard_solid_bar_sits_flush_and_leaves_the_corner_square() {
    let style = ScrollBarStyle {
        edge_guard: 0.0,
        ..ScrollBarStyle::solid()
    };
    let span = style.outer_margin + style.bar_width + style.inner_margin;
    let content = SizedBox::new().with_width(600.0).with_height(500.0);
    let mut scroll = ScrollView::new(Box::new(content))
        .horizontal(true)
        .with_style(style)
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    relayout(&mut scroll);
    let p = props(&scroll);
    let (v, h) = (p.vertical, p.horizontal);
    assert!(v.showing && h.showing);

    // reserved: exactly the strip, on both axes
    close(v.reserved, span, 1e-9);
    close(h.reserved, span, 1e-9);
    assert_eq!(
        p.viewport,
        crate::Rect::new(0.0, span, 200.0 - span, 100.0 - span)
    );

    // flush: the strips and tracks reach the view's right and bottom edges
    assert_eq!(v.strip.x + v.strip.width, 200.0);
    assert_eq!(v.track.x + v.track.width, 200.0);
    assert_eq!(h.strip.y, 0.0);
    assert_eq!(h.track.y, 0.0);

    // the corner: the vertical strip starts above the horizontal one and the
    // horizontal strip ends left of the vertical one, leaving the span square
    assert_eq!(v.strip.y, h.strip.y + h.strip.height);
    assert_eq!(v.strip.y, span);
    assert_eq!(h.strip.x + h.strip.width, v.strip.x);
    assert_eq!(v.strip.x, 200.0 - span);

    // the default keeps the resize-grip guard outside each strip
    let guard = crate::DEFAULT_SCROLL_EDGE_GUARD;
    assert_eq!(ScrollBarStyle::solid().edge_guard, guard);
    assert_eq!(ScrollBarStyle::default().edge_guard, guard);
    let scroll = restyle(scroll, ScrollBarStyle::solid());
    let p = props(&scroll);
    close(p.vertical.reserved, span + guard, 1e-9);
    assert_eq!(p.vertical.strip.x + p.vertical.strip.width, 200.0 - guard);
    assert_eq!(p.horizontal.strip.y, guard);
}
