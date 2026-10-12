//! agg-sharp `SliderAndComboRestyleTests`
//! (Tests/Agg.Tests/Agg.UI/SliderAndComboRestyleTests.cs): the slider's
//! accent fill and round thumb, the drop-down's chevron and rounded field,
//! and that neither look changed a size layouts are built against.
//!
//! - `SliderBoundsAreWhatTheyWereBeforeTheRestyle` and
//!   `DropDownSizeIsWhatItWasBeforeTheRestyle` pin agg-gui's own sizes (C#
//!   pins agg-sharp's): a horizontal `Slider` is as wide as it is offered and
//!   22 tall, a vertical one 140 long; a fit-width `ComboBox` is its widest
//!   item plus padding and arrow, 24 tall, inside a 1 px outline band.
//! - The thumb-ring sample sits mid-ring: C#'s `center.X - 4` is mid-ring on
//!   its 5 px thumb; agg-gui's 7 px thumb keeps the same 4.5/7 inner radius,
//!   so its ring middle is `r * (1 - 2.5 / 14)` from the centre.
//! - C#'s outline check samples the row just outside the field; agg-gui's
//!   outline band is the field's own edge row (a ring inside the box, as
//!   `RoundedFieldChrome.DrawRing` strokes C#'s Border band).
//!
//! Not ported here (duplicates, compared by what they assert):
//! - `SliderFillsTheTrackLeftOfTheThumbInTheThemeAccent` =
//!   `src/widgets/slider/style_tests.rs::fill_colours_the_trailing_fill`.
//! - `ExplicitSliderColorsWinOverTheTheme` =
//!   `src/widgets/slider/tests.rs::slider_style_track_and_thumb_colors_are_used`.
//! - `PopupMarksTheSelectedRowApartFromTheHoveredRow` =
//!   `src/tests/widgets/combo_state_style.rs::combo_state_style_popup_fill_and_item_colours`.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::widgets::combo_box::ComboBoxStyle;
use agg_gui::{
    Color, ComboBox, Event, EventResult, Modifiers, MouseButton, Point, Rect, Size, Slider,
    SliderOrientation, Widget,
};

use super::support::{font, install_theme, is_near, luma, pixel, render_at, rgb};

/// C#'s `NewSlider`: a 160 px slider at value .5 whose track centre line is
/// y = 50 of a 200 x 100 root. agg-gui's default slider shows a value
/// readout; C#'s shows none until given text, so it is off here.
fn new_slider() -> Slider {
    let mut slider = Slider::new(0.5, 0.0, 1.0, font()).with_show_value(false);
    let size = slider.layout(Size::new(160.0, 100.0));
    slider.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    slider
}

/// Where [`new_slider`] is placed in its root: centred on y = 50.
fn slider_origin(slider: &Slider) -> (f64, f64) {
    (20.0, 50.0 - slider.bounds().height * 0.5)
}

fn render_slider(slider: &mut Slider, background: Color) -> agg_gui::Framebuffer {
    let at = slider_origin(slider);
    render_at(slider, (200, 100), at, background)
}

/// The middle of the thumb's ring, in root pixels.
fn ring_pixel(slider: &Slider) -> (i32, i32) {
    let thumb = slider.thumb_rect();
    let (ox, oy) = slider_origin(slider);
    let r = thumb.width * 0.5;
    let cx = ox + thumb.x + r;
    let cy = oy + thumb.y + thumb.height * 0.5;
    ((cx - r * (1.0 - 2.5 / 14.0)) as i32, cy as i32)
}

#[test]
fn slider_bounds_are_what_they_were_before_the_restyle() {
    let mut slider = Slider::new(0.5, 0.0, 1.0, font());
    assert_eq!(
        slider.layout(Size::new(160.0, 100.0)),
        Size::new(160.0, 22.0)
    );

    let mut vertical =
        Slider::new(0.5, 0.0, 1.0, font()).with_orientation(SliderOrientation::Vertical);
    assert_eq!(
        vertical.layout(Size::new(20.0, 400.0)),
        Size::new(20.0, 140.0)
    );
}

#[test]
fn drop_down_size_is_what_it_was_before_the_restyle() {
    let mut drop_down =
        ComboBox::new(vec!["A much longer item", "B"], 0, font()).with_fit_width(true);
    let size = drop_down.layout(Size::new(400.0, 100.0));
    assert_eq!(size, DROP_DOWN_BASELINE_SIZE);
}

/// Captured before the chevron and the inset outline, from the same font.
const DROP_DOWN_BASELINE_SIZE: Size = Size {
    width: 150.0,
    height: 24.0,
};

#[test]
fn slider_thumb_shades_for_hover_and_drag_and_goes_grey_disabled() {
    let v = install_theme(true);
    let enabled = Rc::new(Cell::new(true));
    let gate = Rc::clone(&enabled);
    let mut slider = new_slider().with_enabled_fn(move || gate.get());
    let (ring_x, ring_y) = ring_pixel(&slider);
    let thumb = slider.thumb_rect();
    let center = Point::new(thumb.x + thumb.width * 0.5, thumb.y + thumb.height * 0.5);

    let normal = pixel(&render_slider(&mut slider, v.bg_color), ring_x, ring_y);

    slider.on_event(&Event::MouseMove { pos: center });
    let hovered = pixel(&render_slider(&mut slider, v.bg_color), ring_x, ring_y);

    slider.on_event(&Event::MouseDown {
        pos: center,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    let dragged = pixel(&render_slider(&mut slider, v.bg_color), ring_x, ring_y);
    slider.on_event(&Event::MouseUp {
        pos: center,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    enabled.set(false);
    let disabled = render_slider(&mut slider, v.bg_color);
    let disabled_fill = pixel(&disabled, 40, 50);

    assert!(
        luma(hovered) > luma(normal),
        "hover lightens the thumb: {hovered:?} {normal:?}"
    );
    assert!(
        luma(dragged) < luma(normal),
        "dragging darkens the thumb: {dragged:?} {normal:?}"
    );
    assert!(
        !is_near(disabled_fill, rgb(v.accent), 40),
        "a disabled slider drops the accent: {disabled_fill:?}"
    );
    assert!(!slider.is_enabled());
    assert_eq!(
        slider.on_event(&Event::MouseMove { pos: center }),
        EventResult::Ignored,
        "a disabled slider takes no input"
    );
}

/// C#'s root: a 200 x 60 field of the theme background with the drop down
/// at (10, 20).
fn render_drop_down(drop_down: &mut ComboBox, background: Color) -> agg_gui::Framebuffer {
    let size = drop_down.layout(Size::new(180.0, 60.0));
    drop_down.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    render_at(drop_down, (200, 60), (10.0, 20.0), background)
}

#[test]
fn drop_down_draws_a_chevron_in_its_text_color() {
    for dark in [false, true] {
        let v = install_theme(dark);
        let mut drop_down = ComboBox::new(vec!["Item"], 0, font()).with_fit_width(true);
        let image = render_drop_down(&mut drop_down, v.bg_color);
        let b = drop_down.bounds();

        // A chevron is two strokes meeting at a point: the column through its
        // tip has ink at the tip but not above it, where a filled triangle
        // would be solid. agg-gui's arrow box is the 20 px strip at the right
        // and the chevron is 4 px half wide, 2 px half tall.
        let half_h = 2.0;
        let center = (10.0 + b.width - 10.0, 20.0 + b.height * 0.5);
        let x = center.0.round() as i32;
        let tip_y = (center.1 - half_h).round() as i32;
        let top_y = (center.1 + half_h).round() as i32 - 1;

        let tip = pixel(&image, x, tip_y);
        let top = pixel(&image, x, top_y);
        assert!(
            is_near(tip, rgb(v.text_color), 120),
            "the chevron's tip was {tip:?} (dark {dark})"
        );
        assert!(
            is_near(top, rgb(v.widget_bg), 30),
            "above the tip a chevron is open, found {top:?} (dark {dark})"
        );
    }
}

/// The field is rounded: its outline runs through the band just inside the
/// box, so at the very corner of that band there is background, while the
/// middle of each edge is outline.
#[test]
fn drop_down_field_is_rounded_inside_the_old_border_ring() {
    for dark in [false, true] {
        let v = install_theme(dark);
        let mut drop_down = ComboBox::new(vec!["Item"], 0, font())
            .with_fit_width(true)
            .with_style(ComboBoxStyle {
                border: Some(Color::rgb(1.0, 0.0, 0.0)),
                ..Default::default()
            });
        let image = render_drop_down(&mut drop_down, v.bg_color);
        let b = drop_down.bounds();
        let (left, bottom) = (10, 20);
        let center = (10.0 + b.width * 0.5, 20.0 + b.height * 0.5);

        let corner = pixel(&image, left, bottom);
        let edge = pixel(&image, center.0 as i32, bottom);
        assert!(
            is_near(corner, rgb(v.bg_color), 30),
            "the rounded corner leaves background, found {corner:?} (dark {dark})"
        );
        assert!(
            edge[0] > 150 && edge[1] < 100,
            "the outline runs through the ring, found {edge:?} (dark {dark})"
        );

        // And the field itself is filled with the theme's widget fill.
        let field = pixel(&image, left + 4, center.1 as i32);
        assert!(
            is_near(field, rgb(v.widget_bg), 30),
            "field fill was {field:?} (dark {dark})"
        );
    }
}
