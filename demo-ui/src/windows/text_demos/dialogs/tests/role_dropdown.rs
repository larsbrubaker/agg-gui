//! The Modals demo's User dialog Role dropdown at an effective scale: the
//! open list paints where the production `ComboBox` hit-tests it, above the
//! modal body, and a click on a painted row selects that row.
//!
//! A child of `dialogs/tests.rs` (split out to keep that file under the
//! 800-line limit), so it shares its `test_font` and `UxScaleGuard` helpers
//! and, through them, the real private `ModalOverlay` production code.

use super::*;

/// Paint `app` into a fresh `w`×`h` device framebuffer and return its RGBA
/// pixels (rows bottom-up: device px, Y-up, like the rest of agg-gui).
fn paint_app(app: &mut agg_gui::App, w: u32, h: u32) -> Vec<u8> {
    let mut fb = agg_gui::Framebuffer::new(w, h);
    {
        let mut ctx = agg_gui::GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    fb.pixels().to_vec()
}

/// The User modal's open Role dropdown must paint where it hit-tests, above
/// the modal body. `role_combo` is not one of `ModalOverlay`'s children (the
/// overlay paints it by hand inside `draw_modal`), so the `paint_global_overlays`
/// walk never reaches it, and a `ComboBox` submits its popup only from
/// `paint_global_overlay` — `ComboBox::paint` draws just the closed box. The
/// overlay must forward that call for the list to be drawn at all;
/// `paint_lifted_tree` drains combo popups again after the global-overlay
/// pass, so the forwarded popup lands above the dialog. Same App harness as
/// `modal_paints_where_it_hit_tests_at_effective_scale` (effective scale 2,
/// overlay under a non-zero local translate): the painted list must cover the
/// area the production combo hit-tests as its popup (root logical, × scale in
/// device px), hide the Cancel button it overlaps, and a click on its painted
/// second row must select that row.
#[test]
fn role_dropdown_paints_where_it_hit_tests_at_effective_scale() {
    let _guard = UxScaleGuard;
    agg_gui::ux_scale::set_ux_scale(2.0);
    let scale = agg_gui::ux_scale::effective_scale();
    assert!(
        (scale - 2.0).abs() < 1e-9,
        "test assumes device_scale 1.0 so effective scale is 2.0, got {scale}"
    );

    let font = test_font();
    let state = Rc::new(ModalState::default());
    state.user_open.set(true);

    let mut root = FlexColumn::new().with_padding(30.0);
    root.push(Box::new(SizedBox::new().with_height(40.0)), 0.0);
    root.push(
        Box::new(ModalOverlay::new(Arc::clone(&font), Rc::clone(&state))),
        0.0,
    );
    let mut app = agg_gui::App::new(Box::new(root));
    let (dev_w, dev_h) = (1280_u32, 960_u32);
    app.layout(Size::new(dev_w as f64, dev_h as f64));

    // Production geometry (root logical, Y-up) from a probe overlay, against
    // the viewport the App layout just published.
    let mut probe = ModalOverlay::new(Arc::clone(&font), Rc::clone(&state));
    let modal = probe.modal_rect(ModalLayer::User);
    let role = probe.role_rect(modal);
    let role_root = Rect::new(modal.x + role.x, modal.y + role.y, role.width, role.height);
    let cancel = probe
        .button_rects(ModalLayer::User)
        .into_iter()
        .find(|(label, _)| *label == "Cancel")
        .map(|(_, rect)| rect)
        .expect("User modal has a Cancel button");
    let cancel_root = Rect::new(
        modal.x + cancel.x,
        modal.y + cancel.y,
        cancel.width,
        cancel.height,
    );

    // Bottom edge of the area the production combo hit-tests as its open
    // list: lay the probe's combo out as `draw_modal` / `on_event` do, open
    // it with a press on its closed box, and walk down its centre line from
    // the box's bottom edge (combo-local y = 0) while the popup still hits.
    probe.prepare_user_controls(modal);
    probe.role_combo.on_event(&Event::MouseDown {
        pos: Point::new(role.width * 0.5, role.height * 0.5),
        button: MouseButton::Left,
        modifiers: Default::default(),
    });
    assert!(
        probe.role_combo.is_open(),
        "probe combo should open on a press on its closed box"
    );
    let popup_hit_bottom = (0..4000)
        .map(|i| -0.25 * i as f64)
        .take_while(|&y| {
            probe
                .role_combo
                .hit_test_global_overlay(Point::new(role.width * 0.5, y))
        })
        .last()
        .map(|y| role_root.y + y)
        .expect("the open combo should hit-test its list directly below the closed box");
    let expected = Rect::new(
        role_root.x,
        popup_hit_bottom,
        role_root.width,
        role_root.y - popup_hit_bottom,
    );

    // App input takes screen coords: Y-down device px.
    let click = |app: &mut agg_gui::App, sx: f64, sy: f64| {
        app.on_mouse_down(sx, sy, MouseButton::Left, Default::default());
        app.on_mouse_up(sx, sy, MouseButton::Left, Default::default());
    };
    let combo_sx = (role_root.x + role_root.width * 0.5) * scale;
    let combo_sy = dev_h as f64 - (role_root.y + role_root.height * 0.5) * scale;

    let closed = paint_app(&mut app, dev_w, dev_h);

    // Open the dropdown through the App. Escape then proves the click opened
    // it: with the dropdown open, Escape closes only the dropdown; with it
    // closed, Escape would close the modal. Reopen and paint.
    click(&mut app, combo_sx, combo_sy);
    app.on_key_down(Key::Escape, Default::default());
    assert!(
        state.user_open.get() && state.role.get() == 0,
        "clicking the Role combo's centre (screen {combo_sx}, {combo_sy}) must open \
         its dropdown, so Escape closes only that; user_open = {}, role = {}",
        state.user_open.get(),
        state.role.get()
    );
    click(&mut app, combo_sx, combo_sy);
    let open = paint_app(&mut app, dev_w, dev_h);

    let idx = |x: u32, y: u32| ((y * dev_w + x) * 4) as usize;
    let rgba = |px: &[u8], x: u32, y: u32| {
        let i = idx(x, y);
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    let differs = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).any(|(p, q)| p.abs_diff(q) > 2);

    // The list opens downward here, so its pixels are those the open paint
    // changed strictly below the closed box. The box's 1 px stroke reaches
    // 0.5 logical px below its bottom edge; rows from there up are left out.
    let below_box = ((role_root.y - 0.5) * scale).floor() as u32;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0_u32, 0_u32);
    for y in 0..below_box {
        for x in 0..dev_w {
            if differs(rgba(&closed, x, y), rgba(&open, x, y)) {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }

    // A: the open list was painted at all.
    assert!(
        min_x < max_x && min_y < max_y,
        "opening the Role dropdown painted nothing below its closed box \
         (combo root logical {role_root:?}, device rows y < {below_box}); \
         the popup hit-tests at {expected:?} (root logical) but is never drawn"
    );
    let painted_device = Rect::new(
        min_x as f64,
        min_y as f64,
        (max_x - min_x) as f64,
        (max_y - min_y) as f64,
    );
    let painted = Rect::new(
        painted_device.x / scale,
        painted_device.y / scale,
        painted_device.width / scale,
        painted_device.height / scale,
    );

    // B: the painted list covers the popup hit-test area (tolerance covers
    // the 1 px stroke's 0.5 logical px overhang and anti-aliased edges).
    let tol = 1.0;
    let close = |a: f64, b: f64| (a - b).abs() <= tol;
    assert!(
        close(painted.x, expected.x)
            && close(painted.y, expected.y)
            && close(painted.x + painted.width, expected.x + expected.width)
            && close(painted.y + painted.height, expected.y + expected.height),
        "Role dropdown must paint where it hit-tests at effective scale {scale}: \
         popup hit-test area (logical) = {expected:?}, \
         changed bbox (device px) = {painted_device:?}, \
         changed bbox / scale (logical) = {painted:?}"
    );

    // C: above the modal body. Both options are visible, so row 1 ("admin")
    // is the lower half of the painted list; with the combo low in the
    // dialog it straddles the Cancel button's bottom edge. Sample one pixel
    // over Cancel and one over plain dialog body below Cancel, both in row 1
    // at the same x: 4 px in from Cancel's left edge. `draw_button` paints the
    // Button at its natural width from the left of its rect, so only the left
    // part of the rect is button; this x is inside it, and clear of the
    // row's left-aligned text and Cancel's label (which sits above row 1).
    let row1_bottom = painted.y;
    let row1_top = painted.y + painted.height * 0.5;
    assert!(
        cancel_root.y > row1_bottom && cancel_root.y < row1_top,
        "test geometry: Cancel {cancel_root:?} should straddle row 1 \
         ({row1_bottom}..{row1_top}) of the painted list {painted:?}"
    );
    let sample_x = cancel_root.x + 4.0;
    let over_cancel_y = (cancel_root.y + row1_top.min(cancel_root.y + cancel_root.height)) * 0.5;
    let over_body_y = (row1_bottom + cancel_root.y) * 0.5;
    let to_dev = |v: f64| (v * scale).floor() as u32;
    let (sx, cy, by) = (to_dev(sample_x), to_dev(over_cancel_y), to_dev(over_body_y));
    let (closed_cancel, closed_body) = (rgba(&closed, sx, cy), rgba(&closed, sx, by));
    assert!(
        differs(closed_cancel, closed_body),
        "test geometry: with the list closed, the samples at logical \
         ({sample_x}, {over_cancel_y}) over Cancel and ({sample_x}, {over_body_y}) \
         over the dialog body should differ; got {closed_cancel:?} vs {closed_body:?}"
    );
    let (open_cancel, open_body) = (rgba(&open, sx, cy), rgba(&open, sx, by));
    assert!(
        !differs(open_cancel, open_body),
        "the open list's row 1 must paint above the modal body: the pixel over \
         Cancel at logical ({sample_x}, {over_cancel_y}) = {open_cancel:?} should \
         match the row-1 pixel over plain dialog body at ({sample_x}, {over_body_y}) \
         = {open_body:?}"
    );

    // D: paint and hit-test agree on the rows: clicking the painted centre of
    // row 1 picks "admin" (index 1) and leaves the modal open.
    let row1_cx = painted_device.x + painted_device.width * 0.5;
    let row1_cy = painted_device.y + painted_device.height * 0.25;
    let (row1_sx, row1_sy) = (row1_cx, dev_h as f64 - row1_cy);
    click(&mut app, row1_sx, row1_sy);
    assert!(
        state.role.get() == 1 && state.user_open.get(),
        "clicking the painted centre of the list's row 1 (device {row1_cx}, {row1_cy}; \
         screen {row1_sx}, {row1_sy}) must select role 1 and keep the modal open; \
         role = {}, user_open = {}",
        state.role.get(),
        state.user_open.get()
    );
}
