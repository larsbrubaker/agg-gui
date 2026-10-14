//! A popup's row labels follow every font swap.
//!
//! `PopupLabels` rebuilds its cached row `Label`s when the font it paints
//! with changes, recognising the font by [`Font::id`] rather than its
//! address, which a freed font can hand to the next one. Painting reads the
//! process-global input profile, so the test holds
//! `input_profile::profile_test_lock()` and pins `InputProfile::Desktop`,
//! as `tests_fit.rs` does.

use std::sync::Arc;

use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Size};
use crate::gfx_ctx::GfxCtx;
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;
use crate::Color;

use super::super::model::MenuItem;
use super::PopupMenu;

const CASCADIA: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
const NOTO: &[u8] = include_bytes!("../../../../assets/fonts/NotoSans-Regular.ttf");

/// Width of `text` as a fresh popup-row `Label` in `font` lays it out.
fn label_width(font: &Arc<Font>, text: &str, size: f64) -> f64 {
    let mut label = Label::new(text, Arc::clone(font)).with_font_size(size);
    label.layout(Size::new(10_000.0, 100.0)).width
}

/// Paint `popup` once with `font` into a scratch framebuffer.
fn paint_with(popup: &mut PopupMenu, font: &Arc<Font>) {
    let mut fb = Framebuffer::new(400, 300);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());
    popup.paint(&mut ctx, Arc::clone(font), 14.0, Size::new(400.0, 300.0));
}

/// Painting an open popup with a new font each time, every font freed
/// before the next is made, leaves its row label measured in the font just
/// painted with.
#[test]
fn popup_font_swap_remeasures_row_labels() {
    let _guard = profile_test_lock();
    set_input_profile(InputProfile::Desktop);
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
    let mut popup = PopupMenu::new(vec![MenuItem::action("Width 15.00 mm", "w").into()]);
    popup.open_at(Point::new(20.0, 250.0));
    let mut widths = Vec::new();
    for i in 0..32 {
        let bytes = if i % 2 == 0 { CASCADIA } else { NOTO };
        let font = Arc::new(Font::from_slice(bytes).expect("font"));
        paint_with(&mut popup, &font);
        let label = popup.labels.row_label(0, 0).expect("row label");
        let width = label.bounds().width;
        assert_eq!(
            width,
            label_width(&font, "Width 15.00 mm", 14.0),
            "swap {i}"
        );
        widths.push(width);
        // `font` is released here; the popup's labels let go of it when the
        // next paint rebuilds them.
    }
    assert_ne!(
        widths[0], widths[1],
        "the two faces must measure differently"
    );
}
