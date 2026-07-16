//! Proves the **self-contained** `RichTextToolbar` colour picker floats over the
//! editor even from inside a thin, clipped host.
//!
//! The toolbar embeds its colour dialog as an internal overlay child rather than
//! a companion the app must place. That only works because the dialog is a modal
//! `Window`, which paints through the framework's clip-free global-overlay pass.
//! This test nests a toolbar under a parent that clips to a small rectangle,
//! opens the picker, and asserts the dialog paints OUTSIDE that clip — mirroring
//! `color_dialog_overlay::test_modal_dialog_paints_outside_ancestor_clip` but
//! for the whole toolbar widget, and checking the preview session is engaged.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::{Point, Rect};
use crate::widgets::rich_text::toolbar::RichTextToolbar;
use crate::widgets::rich_text::{single_font_resolver, Block, RichDoc, RichEditHandle, RichTextEdit};
use crate::Stack;
use std::sync::Arc;

/// A parent that clips its single child to a small local rectangle — stands in
/// for the scrolled / clipped container a real toolbar might be nested inside.
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
        child.layout(Size::new(cr.width, cr.height));
        child.set_bounds(cr);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn unit_scale() {
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
}

const VP_W: f64 = 420.0;
const VP_H: f64 = 520.0;

/// Build an `App` with a toolbar nested inside a `ClipParent` clipped to a
/// 120×120 rect. The toolbar strip sits high (y ≈ 380) so its own rows are
/// clipped away and the region we sample below the clip starts empty. Returns
/// the app and the shared handle so the test can assert preview state.
fn build_app() -> (App, RichEditHandle) {
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let doc = RichDoc::from_blocks(vec![Block::plain("colour me")]);
    let editor = RichTextEdit::new(doc, single_font_resolver(Arc::clone(&font)));
    let handle = editor.handle();
    handle.select_all();

    let toolbar = RichTextToolbar::new(handle.clone(), Arc::clone(&font));
    let clip = ClipParent {
        bounds: Rect::default(),
        clip: Rect::new(0.0, 0.0, 120.0, 120.0),
        child_rect: Rect::new(0.0, 380.0, VP_W, 60.0),
        children: vec![Box::new(toolbar)],
    };
    let root = Stack::new().with_hit_children_only(false).add(Box::new(clip));
    (App::new(Box::new(root)), handle)
}

/// Click the text-colour swatch by dispatching a real MouseDown+MouseUp to the
/// button widget (row 1, index 5: after B/I/U/S and the size combo). Direct
/// widget dispatch — the button self-hit-tests in local space, so it fires even
/// though the ancestor clip would hide it.
fn click_text_color_swatch(app: &mut App) {
    let clip = &mut app.root_mut().children_mut()[0];
    let toolbar = &mut clip.children_mut()[0];
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

/// Lay out + paint the app into a white-cleared framebuffer via the real
/// pipeline (`App::paint` runs the global-overlay pass).
fn paint_app(app: &mut App) -> Framebuffer {
    app.layout(Size::new(VP_W, VP_H));
    let mut fb = Framebuffer::new(VP_W as u32, VP_H as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        app.paint(&mut ctx);
    }
    fb
}

/// Count opaque, non-white pixels in a grid that lies OUTSIDE the 120×120 clip
/// (x > 120) but inside where the clamped dialog renders.
fn opaque_outside_clip(fb: &Framebuffer) -> usize {
    let mut count = 0;
    for &x in &[140u32, 180, 220, 260] {
        for &y in &[160u32, 240, 320, 400] {
            let px = sample(fb, x, y);
            if !is_white(px) && px[3] > 200 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn toolbar_color_dialog_paints_outside_ancestor_clip() {
    unit_scale();
    let (mut app, handle) = build_app();

    // Before opening: the sampled region below the clip is empty (the toolbar's
    // own rows are clipped away, and no dialog exists yet).
    let before = paint_app(&mut app);
    assert_eq!(
        opaque_outside_clip(&before),
        0,
        "nothing should paint outside the clip before the picker opens"
    );
    assert!(!handle.is_previewing(), "no preview session before opening");

    // Open the picker, then re-layout so the Rebuilder builds the modal dialog
    // (which starts the live-preview session) and paint again.
    click_text_color_swatch(&mut app);
    let after = paint_app(&mut app);

    assert!(
        opaque_outside_clip(&after) > 0,
        "the modal colour dialog must paint OUTSIDE the ancestor clip \
         (global-overlay pass) — the toolbar is not truly self-contained otherwise"
    );
    assert!(
        handle.is_previewing(),
        "opening the picker must begin the handle's live-preview session"
    );
}

/// Dismissing the dialog via its × / Escape route must unwind the preview
/// session (cancel_preview), leaving no dangling suspended-undo state.
#[test]
fn toolbar_color_dialog_escape_unwinds_preview() {
    unit_scale();
    let (mut app, handle) = build_app();

    click_text_color_swatch(&mut app);
    let _ = paint_app(&mut app);
    assert!(handle.is_previewing(), "preview active after opening");

    // Escape routes through the modal window's close path → the dialog's
    // on_close → cancel_preview.
    app.on_key_down(Key::Escape, Modifiers::default());
    let _ = paint_app(&mut app);

    assert!(
        !handle.is_previewing(),
        "Escape-closing the dialog must cancel the preview session"
    );
}
