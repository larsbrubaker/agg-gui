//! `Window` widget regression tests — split out of the parent
//! `crate::tests::widgets` module to keep that file under the 800-line
//! limit.
//!
//! Covers the chrome-level behaviour of [`crate::widgets::window::Window`]:
//! hiding must stop the content subtree from painting, a collapsed window
//! must keep its rounded bottom corners, and the backbuffer spec must
//! describe the shadow outsets / fade-out alpha the layered renderers use.
//! Shares the parent module's test prelude via `use super::*`.

use super::*;

/// Closing a Window (visible = false) must prevent its content from being painted.
#[test]
fn test_window_close_hides_content() {
    use crate::text::Font;
    use crate::widget::paint_subtree;
    use crate::widgets::window::Window;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    // A window whose content is a Button — Button.paint() fills its bounds with
    // a visible background, so a leak is detectable as non-black pixels.
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win = Window::new("Test", Arc::clone(&font), Box::new(content))
        .with_bounds(crate::Rect::new(0.0, 0.0, 200.0, 200.0));

    // Run layout so child bounds are set.
    win.layout(Size::new(200.0, 200.0));

    // First paint with window visible — content area should have some pixel.
    let mut fb_visible = Framebuffer::new(200, 200);
    {
        let mut ctx = GfxCtx::new(&mut fb_visible);
        ctx.clear(Color::black());
        paint_subtree(&mut win, &mut ctx);
    }

    // Hide the window, paint again — should revert to all-black.
    win.hide();
    let mut fb_hidden = Framebuffer::new(200, 200);
    {
        let mut ctx = GfxCtx::new(&mut fb_hidden);
        ctx.clear(Color::black());
        paint_subtree(&mut win, &mut ctx);
    }

    // The visible framebuffer should have non-black pixels (window chrome).
    let visible_has_pixels = fb_visible
        .pixels()
        .chunks(4)
        .any(|p| p[0] > 50 || p[1] > 50 || p[2] > 50);
    assert!(visible_has_pixels, "visible window must paint something");

    // The hidden framebuffer must be completely black.
    let hidden_all_black = fb_hidden
        .pixels()
        .chunks(4)
        .all(|p| p[0] < 10 && p[1] < 10 && p[2] < 10);
    assert!(
        hidden_all_black,
        "hidden window must not paint anything; content child leaked"
    );
}

/// A collapsed Window is only its title bar, so the title-bar fill must not
/// square off the bottom corners of the outer rounded window shape.
#[test]
fn test_collapsed_window_title_bar_rounds_bottom_corners() {
    use crate::text::Font;
    use crate::widget::{paint_subtree, Widget};
    use crate::widgets::window::Window;
    use std::sync::Arc;

    fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * fb.width() + x) * 4) as usize;
        let p = &fb.pixels()[i..i + 4];
        [p[0], p[1], p[2], p[3]]
    }

    fn brightness(px: [u8; 4]) -> u16 {
        px[0] as u16 + px[1] as u16 + px[2] as u16
    }

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win = Window::new("Test", Arc::clone(&font), Box::new(content))
        .with_bounds(crate::Rect::new(0.0, 0.0, 200.0, 80.0));

    win.layout(Size::new(240.0, 120.0));
    win.on_event(&crate::Event::MouseDown {
        pos: crate::Point::new(12.0, 66.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    win.layout(Size::new(240.0, 120.0));

    let mut fb = Framebuffer::new(220, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(&mut win, &mut ctx);
    }

    let bottom_left_corner = sample(&fb, 1, 1);
    let title_bar_interior = sample(&fb, 100, 14);
    assert!(
        brightness(bottom_left_corner) + 40 < brightness(title_bar_interior),
        "collapsed title bar should leave the bottom-left corner rounded; corner={bottom_left_corner:?}, interior={title_bar_interior:?}"
    );
}

#[test]
fn test_window_backbuffer_spec_covers_shadow_and_fade_out() {
    use crate::text::Font;
    use crate::widget::{BackbufferKind, Widget};
    use crate::widgets::window::Window;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win = Window::new("Layered", Arc::clone(&font), Box::new(content))
        .with_bounds(crate::Rect::new(0.0, 0.0, 200.0, 120.0));

    let visible_spec = win.backbuffer_spec();
    assert_eq!(visible_spec.kind, BackbufferKind::GlFbo);
    assert!(visible_spec.cached);
    assert!(visible_spec.alpha > 0.99);
    assert!(visible_spec.outsets.left > 0.0);
    assert!(visible_spec.outsets.bottom > 0.0);
    assert!(visible_spec.outsets.right > 0.0);
    assert!(visible_spec.outsets.top > 0.0);

    win.hide();
    assert!(
        !win.is_visible(),
        "non-layer renderers should still see hide() as immediate"
    );
    let fading_layer = win.backbuffer_spec();
    assert!(
        fading_layer.alpha > 0.001 && fading_layer.alpha <= 1.0,
        "fade-out layer alpha should be visible and bounded, got {}",
        fading_layer.alpha
    );
}

#[test]
fn test_window_can_opt_out_of_gl_backbuffer() {
    use crate::text::Font;
    use crate::widget::{BackbufferKind, Widget};
    use crate::widgets::window::Window;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win =
        Window::new("Direct", Arc::clone(&font), Box::new(content)).with_gl_backbuffer(false);

    assert_eq!(win.backbuffer_spec().kind, BackbufferKind::None);
}
