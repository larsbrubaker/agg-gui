//! App-pipeline tests for the colour-dialog click-away rework.
//!
//! Two behaviours, both driven through the real [`App`] pointer path:
//!
//! 1. **Free dragging** — a modal dialog nested in a small overlay slot can be
//!    dragged across the whole app viewport (its drag is no longer caged to the
//!    slot), and the global-overlay paint clamp keeps it on-screen.
//! 2. **Click-away** — pressing outside the dialog closes it: the change is
//!    committed (kept) when the user recoloured, or the dialog just closes when
//!    nothing changed; and the closing press never activates whatever sat
//!    underneath. Escape still cancels (covered by sibling suites).

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widget::active_modal_path;
use crate::widgets::rich_text::{
    single_font_resolver, Block, RichCommand, RichDoc, RichEditHandle, RichTextEdit, RichTextToolbar,
};
use crate::widgets::window::{ClickAwayAction, Window};
use crate::{ColorWheelPicker, Stack};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

fn unit_scale() {
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
}

const VP_W: f64 = 420.0;
const VP_H: f64 = 520.0;

/// World-space rect of the currently-open modal window, found by walking the
/// active-modal path and summing child-bounds offsets down to it. Panics if no
/// modal is open.
fn modal_window_world(app: &App) -> Rect {
    let path = active_modal_path(app.root()).expect("a modal dialog must be open");
    let mut widget: &dyn Widget = app.root();
    let (mut ox, mut oy) = (0.0, 0.0);
    for &idx in &path {
        let child: &Box<dyn Widget> = &widget.children()[idx];
        let b = child.bounds();
        ox += b.x;
        oy += b.y;
        widget = child.as_ref();
    }
    let b = widget.bounds();
    Rect::new(ox, oy, b.width, b.height)
}

fn modal_open(app: &App) -> bool {
    active_modal_path(app.root()).is_some()
}

/// Deliver a full press/release at a world-space (Y-up) point via the App.
fn click_world(app: &mut App, world: Point) {
    let screen_y = VP_H - world.y;
    app.on_mouse_down(world.x, screen_y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(world.x, screen_y, MouseButton::Left, Modifiers::default());
}

// ── Click-away through the real RichTextToolbar colour dialog ───────────────

/// A parent that lays its single child out in a small slot at an offset —
/// stands in for the thin toolbar strip / aligned overlay slot the real dialog
/// nests in, whose size used to cage the dialog's drag.
struct Slot {
    bounds: Rect,
    slot: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Slot {
    fn type_name(&self) -> &'static str {
        "Slot"
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
        let s = self.slot;
        let child = &mut self.children[0];
        // Hand the child the SMALL slot size as its available canvas — this is
        // what used to become `Window::canvas_size` and cage the drag.
        child.layout(Size::new(s.width, s.height));
        child.set_bounds(s);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn build_toolbar_app() -> (App, RichEditHandle) {
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let doc = RichDoc::from_blocks(vec![Block::plain("colour me")]);
    let editor = RichTextEdit::new(doc, single_font_resolver(Arc::clone(&font)));
    let handle = editor.handle();
    handle.select_all();

    let toolbar = RichTextToolbar::new(handle.clone(), Arc::clone(&font));
    let slot = Slot {
        bounds: Rect::default(),
        // A thin strip high in the viewport, exactly the caging shape.
        slot: Rect::new(0.0, 460.0, VP_W, 48.0),
        children: vec![Box::new(toolbar)],
    };
    let root = Stack::new().with_hit_children_only(false).add(Box::new(slot));
    (App::new(Box::new(root)), handle)
}

/// Open the toolbar's text-colour picker by dispatching a click to the swatch
/// button (row 1, index 5), then lay out so the Rebuilder builds the modal
/// dialog and begins the preview session.
fn open_text_color(app: &mut App) {
    {
        let slot = &mut app.root_mut().children_mut()[0];
        let toolbar = &mut slot.children_mut()[0];
        let col = &mut toolbar.children_mut()[0];
        let row1 = &mut col.children_mut()[0];
        let swatch = &mut row1.children_mut()[5];
        let b = swatch.bounds();
        let pos = Point::new(b.width * 0.5, b.height * 0.5);
        swatch.on_event(&Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        swatch.on_event(&Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }
    app.layout(Size::new(VP_W, VP_H));
}

#[test]
fn clickaway_commits_changed_color_and_closes() {
    unit_scale();
    let (mut app, handle) = build_toolbar_app();
    app.layout(Size::new(VP_W, VP_H));

    open_text_color(&mut app);
    assert!(modal_open(&app), "the picker dialog must be open");
    assert!(handle.is_previewing(), "opening begins a preview session");

    // Live-preview a new colour (what a wheel drag does).
    let blue = Color::rgb(0.0, 0.0, 1.0);
    handle.exec(&RichCommand::SetTextColor(blue));
    app.layout(Size::new(VP_W, VP_H));
    assert!(handle.is_preview_dirty(), "the preview mutated the document");

    // Press OUTSIDE the dialog (to its right). The modal grab routes it to the
    // window, which closes with ClickAway → the toolbar commits the change.
    let w = modal_window_world(&app);
    let outside = Point::new(w.x + w.width + 12.0, w.y + w.height * 0.5);
    click_world(&mut app, outside);
    app.layout(Size::new(VP_W, VP_H));

    assert!(!modal_open(&app), "click-away must close the dialog");
    assert!(!handle.is_previewing(), "click-away must end the preview session");
    assert_eq!(
        handle.common_style_of_selection().text_color,
        Some(Some(blue)),
        "a changed colour must be COMMITTED (kept) on click-away"
    );
}

#[test]
fn clickaway_without_change_closes_and_keeps_document() {
    unit_scale();
    let (mut app, handle) = build_toolbar_app();
    app.layout(Size::new(VP_W, VP_H));

    // Baseline colour before opening (the seed doc is plain — no explicit colour).
    let before = handle.common_style_of_selection().text_color;

    open_text_color(&mut app);
    assert!(modal_open(&app), "the picker dialog must be open");
    assert!(handle.is_previewing(), "opening begins a preview session");
    assert!(!handle.is_preview_dirty(), "nothing changed yet");

    // Press outside without ever recolouring.
    let w = modal_window_world(&app);
    let outside = Point::new(w.x + w.width + 12.0, w.y + w.height * 0.5);
    click_world(&mut app, outside);
    app.layout(Size::new(VP_W, VP_H));

    assert!(!modal_open(&app), "click-away must close the dialog");
    assert!(!handle.is_previewing(), "click-away must end the preview session");
    assert_eq!(
        handle.common_style_of_selection().text_color,
        before,
        "an untouched click-away must leave the document unchanged"
    );
}

#[test]
fn clickaway_right_press_also_dismisses() {
    // Any button pressed outside dismisses — not just left/middle. A right-press
    // must close the dialog rather than being swallowed with the dialog left up.
    unit_scale();
    let (mut app, handle) = build_toolbar_app();
    app.layout(Size::new(VP_W, VP_H));

    open_text_color(&mut app);
    assert!(modal_open(&app), "the picker dialog must be open");

    let w = modal_window_world(&app);
    let outside = Point::new(w.x + w.width + 12.0, w.y + w.height * 0.5);
    let screen_y = VP_H - outside.y;
    app.on_mouse_down(outside.x, screen_y, MouseButton::Right, Modifiers::default());
    app.on_mouse_up(outside.x, screen_y, MouseButton::Right, Modifiers::default());
    app.layout(Size::new(VP_W, VP_H));

    assert!(!modal_open(&app), "a right-press outside must close the dialog");
    assert!(!handle.is_previewing(), "right-press click-away ends the preview");
}

// ── Click-away must not activate the widget underneath ──────────────────────

/// Hosts a single `ComboBox` whose rect is driven by a shared cell, standing in
/// for a control sitting under the floating dialog.
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

fn combo_open(app: &App) -> bool {
    let combo = &app.root().children()[0].children()[0];
    combo
        .properties()
        .into_iter()
        .find(|(k, _)| *k == "open")
        .map(|(_, v)| v == "true")
        .unwrap_or(false)
}

#[test]
fn clickaway_does_not_activate_widget_underneath() {
    unit_scale();
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let combo_rect = Rc::new(Cell::new(Rect::new(0.0, 0.0, 100.0, 24.0)));
    let combo = ComboBox::new(vec!["Zero", "One", "Two"], 0, Arc::clone(&font));
    let underlay = ComboUnderlay {
        bounds: Rect::default(),
        children: vec![Box::new(combo)],
        combo_rect: Rc::clone(&combo_rect),
    };
    let picker = ColorWheelPicker::new(Color::rgb(0.2, 0.45, 0.88), Arc::clone(&font))
        .with_show_alpha(true)
        .with_font_size(12.0);
    // The with_on_close variant opts into click-away close.
    let dialog =
        crate::color_wheel_picker_dialog_with_on_close(picker, "Text colour", move |_reason| {});
    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(underlay))
        .add_aligned(dialog);
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(VP_W, VP_H));

    // Park the combo under a point that is OUTSIDE the dialog window, then relayout.
    let w = modal_window_world(&app);
    let over_combo = Point::new(w.x + w.width + 20.0, w.y + w.height * 0.5);
    combo_rect.set(Rect::new(over_combo.x - 50.0, over_combo.y - 12.0, 100.0, 24.0));
    app.layout(Size::new(VP_W, VP_H));

    assert!(!combo_open(&app), "combo starts closed");
    assert!(modal_open(&app), "dialog starts open");

    click_world(&mut app, over_combo);
    app.layout(Size::new(VP_W, VP_H));

    assert!(!modal_open(&app), "click-away must close the dialog");
    assert!(
        !combo_open(&app),
        "the click that closed the dialog must NOT open the combo underneath"
    );
}

// ── Free dragging across the viewport (unclamped by the slot) ───────────────

fn drag(app: &mut App, from: Point, to: Point) {
    app.on_mouse_down(from.x, VP_H - from.y, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(to.x, VP_H - to.y);
}

fn end_drag(app: &mut App, at: Point) {
    app.on_mouse_up(at.x, VP_H - at.y, MouseButton::Left, Modifiers::default());
}

fn paint_once(app: &mut App) {
    app.layout(Size::new(VP_W, VP_H));
    let mut fb = Framebuffer::new(VP_W as u32, VP_H as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
    app.paint(&mut ctx);
}

#[test]
fn modal_drag_escapes_slot_and_clamps_to_viewport() {
    unit_scale();
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let content = SizedBox::new().with_width(120.0).with_height(150.0);
    let win = Window::new("Dlg", Arc::clone(&font), Box::new(content))
        .with_bounds(Rect::new(20.0, 20.0, 120.0, 150.0))
        .with_auto_size(false)
        .with_resizable(false)
        .with_constrain(true)
        .with_gl_backbuffer(false)
        .with_modal(true)
        .with_click_away(ClickAwayAction::None);
    // Nest the window in a TINY slot — the old drag clamp caged it to this.
    let slot = Slot {
        bounds: Rect::default(),
        slot: Rect::new(20.0, 20.0, 60.0, 60.0),
        children: vec![Box::new(win)],
    };
    let root = Stack::new().with_hit_children_only(false).add(Box::new(slot));
    let mut app = App::new(Box::new(root));
    paint_once(&mut app);

    // The window's title bar is at its top (Y-up). Grab it and drag far beyond
    // the 60×60 slot, but still inside the 420×520 viewport.
    let w0 = modal_window_world(&app);
    let grab = Point::new(w0.x + w0.width * 0.5, w0.y + w0.height - 8.0);
    drag(&mut app, grab, Point::new(300.0, 340.0));

    let w1 = modal_window_world(&app);
    assert!(
        w1.x > 60.0 && w1.y > 60.0,
        "the drag must ESCAPE the 60px slot (got {w1:?}) — it is no longer caged"
    );
    // Still within the viewport, and unchanged in size.
    assert!(
        w1.x + w1.width <= VP_W + 0.5 && w1.y + w1.height <= VP_H + 0.5,
        "the dragged window must stay within the viewport (got {w1:?})"
    );
    end_drag(&mut app, Point::new(300.0, 340.0));

    // Now drag well beyond the viewport: the global-overlay paint clamp pulls
    // the window back so it stays fully on-screen.
    let w1 = modal_window_world(&app);
    let grab = Point::new(w1.x + w1.width * 0.5, w1.y + w1.height - 8.0);
    drag(&mut app, grab, Point::new(900.0, 900.0));
    end_drag(&mut app, Point::new(900.0, 900.0));
    paint_once(&mut app);

    let w2 = modal_window_world(&app);
    assert!(
        (w2.x + w2.width) <= VP_W + 0.5 && (w2.y + w2.height) <= VP_H + 0.5,
        "dragging past the viewport must clamp the window fully on-screen (got {w2:?})"
    );
}
