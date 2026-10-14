//! Rust-only tests of `overlay.rs`, the port of C# `RenderMouse`: what the
//! label says, and what the drawn pointer looks like, on agg-gui's software
//! context (the live driver draws the same calls over its presented frames).

use agg_gui::{Framebuffer, GfxCtx, Modifiers};
use agg_gui_automation::overlay::{mouse_label, render_mouse, SimulatedMouse};
use agg_gui_automation::Keys;

fn pixel(fb: &Framebuffer, x: u32, y_up: u32) -> [u8; 4] {
    let i = ((y_up * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2], p[3]]
}

#[test]
fn rust_only_the_label_shows_held_shift_and_control_then_a_click_count_above_one() {
    let mut mouse = SimulatedMouse::default();
    assert_eq!(mouse_label(&mouse), "");
    mouse.click_count = 1;
    assert_eq!(mouse_label(&mouse), "", "a single click shows no count");
    mouse.modifiers = Modifiers {
        shift: true,
        ..Keys::CONTROL.to_agg_modifiers()
    };
    mouse.click_count = 2;
    assert_eq!(mouse_label(&mouse), "SC2");
}

#[test]
fn rust_only_the_pointer_is_filled_green_only_while_the_left_button_is_down() {
    // A 60 x 60 window with the pointer at (30, 20) in Y-down pixels, so
    // the circle's centre is 40 pixels up from the bottom.
    let draw = |left_button_down| {
        let mut fb = Framebuffer::new(60, 60);
        let mouse = SimulatedMouse {
            x: 30.0,
            y: 20.0,
            left_button_down,
            ..SimulatedMouse::default()
        };
        render_mouse(&mut GfxCtx::new(&mut fb), &mouse, 60.0, 1.0);
        fb
    };

    let up = draw(false);
    assert_eq!(
        pixel(&up, 30, 40)[3],
        0,
        "an up button leaves the centre clear"
    );
    let ring = pixel(&up, 40, 40);
    assert!(ring[3] > 0, "the ring is drawn at the radius: {ring:?}");

    let down = draw(true);
    let centre = pixel(&down, 30, 40);
    assert!(
        centre[1] > 100 && centre[0] < 30 && centre[2] < 30,
        "a held left button fills the circle green: {centre:?}"
    );
}
