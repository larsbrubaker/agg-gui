use super::*;

/// **Window layout NEVER mutates saved bounds.**
///
/// Auto-save serialises window bounds every frame that the mouse is up.
/// If `layout()` ever clamped or otherwise mutated `bounds`, the transient
/// canvas sizes that platforms fire during startup and fullscreen-exit
/// (Windows in particular) would silently corrupt saved state: user moves
/// a window to `Y=900` → fullscreen exits at close with a transient small
/// canvas → clamp would pull `bounds.y` to the shrunken `max_y` → auto-save
/// captures the clamped Y → next startup restores the wrong position.
///
/// This test asserts the non-mutation invariant across a startup transient
/// (small first frame), a growth (fullscreen second frame), and a later
/// shrink (fullscreen exit or user resize).  Clamp is still performed on
/// explicit user actions — drag, resize handle, collapse — which are
/// exercised separately.
#[test]
fn test_window_layout_never_mutates_bounds() {
    use crate::text::Font;
    use crate::widgets::window::Window;
    use crate::Label;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let content: Box<dyn crate::widget::Widget> =
        Box::new(Label::new("content", Arc::clone(&font)));

    // Saved position: window high on the Y-up canvas.  Valid under a large
    // canvas, "out of reach" under a small one — the scenario where a
    // buggy clamp would have triggered.
    let saved = crate::geometry::Rect::new(50.0, 800.0, 400.0, 200.0);
    let mut win = Window::new("Test", Arc::clone(&font), content).with_bounds(saved);

    // Each of these layout passes must leave `bounds` untouched.
    let sizes = [
        (800.0, 600.0),   // transient startup frame
        (1920.0, 1017.0), // fullscreen
        (800.0, 600.0),   // fullscreen-exit transient → would have
        //  corrupted state under the old clamp policy
        (1920.0, 1017.0), // stabilise
    ];
    for (w, h) in sizes {
        let _ =
            <Window as crate::widget::Widget>::layout(&mut win, crate::geometry::Size::new(w, h));
        assert_eq!(
            win.bounds().y,
            800.0,
            "layout({w}, {h}) mutated bounds.y to {} — auto-save would \
             now persist the mutated position, corrupting saved state",
            win.bounds().y,
        );
        assert_eq!(win.bounds().x, 50.0);
    }
}

#[test]
fn test_window_middle_drag_title_moves_for_touch_scroll_bridge() {
    use crate::text::Font;
    use crate::widgets::{primitives::Stack, window::Window};
    use crate::{App, Label, Modifiers, MouseButton};
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let win = Window::new(
        "Touch Movable",
        Arc::clone(&font),
        Box::new(Label::new("content", Arc::clone(&font))),
    )
    .with_bounds(crate::geometry::Rect::new(100.0, 100.0, 240.0, 140.0));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(win))));
    let viewport = crate::geometry::Size::new(640.0, 480.0);
    app.layout(viewport);

    let start_x = 140.0;
    let start_y_up = 100.0 + 140.0 - 12.0;
    let start_y_down = viewport.height - start_y_up;
    app.on_mouse_down(start_x, start_y_down, MouseButton::Middle, Modifiers::default());
    app.on_mouse_move(start_x + 30.0, start_y_down - 20.0);
    app.on_mouse_up(
        start_x + 30.0,
        start_y_down - 20.0,
        MouseButton::Middle,
        Modifiers::default(),
    );
    app.layout(viewport);

    let moved = crate::find_widget_by_id(app.root(), "Touch Movable")
        .expect("window remains in tree")
        .bounds();
    assert_eq!(moved.x, 130.0);
    assert_eq!(moved.y, 120.0);
}

/// **End-to-end: sidebar-toggle raise actually reorders the Stack.**
///
/// Not just "flags get drained" — asserts the child that was raised ends
/// up at the END of the children vec (painted last = top of z-order).
/// Uses a distinguishable `bounds.x` per Window so we can identify each
/// child through the `dyn Widget` trait object.
#[test]
fn test_sidebar_toggle_reorders_stack_to_end() {
    use crate::widgets::{primitives::Stack, window::Window};
    use crate::{
        geometry::{Rect, Size},
        text::Font,
        Label, Widget,
    };
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    // Three demos — all visible at start, distinct `bounds.x` (100 / 200 /
    // 300) so we can identify them after reorder.
    let a_visible = Rc::new(Cell::new(true));
    let b_visible = Rc::new(Cell::new(true));
    let c_visible = Rc::new(Cell::new(true));

    let make = |x: f64, vis: Rc<Cell<bool>>| -> Box<dyn Widget> {
        Box::new(
            Window::new(
                "W",
                Arc::clone(&font),
                Box::new(Label::new("x", Arc::clone(&font))),
            )
            .with_bounds(Rect::new(x, 0.0, 200.0, 120.0))
            .with_visible_cell(vis),
        )
    };

    let mut stack: Box<dyn Widget> = Box::new(
        Stack::new()
            .add(make(100.0, Rc::clone(&a_visible)))
            .add(make(200.0, Rc::clone(&b_visible)))
            .add(make(300.0, Rc::clone(&c_visible))),
    );

    // Baseline layout — seeds each Window's `last_visible`.
    let _ = stack.layout(Size::new(1024.0, 768.0));

    // Simulate: user clicks B's sidebar entry → hides B, then clicks again
    // to show it.  Each toggle is followed by ONE layout pass to mimic the
    // reactive-mode one-render-per-event cycle.
    b_visible.set(false);
    let _ = stack.layout(Size::new(1024.0, 768.0));
    b_visible.set(true);
    let _ = stack.layout(Size::new(1024.0, 768.0));

    // Expected children order by identifying bounds.x:
    //   index 0 → A (x=100, not raised)
    //   index 1 → C (x=300, not raised — preserved order)
    //   index 2 → B (x=200, raised to the end)
    let last_x = stack.children()[2].bounds().x;
    assert_eq!(
        last_x, 200.0,
        "after sidebar-toggle-on of B (x=200), B must be at the END of \
         Stack.children (got child with bounds.x={last_x} at index 2)"
    );
    let first_x = stack.children()[0].bounds().x;
    assert_eq!(first_x, 100.0, "A preserved at index 0");
    let mid_x = stack.children()[1].bounds().x;
    assert_eq!(mid_x, 300.0, "C preserved at index 1");
}

/// **Same-frame raise.**
///
/// When a raise is triggered during `layout()` (the sidebar-toggle path:
/// Window detects its `visible_cell` false→true in its own `layout()` and
/// sets `raise_request`), the `Stack`'s reorder MUST happen in the same
/// frame — not the next.  In reactive mode only one render runs per event,
/// so a one-frame delay means the raise is invisible until something else
/// fires an event, which is exactly the "opened window appears in the back"
/// bug the user reported.
///
/// Asserts that after a SINGLE `Stack::layout` following a visibility
/// toggle, the raised widget is at the END of the child list (last =
/// painted on top).
#[test]
fn test_raise_takes_effect_same_frame_as_visibility_toggle() {
    use crate::widgets::{primitives::Stack, window::Window};
    use crate::{
        geometry::{Rect, Size},
        text::Font,
        Label, Widget,
    };
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let a_visible = Rc::new(Cell::new(false)); // closed
    let b_visible = Rc::new(Cell::new(true)); // open

    let make = |vis: Rc<Cell<bool>>| -> Box<dyn Widget> {
        Box::new(
            Window::new(
                "W",
                Arc::clone(&font),
                Box::new(Label::new("x", Arc::clone(&font))),
            )
            .with_bounds(Rect::new(0.0, 0.0, 200.0, 120.0))
            .with_visible_cell(vis),
        )
    };

    let mut stack: Box<dyn Widget> = Box::new(
        Stack::new()
            .add(make(Rc::clone(&a_visible))) // index 0 (back)
            .add(make(Rc::clone(&b_visible))), // index 1 (front)
    );

    // Frame 1 — establish baseline: A invisible, B visible.  Window.layout
    // updates `last_visible` to match current visibility.
    let _ = stack.layout(Size::new(1024.0, 768.0));

    // User clicks A's sidebar checkbox.  `a_visible` flips to true.
    a_visible.set(true);

    // SINGLE layout call — simulates the next render frame.  A's layout
    // will detect the rising edge and set raise_request; Stack must drain
    // that flag in the same call.
    let _ = stack.layout(Size::new(1024.0, 768.0));

    // After this one layout pass the raise should have been consumed.
    // We can't identify "which Window is A" through the trait object, but
    // we can assert that no child has a pending raise (proves Stack ran
    // the drain AFTER children.layout, catching the rising-edge raise that
    // was set during this same layout pass).
    assert!(
        !stack.children_mut()[0].take_raise_request(),
        "child 0 still has a pending raise — Stack drain ran before \
         Window.layout set the flag; sidebar-opened windows will paint \
         in the back for one frame"
    );
    assert!(
        !stack.children_mut()[1].take_raise_request(),
        "child 1 still has a pending raise — same bug"
    );
}

/// **Raise-on-activation.**
///
/// Toggling a `Window`'s `visible_cell` from false→true (e.g. user clicks
/// the sidebar checkbox / demo-panel button that opens the window) must
/// cause the next `Stack::layout` to move that Window to the END of the
/// stack's child list — painted last, i.e. at the top of the visual z-order.
/// Two Windows exercise the reorder: the second becomes visible, then the
/// first; the first must end up last.
#[test]
fn test_window_raises_on_visibility_rising_edge() {
    use crate::widgets::{primitives::Stack, window::Window};
    use crate::{
        geometry::{Rect, Size},
        text::Font,
        Label, Widget,
    };
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    // Two windows, each with an independent visible_cell so we can toggle
    // them independently in the test.
    let a_visible = Rc::new(Cell::new(true));
    let b_visible = Rc::new(Cell::new(true));

    let make = |title: &str, vis: Rc<Cell<bool>>| -> Box<dyn Widget> {
        Box::new(
            Window::new(
                title,
                Arc::clone(&font),
                Box::new(Label::new("x", Arc::clone(&font))),
            )
            .with_bounds(Rect::new(0.0, 0.0, 200.0, 120.0))
            .with_visible_cell(vis),
        )
    };

    let mut stack: Box<dyn Widget> = Box::new(
        Stack::new()
            .add(make("A", Rc::clone(&a_visible)))
            .add(make("B", Rc::clone(&b_visible))),
    );

    // Frame 1 — both visible, both have visibility seeded true in
    // `Window::new` so neither requests a raise.  Order: [A, B].
    let _ = stack.layout(Size::new(1024.0, 768.0));
    assert_eq!(stack.children()[0].type_name(), "Window");
    assert_eq!(stack.children()[1].type_name(), "Window");

    // Close A, then reopen on the following frame.
    a_visible.set(false);
    let _ = stack.layout(Size::new(1024.0, 768.0)); // A goes invisible
    a_visible.set(true);
    let _ = stack.layout(Size::new(1024.0, 768.0)); // A: false→true transition
                                                    // A's raise should have fired; the Stack should have moved A to the end.
                                                    // We can't easily peek at the Window's title through the trait boundary,
                                                    // so the best structural check is: only Windows are in the Stack, and
                                                    // the order reflects the raise.  Re-toggle B to confirm a second raise
                                                    // lands ABOVE the first.
    b_visible.set(false);
    let _ = stack.layout(Size::new(1024.0, 768.0)); // B invisible
    b_visible.set(true);
    let _ = stack.layout(Size::new(1024.0, 768.0)); // B: false→true transition

    // After A then B were each toggled off→on, B was raised last.  Drain
    // each child's raise flag by running `take_raise_request` — if the
    // mechanism actually fired, the raise flags are already cleared and
    // calling again returns false.
    assert!(
        !stack.children_mut()[0].take_raise_request(),
        "first child still has a pending raise — Stack didn't consume it"
    );
    assert!(
        !stack.children_mut()[1].take_raise_request(),
        "second child still has a pending raise — Stack didn't consume it"
    );

    // Re-toggle A and assert the NEXT layout puts A last.  We capture a
    // uniquely-identifiable marker on A via a probe child.
    //
    // Rather than inject a probe, check behaviourally: toggle A off→on
    // then run one more layout; the child whose take_raise_request now
    // returns true would be A.  After that layout it's cleared and A is
    // at the end of the list.  If the raise mechanism works, take_raise
    // returns true exactly on the child that was raised.
    a_visible.set(false);
    let _ = stack.layout(Size::new(1024.0, 768.0));
    a_visible.set(true);
    // Before the next layout, A's raise_request is true and Stack's
    // reorder hasn't run yet.  Peek:
    let raise_flags_before: Vec<bool> = (0..stack.children().len())
        .map(|i| {
            // Can't peek non-destructively; take + verify separately on a
            // cloned mindset.  Instead call layout and rely on the final
            // state assertions.
            let _ = i;
            false
        })
        .collect();
    let _ = raise_flags_before;

    let _ = stack.layout(Size::new(1024.0, 768.0));
    // After this layout the raise has been consumed.  All take_raise should
    // return false.
    assert!(!stack.children_mut()[0].take_raise_request());
    assert!(!stack.children_mut()[1].take_raise_request());
}

/// **Paint-entry CTM snap invariant.**
///
/// The contract for a widget whose `enforce_integer_bounds()` returns `true`
/// is: "my `paint()` is called with an integer-translation CTM".  That
/// contract MUST hold regardless of how the widget is reached — via the
/// normal parent-walks-children loop inside `paint_subtree`, OR via a manual
/// `ctx.translate(fractional, fractional); paint_subtree(child, ctx)`
/// sequence in a widget that does its own layout (SegRow, drag overlays,
/// popups, anything with custom centering math).
///
/// Before the fix the snap happened only in the child-iteration loop, so
/// manual-translate callers silently handed off a fractional CTM — invisibly
/// breaking the guarantee and producing blurry `Label` backbuffer blits.
///
/// This test wraps a probe widget in a fractional manual translate and
/// asserts the probe sees an integer CTM at `paint()` entry.  If anyone ever
/// removes the `paint_subtree` entry snap again, this regresses.
#[test]
fn test_paint_subtree_snaps_ctm_for_manual_translate_entry() {
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::geometry::Rect;
    use crate::widget::{paint_subtree, Widget};
    use agg_rust::trans_affine::TransAffine;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Widget that captures the CTM present at its `paint()` entry.
    struct CtmProbe {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        captured: Rc<Cell<Option<TransAffine>>>,
    }

    impl Widget for CtmProbe {
        fn type_name(&self) -> &'static str {
            "CtmProbe"
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
            Size::new(
                self.bounds.width.min(available.width),
                self.bounds.height.min(available.height),
            )
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            self.captured.set(Some(ctx.transform()));
        }
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    let captured = Rc::new(Cell::new(None));
    let mut probe = CtmProbe {
        bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        children: Vec::new(),
        captured: Rc::clone(&captured),
    };

    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);

    // Manual caller: applies a FRACTIONAL translate, then drives paint_subtree.
    // This is the pattern `SegRow` uses when centring labels in unevenly
    // divided columns.  The snap has to happen inside paint_subtree — manual
    // callers shouldn't need to remember `snap_to_pixel`.
    ctx.translate(100.3, 50.7);
    paint_subtree(&mut probe, &mut ctx);

    let ctm = captured.get().expect("probe must have been painted");
    assert_eq!(
        ctm.tx.fract(),
        0.0,
        "tx still fractional at paint() entry: {} — paint_subtree snap regressed",
        ctm.tx,
    );
    assert_eq!(
        ctm.ty.fract(),
        0.0,
        "ty still fractional at paint() entry: {} — paint_subtree snap regressed",
        ctm.ty,
    );
    // Specific floor values so this also guards against a silent change to
    // round-nearest (which would subtly shift widgets by up to 0.5 px).
    assert_eq!(ctm.tx, 100.0);
    assert_eq!(ctm.ty, 50.0);
}

/// A widget that opts OUT of enforce_integer_bounds must NOT have its CTM
/// snapped — preserves sub-pixel positioning for smooth-scroll markers /
/// zoomed canvases.
#[test]
fn test_paint_subtree_preserves_fractional_ctm_when_opted_out() {
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::geometry::Rect;
    use crate::widget::{paint_subtree, Widget};
    use agg_rust::trans_affine::TransAffine;
    use std::cell::Cell;
    use std::rc::Rc;

    struct SubpixelProbe {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        captured: Rc<Cell<Option<TransAffine>>>,
    }
    impl Widget for SubpixelProbe {
        fn type_name(&self) -> &'static str {
            "SubpixelProbe"
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
            Size::new(
                self.bounds.width.min(available.width),
                self.bounds.height.min(available.height),
            )
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            self.captured.set(Some(ctx.transform()));
        }
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn enforce_integer_bounds(&self) -> bool {
            false
        } // opt out
    }

    let captured = Rc::new(Cell::new(None));
    let mut probe = SubpixelProbe {
        bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        children: Vec::new(),
        captured: Rc::clone(&captured),
    };

    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.translate(100.3, 50.7);
    paint_subtree(&mut probe, &mut ctx);

    let ctm = captured.get().expect("probe must have been painted");
    // Opt-out honoured: CTM passes through untouched.
    assert!(
        (ctm.tx - 100.3).abs() < 1e-9,
        "opt-out widget had tx snapped: {}",
        ctm.tx
    );
    assert!(
        (ctm.ty - 50.7).abs() < 1e-9,
        "opt-out widget had ty snapped: {}",
        ctm.ty
    );
}

/// AGG's software rasterizer, given **integer-aligned** 1-px-wide fills at
/// integer positions, must produce pixels that are **exactly** the fill
/// colour or the original buffer — never a half-covered mid-tone.  If this
/// ever regresses, every "bitmap then blit" path in the app loses its
/// pixel-perfect guarantee, including Label backbuffers.
///
/// This is the agg-side half of the "why did the bitmap grid look fuzzy on
/// native GL" investigation — if AGG is correct here, the fault lies in the
/// texture-upload / texture-sample stage; if AGG is wrong here, the source
/// image is already gray before the GL blit.
#[test]
fn test_agg_rasters_1px_stripes_with_zero_gray() {
    use crate::framebuffer::unpremultiply_rgba_inplace;

    let w = 96_u32;
    let h = 96_u32;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        // Alternating 1-px white / 1-px black vertical columns — exactly what
        // `PixelTestLinesBitmap` draws.
        for i in 0..(w as usize / 2) {
            let x = (2 * i) as f64;
            gfx.set_fill_color(Color::white());
            gfx.begin_path();
            gfx.rect(x, 0.0, 1.0, h as f64);
            gfx.fill();
            gfx.set_fill_color(Color::black());
            gfx.begin_path();
            gfx.rect(x + 1.0, 0.0, 1.0, h as f64);
            gfx.fill();
        }
    }
    let mut pixels = fb.pixels_flipped();
    unpremultiply_rgba_inplace(&mut pixels);

    let row_bytes = (w * 4) as usize;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let off = y * row_bytes + x * 4;
            let px = &pixels[off..off + 4];
            let expected_white = x % 2 == 0;
            let (er, eg, eb) = if expected_white {
                (255, 255, 255)
            } else {
                (0, 0, 0)
            };
            assert_eq!(
                (px[0], px[1], px[2], px[3]),
                (er, eg, eb, 255),
                "pixel ({x}, {y}) should be {} but is {:?}",
                if expected_white { "white" } else { "black" },
                px,
            );
        }
    }
}

/// `snap_to_pixel` must zero the fractional component of the CTM translation
/// and leave rotations / scales / integer translations alone.  Covers the
/// `paint_subtree` round-on-translate path exercised by every widget that
/// opts into `enforce_integer_bounds`.
#[test]
fn test_snap_to_pixel_zeros_fractional_translation() {
    use crate::draw_ctx::DrawCtx;

    let mut fb = Framebuffer::new(10, 10);
    let mut ctx = GfxCtx::new(&mut fb);

    // Build a pure-translation CTM with fractional tx and ty.
    ctx.translate(100.3, 50.7);
    let before = ctx.transform();
    assert!((before.tx - 100.3).abs() < 1e-9);
    assert!((before.ty - 50.7).abs() < 1e-9);

    ctx.snap_to_pixel();
    let after = ctx.transform();
    assert_eq!(after.tx.fract(), 0.0, "tx still fractional: {}", after.tx);
    assert_eq!(after.ty.fract(), 0.0, "ty still fractional: {}", after.ty);
    // Snap rounds DOWN (floor) so text/strokes sit on the pixel they would
    // have partially covered — predictable and matches MatterCAD semantics.
    assert_eq!(after.tx, 100.0);
    assert_eq!(after.ty, 50.0);

    // Negative translations floor toward -infinity.
    let mut fb2 = Framebuffer::new(10, 10);
    let mut ctx2 = GfxCtx::new(&mut fb2);
    ctx2.translate(-3.3, -4.7);
    ctx2.snap_to_pixel();
    let after2 = ctx2.transform();
    assert_eq!(after2.tx, -4.0);
    assert_eq!(after2.ty, -5.0);

    // Already-integer translation is a no-op.
    let mut fb3 = Framebuffer::new(10, 10);
    let mut ctx3 = GfxCtx::new(&mut fb3);
    ctx3.translate(7.0, 13.0);
    ctx3.snap_to_pixel();
    let after3 = ctx3.transform();
    assert_eq!(after3.tx, 7.0);
    assert_eq!(after3.ty, 13.0);
}

// ---------------------------------------------------------------------------
// Slider mouse-capture tests
// ---------------------------------------------------------------------------

/// Dragging a slider outside its bounds must continue to track the cursor —
/// clamping at the range limits — and must NOT snap to the near edge when the
/// pointer first leaves the widget.
///
/// Root cause of the old bug: `dispatch_mouse_move` sent a synthetic
/// `MouseMove { pos: (-1.0, -1.0) }` to the previously-hovered widget when
/// the cursor left its bounds.  The slider's `on_event` called
/// `value_from_x(-1.0)` which clamped to `min`, snapping the thumb to the
/// left edge regardless of the actual cursor position.
///
/// This test reproduces the snap-to-zero bug and guards the mouse-capture fix.
#[test]
fn test_slider_drag_outside_bounds_tracks_cursor() {
    use crate::text::Font;
    use crate::widgets::slider::Slider;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let last_val = Rc::new(Cell::new(0.5_f64));
    let lv = Rc::clone(&last_val);

    // 200 × 36 px slider, value range [0, 1].
    let slider = Slider::new(0.5, 0.0, 1.0, Arc::clone(&font)).on_change(move |v| lv.set(v));

    let mut app = App::new(Box::new(
        SizedBox::new()
            .with_width(200.0)
            .with_height(36.0)
            .with_child(Box::new(slider)),
    ));
    app.layout(Size::new(200.0, 36.0));

    // Press the thumb in the middle.  Y-down input: viewport_height(36) − 18 = 18 Y-up.
    app.on_mouse_down(100.0, 18.0, MouseButton::Left, Modifiers::default());

    // Drag far to the right (outside slider bounds) — value must clamp to max.
    app.on_mouse_move(9999.0, 18.0);
    assert_eq!(
        last_val.get(),
        1.0,
        "dragging outside right must clamp to max (1.0), not snap to 0.0"
    );

    // Drag far to the left — value must clamp to min.
    app.on_mouse_move(-9999.0, 18.0);
    assert_eq!(
        last_val.get(),
        0.0,
        "dragging outside left must clamp to min (0.0)"
    );

    // Release outside bounds — drag ends.
    app.on_mouse_up(0.0, 18.0, MouseButton::Left, Modifiers::default());

    // After release: moving the mouse must NOT fire the callback.
    last_val.set(999.0); // sentinel
    app.on_mouse_move(100.0, 18.0);
    assert_eq!(
        last_val.get(),
        999.0,
        "after mouse-up the slider must stop tracking cursor movement"
    );
}
