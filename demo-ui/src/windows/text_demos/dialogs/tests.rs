//! Unit tests for the Modals demo (`ModalOverlay`).
//!
//! Split out of `dialogs.rs` to keep that file under the 800-line limit.
//! Uses `super::*` so it exercises the real (private) `ModalOverlay`,
//! `ModalState`, and `modals_demo` production code, not copies.

use super::*;

fn test_font() -> Arc<Font> {
    const BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
}

#[test]
fn modal_escape_closes_only_top_layer() {
    let state = Rc::new(ModalState::default());
    state.user_open.set(true);
    state.save_open.set(true);
    let mut overlay = ModalOverlay::new(test_font(), Rc::clone(&state));
    overlay.layout(Size::new(360.0, 220.0));

    assert_eq!(
        overlay.on_event(&Event::KeyDown {
            key: Key::Escape,
            modifiers: Default::default(),
        }),
        EventResult::Consumed
    );

    assert!(state.user_open.get());
    assert!(!state.save_open.get());
}

#[test]
fn escape_with_role_combo_open_closes_combo_not_modal() {
    // Mirrors egui's `clicking_escape_when_popup_open_should_not_close_modal`:
    // with the role dropdown open, Escape must close the dropdown and leave
    // the surrounding modal open.
    let state = Rc::new(ModalState::default());
    state.user_open.set(true);
    let mut overlay = ModalOverlay::new(test_font(), Rc::clone(&state));
    agg_gui::widget::set_current_viewport(Size::new(360.0, 220.0));
    overlay.layout(Size::new(360.0, 220.0));
    let modal = overlay.modal_rect(ModalLayer::User);

    // Open the role dropdown by clicking on it.
    let role_rect = overlay.role_rect(modal);
    let combo_click = Point::new(modal.x + role_rect.x + 8.0, modal.y + role_rect.y + 12.0);
    agg_gui::widget::set_current_mouse_world(combo_click);
    overlay.on_event(&Event::MouseDown {
        pos: combo_click,
        button: MouseButton::Left,
        modifiers: Default::default(),
    });
    assert!(
        overlay.role_combo.is_open(),
        "role dropdown should be open after clicking it"
    );

    // Escape closes the dropdown, NOT the modal.
    assert_eq!(
        overlay.on_event(&Event::KeyDown {
            key: Key::Escape,
            modifiers: Default::default(),
        }),
        EventResult::Consumed
    );
    assert!(
        !overlay.role_combo.is_open(),
        "escape should close the open dropdown"
    );
    assert!(
        state.user_open.get(),
        "escape must not close the modal while the dropdown is open"
    );
}

#[test]
fn modal_save_button_opens_progress_layer() {
    let state = Rc::new(ModalState::default());
    state.save_open.set(true);
    let mut overlay = ModalOverlay::new(test_font(), Rc::clone(&state));
    agg_gui::widget::set_current_viewport(Size::new(360.0, 220.0));
    overlay.layout(Size::new(360.0, 220.0));
    let save = overlay.modal_rect(ModalLayer::Save);
    let yes = overlay.button_rects(ModalLayer::Save)[0].1;
    let click = Point::new(save.x + yes.x + 4.0, save.y + yes.y + 4.0);
    agg_gui::widget::set_current_mouse_world(click);

    overlay.on_event(&Event::MouseDown {
        pos: click,
        button: MouseButton::Left,
        modifiers: Default::default(),
    });

    assert_eq!(state.save_progress.get(), Some(0.0));
    assert_eq!(overlay.top_layer(), Some(ModalLayer::Progress));
}

#[test]
fn user_modal_edits_name_and_role_state() {
    let state = Rc::new(ModalState::default());
    state.user_open.set(true);
    let mut overlay = ModalOverlay::new(test_font(), Rc::clone(&state));
    agg_gui::widget::set_current_viewport(Size::new(360.0, 220.0));
    overlay.layout(Size::new(360.0, 220.0));
    let modal = overlay.modal_rect(ModalLayer::User);

    let name_rect = overlay.name_rect(modal);
    let name_click = Point::new(modal.x + name_rect.x + 8.0, modal.y + name_rect.y + 12.0);
    agg_gui::widget::set_current_mouse_world(name_click);
    overlay.on_event(&Event::MouseDown {
        pos: name_click,
        button: MouseButton::Left,
        modifiers: Default::default(),
    });
    for c in "Z".chars() {
        overlay.on_event(&Event::KeyDown {
            key: Key::Char(c),
            modifiers: Default::default(),
        });
    }
    assert!(state.name.borrow().contains('Z'));

    let role_rect = overlay.role_rect(modal);
    let combo_click = Point::new(modal.x + role_rect.x + 8.0, modal.y + role_rect.y + 12.0);
    agg_gui::widget::set_current_mouse_world(combo_click);
    overlay.on_event(&Event::MouseDown {
        pos: combo_click,
        button: MouseButton::Left,
        modifiers: Default::default(),
    });

    let admin_click = Point::new(modal.x + role_rect.x + 8.0, modal.y + role_rect.y - 33.0);
    agg_gui::widget::set_current_mouse_world(admin_click);
    overlay.on_event(&Event::MouseDown {
        pos: admin_click,
        button: MouseButton::Left,
        modifiers: Default::default(),
    });
    assert_eq!(state.role.get(), 1);
}

#[test]
fn modal_rect_centers_in_app_viewport_not_window_slot() {
    let state = Rc::new(ModalState::default());
    state.user_open.set(true);
    let mut overlay = ModalOverlay::new(test_font(), Rc::clone(&state));
    agg_gui::widget::set_current_viewport(Size::new(800.0, 600.0));
    overlay.layout(Size::new(300.0, 160.0));

    let rect = overlay.modal_rect(ModalLayer::User);
    assert!(
        (rect.x - 275.0).abs() < 1.0 && (rect.y - 229.0).abs() < 1.0,
        "modal should center in viewport, got {rect:?}"
    );
}

#[test]
fn active_modal_blocks_underlying_app_content() {
    let font = test_font();
    let clicked = Rc::new(Cell::new(false));
    let clicked_for_button = Rc::clone(&clicked);
    let state = Rc::new(ModalState::default());
    state.user_open.set(true);

    let root = agg_gui::Stack::new()
        .add(Box::new(
            Button::new("Under modal", Arc::clone(&font)).on_click(move || {
                clicked_for_button.set(true);
            }),
        ))
        .add(Box::new(ModalOverlay::new(font, Rc::clone(&state))));
    let mut app = agg_gui::App::new(Box::new(root));
    app.layout(Size::new(640.0, 480.0));

    // Click far from the modal body, over where regular content could be.
    // The modal backdrop should consume it and close the modal without
    // letting the underlying button see the press/release.
    app.on_mouse_down(20.0, 460.0, MouseButton::Left, Default::default());
    app.on_mouse_up(20.0, 460.0, MouseButton::Left, Default::default());

    assert!(
        !clicked.get(),
        "underlying content must not receive modal backdrop clicks"
    );
    assert!(
        !state.user_open.get(),
        "outside click should close the top modal"
    );
}

#[test]
fn modal_global_overlay_paints_after_normal_tree() {
    let state = Rc::new(ModalState::default());
    state.user_open.set(true);
    let mut overlay = ModalOverlay::new(test_font(), Rc::clone(&state));
    agg_gui::widget::set_current_viewport(Size::new(640.0, 480.0));
    overlay.layout(Size::new(200.0, 120.0));

    let mut fb = agg_gui::Framebuffer::new(640, 480);
    let mut ctx = agg_gui::GfxCtx::new(&mut fb);
    overlay.paint(&mut ctx);
    overlay.paint_global_overlay(&mut ctx);

    let alpha = fb.pixels()[(20 * 640 + 20) * 4 + 3];
    assert!(
        alpha > 0,
        "modal global overlay should paint backdrop alpha"
    );
}

/// Restores the thread-local UX scale to 1.0 when dropped, including during
/// unwinding from a failed assertion, so nothing that runs later on this
/// thread sees the test's 2× scale. (libtest normally runs each test on its
/// own thread, so this is hygiene rather than isolation between tests.)
struct UxScaleGuard;

impl Drop for UxScaleGuard {
    fn drop(&mut self) {
        agg_gui::ux_scale::set_ux_scale(1.0);
    }
}

/// At an effective scale (device × UX) other than 1, the modal must paint in
/// the same root-logical space it hit-tests in. `App::paint` scales the whole
/// tree (and the global-overlay pass) by the effective scale, and
/// `ModalOverlay::on_event` tests `current_mouse_world()` (root logical, the
/// App divides screen coords by the effective scale) against `modal_rect`.
/// So the dialog body must land at `modal_rect × scale` in device pixels, the
/// backdrop must cover the whole device framebuffer, and a click on the
/// dialog's painted centre must be seen by the overlay as inside the dialog
/// (not as an outside click that closes it).
#[test]
fn modal_paints_where_it_hit_tests_at_effective_scale() {
    let _guard = UxScaleGuard;
    agg_gui::ux_scale::set_ux_scale(2.0);
    let scale = agg_gui::ux_scale::effective_scale();
    assert!(
        (scale - 2.0).abs() < 1e-9,
        "test assumes device_scale 1.0 so effective scale is 2.0, got {scale}"
    );

    let font = test_font();
    let state = Rc::new(ModalState::default());
    state.save_open.set(true);

    // Only the modal paints anything opaque: the column has no background
    // and the backdrop is translucent over the transparent framebuffer, so
    // the only fully opaque pixels are the dialog body (fill, buttons, text).
    // The padding and spacer put the overlay's bounds away from the root
    // origin in both x and y, so the global-overlay pass paints it under a
    // non-zero local translate: a fix that only keeps the App's scale and
    // ignores that translate draws the dialog offset from where it hit-tests.
    let mut root = FlexColumn::new().with_padding(30.0);
    root.push(Box::new(SizedBox::new().with_height(40.0)), 0.0);
    root.push(
        Box::new(ModalOverlay::new(Arc::clone(&font), Rc::clone(&state))),
        0.0,
    );
    let mut app = agg_gui::App::new(Box::new(root));
    let (dev_w, dev_h) = (1280_u32, 960_u32);
    app.layout(Size::new(dev_w as f64, dev_h as f64));

    // Expected geometry straight from production `modal_rect` (root logical,
    // Y-up), against the viewport the App layout just published.
    let probe = ModalOverlay::new(Arc::clone(&font), Rc::clone(&state));
    let expected = probe.modal_rect(ModalLayer::Save);

    let mut fb = agg_gui::Framebuffer::new(dev_w, dev_h);
    {
        let mut ctx = agg_gui::GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    let px = fb.pixels();
    let alpha_at = |x: u32, y: u32| px[((y * dev_w + x) * 4 + 3) as usize];

    // Bounding box (device px, Y-up, half-open) of fully opaque pixels.
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0_u32, 0_u32);
    for y in 0..dev_h {
        for x in 0..dev_w {
            if alpha_at(x, y) >= 250 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }
    assert!(
        min_x < max_x && min_y < max_y,
        "no opaque dialog pixels painted at all"
    );
    let painted_device = Rect::new(
        min_x as f64,
        min_y as f64,
        (max_x - min_x) as f64,
        (max_y - min_y) as f64,
    );
    let painted_logical = Rect::new(
        painted_device.x / scale,
        painted_device.y / scale,
        painted_device.width / scale,
        painted_device.height / scale,
    );

    // A: the dialog paints at modal_rect × scale (tolerance covers the 1px
    // stroke, which overhangs the fill by at most 0.5 logical px, and
    // anti-aliased edges).
    let tol = 1.0;
    let close = |a: f64, b: f64| (a - b).abs() <= tol;
    assert!(
        close(painted_logical.x, expected.x)
            && close(painted_logical.y, expected.y)
            && close(
                painted_logical.x + painted_logical.width,
                expected.x + expected.width
            )
            && close(
                painted_logical.y + painted_logical.height,
                expected.y + expected.height
            ),
        "modal must paint where it hit-tests at effective scale {scale}: \
         hit-test modal_rect (logical) = {expected:?}, \
         opaque bbox (device px) = {painted_device:?}, \
         opaque bbox / scale (logical) = {painted_logical:?}"
    );

    // B: the backdrop covers the whole device framebuffer: both the
    // bottom-left and the top-right corner. Checking both catches a backdrop
    // that is unscaled (misses the top-right) and one that is scaled but
    // offset by the overlay's local translate (misses the bottom-left).
    for (x, y) in [(10, 10), (dev_w - 10, dev_h - 10)] {
        let corner_alpha = alpha_at(x, y);
        assert!(
            corner_alpha > 0,
            "backdrop must cover the full {dev_w}x{dev_h} device framebuffer; \
             alpha at ({x}, {y}) = {corner_alpha}"
        );
    }

    // C: clicking the dialog's painted centre (empty Save-modal body, not a
    // button) is inside the dialog as the overlay sees it, so it must not
    // close the modal. App screen coords are Y-down device px.
    let cx = painted_device.x + painted_device.width * 0.5;
    let cy = painted_device.y + painted_device.height * 0.5;
    let (sx, sy) = (cx, dev_h as f64 - cy);
    app.on_mouse_down(sx, sy, MouseButton::Left, Default::default());
    app.on_mouse_up(sx, sy, MouseButton::Left, Default::default());
    assert!(
        state.save_open.get() && state.save_progress.get().is_none(),
        "clicking the painted dialog centre (screen {sx}, {sy}) must not close \
         or advance the Save modal; save_open = {}, save_progress = {:?}",
        state.save_open.get(),
        state.save_progress.get()
    );
}
