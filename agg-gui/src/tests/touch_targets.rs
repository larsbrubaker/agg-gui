//! Phone sizing: the UX zoom a phone or tablet profile recommends brings
//! agg-gui's 14 px body text to about 16 CSS px (not 1.7x, which made text
//! 24 px and the on-screen keyboard twice Gboard's height), and tappable
//! controls are at least 44 CSS px tall there through their minimum heights
//! while the desktop keeps its own.

use std::sync::Arc;

use crate::input_profile::{
    set_input_profile, touch_target_min, InputProfile, TOUCH_TARGET_CSS_PX,
};
use crate::text::Font;
use crate::widgets::{Button, Checkbox, RadioButton, RadioGroup, TabView};
use crate::{Size, Widget};

use super::TEST_FONT;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

#[test]
fn phones_zoom_body_text_to_about_sixteen_css_px() {
    let zoom = InputProfile::MobileAndroid.recommended_ux_scale();
    let body = 14.0 * zoom;
    assert!((15.5..=17.0).contains(&body), "body text {body} CSS px");
    assert_eq!(InputProfile::MobileIOS.recommended_ux_scale(), zoom);
    assert_eq!(InputProfile::Desktop.recommended_ux_scale(), 1.0);
}

/// The CSS-px heights a button, a check box, a radio button, a radio
/// group's row and a tab bar
/// take under `profile` at its recommended zoom.
fn control_heights(profile: InputProfile) -> [f64; 5] {
    set_input_profile(profile);
    let zoom = profile.recommended_ux_scale();
    crate::ux_scale::set_ux_scale(zoom);
    let avail = Size::new(300.0, 400.0);
    let button = Button::new("OK", font()).layout(avail).height;
    let check = Checkbox::new("Snap", font(), false).layout(avail).height;
    let radio = RadioButton::new("Top", font()).layout(avail).height;
    let group = RadioGroup::new(vec!["A", "B"], 0, font())
        .layout(avail)
        .height
        / 2.0;
    let mut tabs = TabView::new(font());
    tabs.layout(avail);
    tabs.set_bounds(crate::Rect::new(0.0, 0.0, avail.width, avail.height));
    let bar = avail.height - tabs.content_height();
    crate::ux_scale::set_ux_scale(1.0);
    set_input_profile(InputProfile::Desktop);
    [button, check, radio, group, bar].map(|h| h * zoom)
}

#[test]
fn tappable_controls_are_a_fingers_height_on_a_phone_and_unchanged_on_the_desktop() {
    let _lock = crate::input_profile::profile_test_lock();
    let desktop = control_heights(InputProfile::Desktop);
    assert!(
        desktop[0] < TOUCH_TARGET_CSS_PX,
        "desktop button {}",
        desktop[0]
    );
    for profile in [InputProfile::MobileAndroid, InputProfile::MobileIOS] {
        let phone = control_heights(profile);
        for (name, h) in [
            "button",
            "check box",
            "radio button",
            "radio group row",
            "tab bar",
        ]
        .iter()
        .zip(phone)
        {
            assert!(
                h >= TOUCH_TARGET_CSS_PX - 1e-9,
                "{profile:?}: the {name} is {h} CSS px"
            );
        }
    }
    assert_eq!(touch_target_min(), 0.0, "the desktop sets no minimum");
}
