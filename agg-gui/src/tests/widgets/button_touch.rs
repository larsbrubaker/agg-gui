use super::*;

/// A touchscreen tap is a bare press→release at one point with NO preceding
/// MouseMove (touch has no hover phase). The button must still fire — the
/// press is routed to it by hit-testing, so the pointer is on it.
#[test]
fn test_button_fires_on_hoverless_tap() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let clicked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clicked2 = Arc::clone(&clicked);

    let button = Button::new("Tap", font)
        .with_font_size(14.0)
        .on_click(move || clicked2.store(true, std::sync::atomic::Ordering::Relaxed));

    let mut app = App::new(Box::new(button));
    app.layout(Size::new(200.0, 100.0));

    // No on_mouse_move first — emulate a finger tap dead-centre.
    app.on_mouse_down(100.0, 50.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(100.0, 50.0, MouseButton::Left, Modifiers::default());

    assert!(
        clicked.load(std::sync::atomic::Ordering::Relaxed),
        "a hover-less tap inside the button must fire on_click"
    );
}

/// Pressing on the button but releasing outside it (a drag-off) must cancel
/// — the click only fires when the release lands within bounds.
#[test]
fn test_button_press_drag_off_does_not_fire() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let clicked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clicked2 = Arc::clone(&clicked);

    let button = Button::new("Tap", font)
        .with_font_size(14.0)
        .on_click(move || clicked2.store(true, std::sync::atomic::Ordering::Relaxed));

    let mut app = App::new(Box::new(button));
    app.layout(Size::new(200.0, 100.0));

    // Press inside, drag well below the viewport, release there.
    app.on_mouse_down(100.0, 50.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(100.0, 400.0);
    app.on_mouse_up(100.0, 400.0, MouseButton::Left, Modifiers::default());

    assert!(
        !clicked.load(std::sync::atomic::Ordering::Relaxed),
        "releasing outside the button after a drag-off must not fire on_click"
    );
}
