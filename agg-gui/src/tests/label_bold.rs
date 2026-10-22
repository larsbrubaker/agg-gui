//! `Label::with_bold`: a bold label draws with the bold companion of the
//! system font (`font_settings::set_system_bold_font`), or the regular face
//! thickened when there is none, at the system text-size scale — and keeps
//! following live changes of the companion and the scale with no rebuild.
//! A label that is not bold draws exactly as before.
//!
//! Each test lays the label out and paints it into a [`PaintRecorder`],
//! then reads the face and size `paint` handed the context: what the text
//! is really drawn with.

use std::sync::Arc;

use super::paint_recorder::PaintRecorder;
use crate::font_settings::{set_font_size_scale, set_system_bold_font, set_system_font};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::Label;
use crate::{Rect, Size};

fn face(bytes: &'static [u8]) -> Arc<Font> {
    Arc::new(Font::from_slice(bytes).expect("font"))
}

fn own_face() -> Arc<Font> {
    face(include_bytes!(
        "../../../demo/assets/LiberationSans-Regular.ttf"
    ))
}

fn system_regular() -> Arc<Font> {
    face(include_bytes!("../../../demo/assets/Nunito_Regular.ttf"))
}

fn system_bold() -> Arc<Font> {
    face(include_bytes!("../../../demo/assets/Nunito_Bold.ttf"))
}

/// Start (and end) every test from the defaults: these settings are
/// per-thread, and a test runner may reuse the thread.
fn reset_typography() {
    set_system_font(None);
    set_system_bold_font(None);
    set_font_size_scale(1.0);
}

/// Lay `label` out and paint it; the face and size it drew with.
fn drawn_with(label: &mut Label) -> (Arc<Font>, f64) {
    let used = label.layout(Size::new(400.0, 100.0));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));
    let mut ctx = PaintRecorder::new();
    label.paint(&mut ctx);
    let font = Arc::clone(ctx.font().expect("the label set a font"));
    (font, ctx.font_size())
}

#[test]
fn bold_label_draws_with_the_system_bold_companion() {
    reset_typography();
    let (regular, bold) = (system_regular(), system_bold());
    set_system_font(Some(Arc::clone(&regular)));
    set_system_bold_font(Some(Arc::clone(&bold)));
    let mut label = Label::new("Heavy", own_face())
        .with_font_size(16.0)
        .with_bold(true);
    assert!(label.is_bold());
    let (font, size) = drawn_with(&mut label);
    assert!(Arc::ptr_eq(&font, &bold), "drew with the bold companion");
    assert_eq!(size, 16.0);
    reset_typography();
}

#[test]
fn bold_label_follows_a_new_companion_and_size_scale_without_a_rebuild() {
    reset_typography();
    set_system_font(Some(system_regular()));
    set_system_bold_font(Some(system_bold()));
    let mut label = Label::new("Heavy", own_face())
        .with_font_size(16.0)
        .with_bold(true);
    let (_, size) = drawn_with(&mut label);
    assert_eq!(size, 16.0);
    let narrow = label.layout(Size::new(400.0, 100.0)).width;

    // The app switches the system font family and the text size.
    let next_bold = face(include_bytes!("../../../demo/assets/Poppins.ttf"));
    set_system_font(Some(own_face()));
    set_system_bold_font(Some(Arc::clone(&next_bold)));
    set_font_size_scale(1.5);
    let (font, size) = drawn_with(&mut label);
    assert!(
        Arc::ptr_eq(&font, &next_bold),
        "drew with the new companion"
    );
    assert_eq!(size, 24.0, "the size scale applies to bold text");
    let wide = label.layout(Size::new(400.0, 100.0)).width;
    assert!(
        wide > narrow,
        "the layout re-measured at the new face and size ({narrow} -> {wide})"
    );
    reset_typography();
}

#[test]
fn regular_label_ignores_the_bold_companion() {
    reset_typography();
    let own = own_face();
    let regular = system_regular();
    set_system_bold_font(Some(system_bold()));

    // No system font: the label's own face, scaled.
    set_font_size_scale(1.25);
    let mut label = Label::new("Plain", Arc::clone(&own)).with_font_size(16.0);
    assert!(!label.is_bold());
    let (font, size) = drawn_with(&mut label);
    assert!(Arc::ptr_eq(&font, &own));
    assert_eq!(size, 20.0);

    // A system font: that face, never the bold companion.
    set_system_font(Some(Arc::clone(&regular)));
    let (font, _) = drawn_with(&mut label);
    assert!(Arc::ptr_eq(&font, &regular));
    assert!(!font.faux_bold());
    reset_typography();
}

#[test]
fn bold_label_without_a_companion_thickens_the_face_it_would_draw() {
    reset_typography();
    let own = own_face();
    let mut label = Label::new("Heavy", Arc::clone(&own))
        .with_font_size(16.0)
        .with_bold(true);

    // No system font: its own face, thickened.
    let (font, size) = drawn_with(&mut label);
    assert!(
        font.faux_bold(),
        "a bold label with no companion is thickened"
    );
    assert_eq!(font.shape_id(), own.shape_id(), "the label's own face");
    assert_eq!(size, 16.0);
    // The same variant every frame, so its caches keep hitting.
    let (again, _) = drawn_with(&mut label);
    assert!(Arc::ptr_eq(&font, &again));

    // A system font with no companion: the system font, thickened.
    let regular = system_regular();
    set_system_font(Some(Arc::clone(&regular)));
    set_font_size_scale(2.0);
    let (font, size) = drawn_with(&mut label);
    assert!(font.faux_bold());
    assert_eq!(
        font.shape_id(),
        regular.shape_id(),
        "follows the system font"
    );
    assert_eq!(size, 32.0);

    // Bold switched off at run time: the plain system font again.
    label.set_bold(false);
    let (font, _) = drawn_with(&mut label);
    assert!(Arc::ptr_eq(&font, &regular));
    reset_typography();
}
