//! Regression tests for the floating `color_wheel_picker_dialog` swallowing
//! pointer events over its own bounds.
//!
//! The dialog wraps a `ColorWheelPicker` in a `Window` and is dropped into a
//! `Stack` as an aligned overlay (usually behind a `Rebuilder`, mirroring the
//! RichTextEdit toolbar + Widget Gallery colour rows).  Because the `Rebuilder`
//! and `Stack` place the overlay in a slot whose bounds do NOT contain the
//! window's own `(60, 60)` offset, ordinary `hit_test_subtree` routing let
//! clicks over the painted window fall through to whatever sat underneath — a
//! `ComboBox` in the real bug report.  These tests drive the real `App`
//! pointer path and assert the dialog claims its rect exclusively while open.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widgets::Rebuilder;
use crate::{ColorWheelPicker, Stack};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// Test-only layer that hosts a single `ComboBox` positioned at an explicit
/// rect (fed through a shared cell).  Stands in for the toolbar / gallery row
/// that sits *under* the floating colour dialog.
struct ComboUnderlay {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    combo_rect: Rc<Cell<Rect>>,
}

impl Widget for ComboUnderlay {
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
        let r = self.combo_rect.get();
        let child = &mut self.children[0];
        child.layout(Size::new(r.width, r.height));
        child.set_bounds(r);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the exact tree shape the demo uses: a Stack with a full-bleed
/// underlay (holding the combo) plus an aligned `Rebuilder` that produces the
/// floating colour dialog.
fn build_app(combo_rect: Rc<Cell<Rect>>, selected: Rc<Cell<usize>>) -> App {
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());

    let combo = ComboBox::new(vec!["Zero", "One", "Two", "Three"], 0, Arc::clone(&font))
        .with_selected_cell(selected);
    let underlay = ComboUnderlay {
        bounds: Rect::default(),
        children: vec![Box::new(combo)],
        combo_rect,
    };

    let build_font = Arc::clone(&font);
    let dialog = Rebuilder::new(
        || 1_u64,
        move || {
            let picker =
                ColorWheelPicker::new(Color::rgb(0.2, 0.45, 0.88), Arc::clone(&build_font))
                    .with_allow_none(true)
                    .with_show_alpha(true)
                    .with_font_size(12.0);
            crate::color_wheel_picker_dialog(picker, "Highlight colour")
        },
    );

    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(underlay))
        .add_aligned(Box::new(dialog));

    App::new(Box::new(root))
}

/// Sum the child-bounds offsets down `Stack -> Rebuilder -> Window` to get the
/// window's origin in root (world, Y-up) space, plus its size.
fn window_world_bounds(app: &App) -> Rect {
    let rebuilder = &app.root().children()[1];
    let window = &rebuilder.children()[0];
    let rb = rebuilder.bounds();
    let wb = window.bounds();
    Rect::new(rb.x + wb.x, rb.y + wb.y, wb.width, wb.height)
}

fn combo_open(app: &App) -> bool {
    let combo = &app.root().children()[0].children()[0];
    combo
        .properties()
        .into_iter()
        .find(|(k, _)| *k == "open")
        .map(|(_, v)| v == "true")
        .unwrap_or(false)
}

fn window_visible(app: &App) -> bool {
    app.root().children()[1].children()[0].is_visible()
}

const VP_W: f64 = 420.0;
const VP_H: f64 = 520.0;

/// Position `combo_rect` so that its 24px-tall button strip (local y 0..24)
/// sits centred on the given world (Y-up) point.
fn place_combo_under(combo_rect: &Rc<Cell<Rect>>, world: Point) {
    combo_rect.set(Rect::new(world.x - 60.0, world.y - 12.0, 120.0, 24.0));
}

/// Clicking the dialog's title-bar close button while it overlaps a ComboBox
/// must close the dialog and NOT leak the press/release to the combo beneath.
#[test]
fn test_color_dialog_close_click_does_not_leak_to_combo() {
    let combo_rect = Rc::new(Cell::new(Rect::new(0.0, 0.0, 120.0, 24.0)));
    let selected = Rc::new(Cell::new(0_usize));
    let mut app = build_app(Rc::clone(&combo_rect), Rc::clone(&selected));
    app.layout(Size::new(VP_W, VP_H));

    // Close button center: window-local (w - 10, h - 14) in Y-up.
    let wb = window_world_bounds(&app);
    let close_world = Point::new(wb.x + wb.width - 10.0, wb.y + wb.height - 14.0);
    place_combo_under(&combo_rect, close_world);
    app.layout(Size::new(VP_W, VP_H));

    assert!(window_visible(&app), "dialog should start open");
    assert!(!combo_open(&app), "combo should start closed");

    let screen_y = VP_H - close_world.y;
    app.on_mouse_down(
        close_world.x,
        screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        close_world.x,
        screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );

    assert!(
        !combo_open(&app),
        "clicking the dialog's close button must not open the ComboBox underneath"
    );
    assert!(
        !window_visible(&app),
        "clicking the close button must close the dialog"
    );
}

/// Clicking the dialog BODY (over the combo, in the region that sticks out of
/// the Rebuilder's slot) must be swallowed — the combo must stay closed.
#[test]
fn test_color_dialog_body_click_does_not_leak_to_combo() {
    let combo_rect = Rc::new(Cell::new(Rect::new(0.0, 0.0, 120.0, 24.0)));
    let selected = Rc::new(Cell::new(0_usize));
    let mut app = build_app(Rc::clone(&combo_rect), Rc::clone(&selected));
    app.layout(Size::new(VP_W, VP_H));

    // A point near the top-right of the window body, below the title bar.
    // TITLE_H is 28; stay a few px below it and near the right edge so the
    // point lands in the Rebuilder's out-of-slot overhang.
    let wb = window_world_bounds(&app);
    let body_world = Point::new(wb.x + wb.width - 6.0, wb.y + wb.height - 34.0);
    place_combo_under(&combo_rect, body_world);
    app.layout(Size::new(VP_W, VP_H));

    assert!(!combo_open(&app), "combo should start closed");

    let screen_y = VP_H - body_world.y;
    app.on_mouse_down(
        body_world.x,
        screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        body_world.x,
        screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );

    assert!(
        !combo_open(&app),
        "clicking the dialog body must not open the ComboBox underneath"
    );
    assert!(
        window_visible(&app),
        "clicking the dialog body must not close the dialog"
    );
}

// ── Bug 1: the modal dialog must PAINT outside an ancestor's clip ───────────
//
// The dialog's *input* already escapes ancestor bounds via the modal path
// (tests above). Its *painting* must too: a modal `Window` renders through the
// clip-free global-overlay pass, so a host that clips its content region (a
// ScrollView, the RichTextEdit demo's content area) can't truncate the dialog.

use crate::widgets::window::Window;

/// A parent that clips its single child to a small local rectangle — stands in
/// for the scrolled / clipped container the real dialog is nested inside.
struct ClipParent {
    bounds: Rect,
    clip: Rect,
    child_rect: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for ClipParent {
    fn type_name(&self) -> &'static str {
        "ClipParent"
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
    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        Some((self.clip.x, self.clip.y, self.clip.width, self.clip.height))
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        let cr = self.child_rect;
        let child = &mut self.children[0];
        // Give the child room to size itself; `Window` keeps its own bounds
        // (its `set_bounds` is a no-op once non-zero), `FillBox` adopts `cr`.
        child.layout(available);
        child.set_bounds(cr);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn crate::draw_ctx::DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A solid-colour box painting its full bounds — a *non-deferred* child used to
/// prove `ClipParent` genuinely clips ordinary content.
struct FillBox {
    bounds: Rect,
    color: Color,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for FillBox {
    fn type_name(&self) -> &'static str {
        "FillBox"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn crate::draw_ctx::DrawCtx) {
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn unit_scale() {
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
}

/// Lay out + paint an app into a white-cleared framebuffer via the real
/// pipeline (`App::paint` runs the global-overlay pass).
fn paint_app(app: &mut App, vp: Size) -> Framebuffer {
    app.layout(vp);
    let mut fb = Framebuffer::new(vp.width as u32, vp.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        app.paint(&mut ctx);
    }
    fb
}

/// Control: an ordinary (non-modal) child IS clipped by `ClipParent`, so a
/// point outside the 100×100 clip stays the clear colour. This pins that the
/// clip is real — otherwise the modal test below would prove nothing.
#[test]
fn test_clip_parent_actually_clips_plain_child() {
    unit_scale();
    let fill = FillBox {
        bounds: Rect::default(),
        color: Color::rgb(0.9, 0.1, 0.1),
        children: Vec::new(),
    };
    let clip = ClipParent {
        bounds: Rect::default(),
        clip: Rect::new(0.0, 0.0, 100.0, 100.0),
        child_rect: Rect::new(60.0, 60.0, 220.0, 160.0),
        children: vec![Box::new(fill)],
    };
    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(clip));
    let mut app = App::new(Box::new(root));
    let fb = paint_app(&mut app, Size::new(420.0, 520.0));

    // Inside the clip and inside the fill → painted red.
    assert!(
        super::is_red(super::sample(&fb, 90, 90)),
        "plain child inside the clip must paint; got {:?}",
        super::sample(&fb, 90, 90)
    );
    // Outside the clip (x=200) but inside the child's rect → clipped away.
    assert!(
        super::is_white(super::sample(&fb, 200, 120)),
        "plain child must be CLIPPED outside the 100x100 clip rect; got {:?}",
        super::sample(&fb, 200, 120)
    );
}

/// The fix: a modal `color_wheel_picker_dialog` hosted under the same clipping
/// parent paints its full extent — a point OUTSIDE the parent's clip but inside
/// the dialog body renders the dialog's fill instead of the clear colour.
#[test]
fn test_modal_dialog_paints_outside_ancestor_clip() {
    unit_scale();
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let picker = ColorWheelPicker::new(Color::rgb(0.2, 0.45, 0.88), Arc::clone(&font))
        .with_show_alpha(true)
        .with_font_size(12.0);
    let dialog = crate::color_wheel_picker_dialog(picker, "Text colour");
    let clip = ClipParent {
        bounds: Rect::default(),
        clip: Rect::new(0.0, 0.0, 100.0, 100.0),
        child_rect: Rect::new(60.0, 60.0, 220.0, 300.0),
        children: vec![dialog],
    };
    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(clip));
    let mut app = App::new(Box::new(root));
    let fb = paint_app(&mut app, Size::new(420.0, 520.0));

    // (200, 120) is outside the 100×100 clip yet well inside the dialog body
    // (window rect ≈ x∈[60,280], y∈[60,446]). With the overlay-paint fix this
    // must be the dialog's opaque fill, not the clear white.
    let px = super::sample(&fb, 200, 120);
    assert!(
        !super::is_white(px) && px[3] > 200,
        "modal dialog must paint OUTSIDE the ancestor clip (overlay pass); \
         got {px:?} at (200,120)"
    );
}

// ── on_close forwarding: × button and Escape ───────────────────────────────
//
// The window chrome's close affordance (× button) and Escape are a route
// SEPARATE from the picker's own Cancel button. `color_wheel_picker_dialog`
// alone never wires the window's `on_close`, so a host relying on it (the
// RichTextEdit demo cancels a live preview there) would be stranded. These pin
// that `color_wheel_picker_dialog_with_on_close` forwards both routes.

fn build_closeable_dialog_app(closed: Rc<Cell<bool>>) -> App {
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let picker = ColorWheelPicker::new(Color::rgb(0.2, 0.45, 0.88), Arc::clone(&font))
        .with_show_alpha(true)
        .with_font_size(12.0);
    let dialog = crate::color_wheel_picker_dialog_with_on_close(picker, "Text colour", move |_| {
        closed.set(true)
    });
    let root = Stack::new().with_hit_children_only(false).add(dialog);
    App::new(Box::new(root))
}

#[test]
fn test_dialog_close_button_fires_on_close() {
    unit_scale();
    let closed = Rc::new(Cell::new(false));
    let mut app = build_closeable_dialog_app(Rc::clone(&closed));
    app.layout(Size::new(VP_W, VP_H));

    // Window is a direct Stack child, so its bounds are already root-space.
    let wb = app.root().children()[0].bounds();
    let close_world = Point::new(wb.x + wb.width - 10.0, wb.y + wb.height - 14.0);
    let screen_y = VP_H - close_world.y;
    app.on_mouse_down(
        close_world.x,
        screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        close_world.x,
        screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );

    assert!(
        closed.get(),
        "clicking the dialog's × button must fire the forwarded on_close"
    );
}

#[test]
fn test_dialog_escape_fires_on_close() {
    unit_scale();
    let closed = Rc::new(Cell::new(false));
    let mut app = build_closeable_dialog_app(Rc::clone(&closed));
    app.layout(Size::new(VP_W, VP_H));

    // Escape routes through the modal path to the window and closes it.
    app.on_key_down(Key::Escape, Modifiers::default());

    assert!(
        closed.get(),
        "Escape on the open modal dialog must fire the forwarded on_close"
    );
}

/// The dialog must also be clamped into the live viewport so it can't open
/// partially off-screen. A modal window positioned so its right edge spills
/// past the viewport is pulled back in during the overlay paint.
#[test]
fn test_modal_dialog_clamped_into_viewport() {
    unit_scale();
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let content = FillBox {
        bounds: Rect::default(),
        color: Color::rgb(0.2, 0.6, 0.9),
        children: Vec::new(),
    };
    // Right edge at 250 + 200 = 450, past the 400-wide viewport by 50px.
    let win = Window::new("Dlg", Arc::clone(&font), Box::new(content))
        .with_bounds(Rect::new(250.0, 60.0, 200.0, 150.0))
        .with_auto_size(false)
        .with_resizable(false)
        .with_constrain(false)
        .with_modal(true);
    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(win));
    let mut app = App::new(Box::new(root));
    let _ = paint_app(&mut app, Size::new(400.0, 400.0));

    let bx = app.root().children()[0].bounds().x;
    assert!(
        (bx - 200.0).abs() < 1.5,
        "modal window must be clamped so it fits the 400px viewport \
         (expected bounds.x ≈ 200, the window is 200 wide); got {bx}"
    );
}
