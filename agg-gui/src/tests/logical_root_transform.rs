//! `crate::widget::logical_root_transform` and the paint-time consumers that
//! place app-level overlays with it, at a UX scale other than 1.
//!
//! `App::paint` scales the ctx by the *effective* scale (device × UX, see
//! `crate::ux_scale::effective_scale`), so `DrawCtx::root_transform` lands in
//! root device pixels that carry both factors. Logical root space — the units
//! of layout, `current_viewport()` and the overlay request queues — is only
//! recovered by dividing out that full product. Dividing by the device scale
//! alone happens to work while `ux_scale == 1` (desktop) and breaks on mobile,
//! where the shell auto-sets ux ≈ 1.7.
//!
//! Every test here runs at device 2 × UX 1.5 (effective 3) and drives the
//! production path — `App` layout, pointer events and paint into a
//! physical-pixel framebuffer — then checks where the overlay actually landed:
//!
//! * the open `ComboBox` popup paints adjacent to the closed box,
//! * an interactive `Tooltip` decides flip / shift against the viewport from
//!   its real logical position,
//! * the `InspectorPanel` hover highlight lands over the hovered widget.
//!
//! The menu consumer (`PopupMenu::sync_root_origin`) is covered beside its
//! siblings in `widgets/menu/widget/tests_rows.rs`.

use super::*;

use crate::geometry::{Point, Rect};
use crate::layout_props::Insets;
use crate::text::Font;
use crate::widget::InspectorOverlay;
use crate::{DrawCtx, Event, EventResult, InspectorPanel, Tooltip};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const DEVICE: f64 = 2.0;
const UX: f64 = 1.5;
/// `DEVICE * UX` — what `App::paint` scales the ctx by.
const EFFECTIVE: f64 = 3.0;

/// Sets device × UX scale for one test and restores both to 1 on drop
/// (including on a failing assert), so the thread-local scales never leak
/// into a later test on a reused harness thread.
struct ScaleGuard;

impl ScaleGuard {
    fn set(device: f64, ux: f64) -> Self {
        crate::set_device_scale(device);
        crate::ux_scale::set_ux_scale(ux);
        ScaleGuard
    }
}

impl Drop for ScaleGuard {
    fn drop(&mut self) {
        crate::set_device_scale(1.0);
        crate::ux_scale::set_ux_scale(1.0);
    }
}

/// Root that places each child at a fixed logical rect.
struct Place {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    slots: Vec<Rect>,
}

impl Place {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            slots: Vec::new(),
        }
    }

    fn at(mut self, slot: Rect, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self.slots.push(slot);
        self
    }
}

impl Widget for Place {
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
        for (child, slot) in self.children.iter_mut().zip(&self.slots) {
            child.layout(Size::new(slot.width, slot.height));
            child.set_bounds(*slot);
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Fixed-size leaf that optionally fills itself with a solid colour.
struct Block {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    size: Option<Size>,
    fill: Option<Color>,
}

impl Block {
    /// Takes whatever size its parent offers and paints nothing.
    fn empty() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            size: None,
            fill: None,
        }
    }

    /// Always `size`, filled with `fill`.
    fn solid(size: Size, fill: Color) -> Self {
        Self {
            size: Some(size),
            fill: Some(fill),
            ..Self::empty()
        }
    }
}

impl Widget for Block {
    fn type_name(&self) -> &'static str {
        "Block"
    }
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
        let s = self.size.unwrap_or(available);
        self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
        s
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if let Some(c) = self.fill {
            ctx.set_fill_color(c);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Sample the physical pixel under logical point `(x, y)` (Y-up).
fn sample_logical(fb: &Framebuffer, x: f64, y: f64) -> [u8; 4] {
    sample(fb, (x * EFFECTIVE) as u32, (y * EFFECTIVE) as u32)
}

fn is_pure_green(p: [u8; 4]) -> bool {
    p[1] > 200 && p[0] < 60 && p[2] < 60
}

/// Logical-unit bounding box `(x0, y0, x1, y1)` of every pure-green pixel.
fn green_bbox_logical(fb: &Framebuffer) -> Option<(f64, f64, f64, f64)> {
    let (w, h) = (fb.width(), fb.height());
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for y in 0..h {
        for x in 0..w {
            if is_pure_green(sample(fb, x, y)) {
                bbox = Some(match bbox {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
    }
    bbox.map(|(x0, y0, x1, y1)| {
        (
            x0 as f64 / EFFECTIVE,
            y0 as f64 / EFFECTIVE,
            x1 as f64 / EFFECTIVE,
            y1 as f64 / EFFECTIVE,
        )
    })
}

/// The helper itself: under `App::paint`'s effective scale (device 2 × UX 1.5
/// = 3) plus a widget offset, a local point maps to its logical root position
/// (the device-only divisor would give 1.5× that), and the local origin maps
/// exactly. Called through the crate-root re-export.
#[test]
fn logical_root_transform_divides_out_device_times_ux_scale() {
    let _scales = ScaleGuard::set(DEVICE, UX);
    let mut fb = Framebuffer::new(4, 4);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.scale(EFFECTIVE, EFFECTIVE); // as `App::paint` does
    ctx.translate(30.0, 20.0); // the widget's offset in the root
    let t = crate::logical_root_transform(&ctx);

    let (mut ox, mut oy) = (0.0, 0.0);
    t.transform(&mut ox, &mut oy);
    assert_eq!((ox, oy), (30.0, 20.0), "local origin → logical root");

    let (mut x, mut y) = (5.0, 7.0);
    t.transform(&mut x, &mut y);
    assert!(
        (x - 35.0).abs() < 1e-9 && (y - 27.0).abs() < 1e-9,
        "local (5, 7) must map to logical (35, 27); got ({x}, {y})"
    );
}

/// At device 2 × UX 1.5 the open combo popup must paint adjacent to the
/// closed box. The request coords come from the paint ctx; dividing its root
/// transform by the device scale alone leaves them 1.5× too large, so the
/// popup floats up and right of the box while hit-testing (purely logical)
/// stays on it. Companion of `test_combo_popup_paints_at_logical_root_coords_under_hidpi`
/// (device 2 × UX 1, where the two divisors agree).
#[test]
fn combo_popup_paints_adjacent_to_the_box_at_ux_scale() {
    let _scales = ScaleGuard::set(DEVICE, UX);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven",
        ],
        0,
        font,
    );
    // Closed box at logical x ∈ [30, 180], y ∈ [20, 44].
    let root = Place::new().at(Rect::new(30.0, 20.0, 150.0, 24.0), Box::new(combo));
    let mut app = App::new(Box::new(root));

    // Physical 540 × 660 → logical 180 × 220.
    let phys = Size::new(180.0 * EFFECTIVE, 220.0 * EFFECTIVE);
    app.layout(phys);

    // Click logical (50, 30): physical x = 150, Y-down = 660 − 90 = 570.
    app.on_mouse_down(150.0, 570.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(150.0, 570.0, MouseButton::Left, Modifiers::default());
    assert_eq!(
        app.root().children()[0]
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "open")
            .map(|(_, v)| v),
        Some("true".to_string()),
        "the click inside the closed box must open the combo"
    );

    let mut fb = Framebuffer::new(phys.width as u32, phys.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
    app.paint(&mut ctx);

    // No room below (20 px), so the popup opens up: 7 rows of 22 starting at
    // the box top, logical x ∈ [30, 180], y ∈ [44, 198]. With the device-only
    // divisor it lands at x ∈ [45, 195], y ∈ [54, 208] instead, leaving both
    // samples (just inside its bottom-left, just above the box) red.
    for (x, y) in [(37.0, 50.0), (100.0, 47.0)] {
        let p = sample_logical(&fb, x, y);
        assert!(
            !is_red(p),
            "the open popup must cover logical ({x}, {y}) just above the closed \
             box at device {DEVICE} × UX {UX}; the pixel there is the red clear \
             colour {p:?}, so the popup painted somewhere else"
        );
    }
}

/// An interactive tip near the bottom of the viewport and right of centre:
/// from its true logical position it must flip ABOVE the anchor (no room
/// below) and needs NO left shift (it fits on the right). With the device-only
/// divisor the anchor reads 1.5× too far up and right, so the tip wrongly
/// stays below (half off-screen) and is shoved 45 px left.
#[test]
fn interactive_tooltip_avoids_viewport_edges_from_its_logical_position_at_ux_scale() {
    struct TooltipGuard;
    impl Drop for TooltipGuard {
        fn drop(&mut self) {
            crate::widgets::tooltip::reset_tooltip_test_state();
        }
    }
    let _scales = ScaleGuard::set(DEVICE, UX);
    crate::widgets::tooltip::reset_tooltip_test_state();
    let _tips = TooltipGuard;
    crate::clock::start_virtual();

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    // Content 100 × 40 → panel 116 × 52 (8 / 6 px padding).
    let content = Block::solid(Size::new(100.0, 40.0), Color::rgba(0.0, 1.0, 0.0, 1.0));
    let tooltip = Tooltip::new(Box::new(Block::empty()), "unused", font)
        .with_interactive_content(Box::new(content));
    // Anchor at logical x ∈ [150, 190], y ∈ [50, 70] in a 300 × 200 viewport.
    let root = Place::new().at(Rect::new(150.0, 50.0, 40.0, 20.0), Box::new(tooltip));
    let mut app = App::new(Box::new(root));
    let phys = Size::new(300.0 * EFFECTIVE, 200.0 * EFFECTIVE);
    app.layout(phys);

    // Hover the anchor centre, logical (170, 60): physical (510, 600 − 180).
    app.on_mouse_move(170.0 * EFFECTIVE, phys.height - 60.0 * EFFECTIVE);
    crate::clock::advance(
        crate::widgets::tooltip::tooltip_timings().initial_delay + Duration::from_millis(10),
    );

    let mut fb = Framebuffer::new(phys.width as u32, phys.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
    app.paint(&mut ctx);

    // Below would put the panel's bottom at 50 − 4 − 52 = −6 < the 4 px
    // margin → flip above: panel bottom = anchor top + 4 = 74, content at
    // y ∈ [80, 120]. The right edge 150 + 116 = 266 ≤ 300 − 4 → no shift:
    // content at x ∈ [158, 258].
    let bbox = green_bbox_logical(&fb).expect("the interactive tip never painted its content");
    let (x0, y0, x1, y1) = bbox;
    let near = |a: f64, b: f64| (a - b).abs() <= 1.0;
    assert!(
        near(y0, 80.0) && near(y1, 120.0),
        "the tip must flip above the anchor (content y ∈ [80, 120]); it painted \
         at y ∈ [{y0:.1}, {y1:.1}] (bbox {bbox:?})"
    );
    assert!(
        near(x0, 158.0) && near(x1, 258.0),
        "the tip fits on the right and must not shift (content x ∈ [158, 258]); \
         it painted at x ∈ [{x0:.1}, {x1:.1}] (bbox {bbox:?})"
    );
}

/// The inspector's hover highlight must land over the hovered widget. Its
/// `hovered_bounds` are the inspector nodes' `screen_bounds` — logical root
/// units, straight from layout — and the panel converts them into its own
/// paint space through the root transform. With the device-only divisor the
/// panel's origin reads 1.5× too far out, dragging the highlight off-screen.
#[test]
fn inspector_hover_highlight_lands_on_the_hovered_widget_at_ux_scale() {
    let _scales = ScaleGuard::set(DEVICE, UX);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered = Rc::new(RefCell::new(None));
    let panel = InspectorPanel::new(font, Rc::new(RefCell::new(Vec::new())), Rc::clone(&hovered));
    let target_rect = Rect::new(20.0, 30.0, 60.0, 40.0);
    let root = Place::new()
        .at(target_rect, Box::new(Block::empty()))
        .at(Rect::new(160.0, 10.0, 130.0, 180.0), Box::new(panel));
    let mut app = App::new(Box::new(root));
    let phys = Size::new(300.0 * EFFECTIVE, 200.0 * EFFECTIVE);
    app.layout(phys);

    // The highlight's input, as a host feeds it: the target's inspector node.
    let target = app
        .collect_inspector_nodes()
        .into_iter()
        .find(|n| n.type_name == "Block")
        .expect("the target is in the inspector snapshot");
    assert_eq!(
        target.screen_bounds, target_rect,
        "inspector screen_bounds are logical root units (the laid-out rect)"
    );
    // After layout: the panel's layout republishes its own (empty) hover.
    *hovered.borrow_mut() = Some(InspectorOverlay {
        bounds: target.screen_bounds,
        margin: Insets::ZERO,
        padding: Insets::ZERO,
    });

    // Black backdrop: the highlight is a 30 % blue wash, too faint to tell
    // from white but plain against black.
    let mut fb = Framebuffer::new(phys.width as u32, phys.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(0.0, 0.0, 0.0, 1.0));
    app.paint(&mut ctx);

    let centre = Point::new(50.0, 50.0);
    let on = sample_logical(&fb, centre.x, centre.y);
    assert!(
        on[2] > 40 && on[2] > on[0],
        "the blue hover highlight must cover the hovered widget's centre \
         (logical {centre:?}) at device {DEVICE} × UX {UX}; the pixel there is {on:?}"
    );
    let beside = sample_logical(&fb, 100.0, 50.0);
    assert!(
        is_dark(beside),
        "the highlight must not spill past the hovered widget (logical (100, 50)); got {beside:?}"
    );
}
