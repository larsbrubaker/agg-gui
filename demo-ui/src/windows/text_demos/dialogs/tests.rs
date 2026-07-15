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
