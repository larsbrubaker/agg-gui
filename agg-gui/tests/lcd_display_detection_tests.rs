//! agg-sharp `Tests/Agg.Tests/Agg/LcdDisplayDetectionTests.cs`, ported 1:1 (same
//! test names, same expected values): covers `agg_gui::lcd_display_detection`,
//! the policy that picks a default for subpixel text from what the OS says about
//! the display. Pure logic over injected facts - no OS calls, no display - so the
//! cases a developer can never reproduce locally (BGR panel, remote session,
//! rotated monitor) are all testable. The Windows reader that gathers the facts
//! lives in agg-gui-shell.

use agg_gui::lcd_display_detection::{
    is_subpixel_appropriate, is_subpixel_appropriate_for, LcdDisplayEnvironment,
    LcdDisplayEnvironmentProvider, LcdFontSmoothingStyle, LcdStripeOrder,
};

/// The one combination that says yes: smoothing on, ClearType, RGB stripes,
/// local session, upright display. This is the ordinary desktop, and getting it
/// wrong would leave 95% of users on grayscale.
#[test]
fn an_ordinary_clear_type_desktop_gets_subpixel() {
    assert!(is_subpixel_appropriate(&ordinary_desktop()));
}

#[test]
fn font_smoothing_off_means_grayscale() {
    let environment = LcdDisplayEnvironment {
        font_smoothing_enabled: false,
        font_smoothing_style: LcdFontSmoothingStyle::ClearType,
        stripe_order: LcdStripeOrder::Rgb,
        is_remote_session: false,
        display_rotated_quarter_turn: false,
    };

    assert!(
        !is_subpixel_appropriate(&environment),
        "a user who turned font smoothing off asked for hard edges, not coloured ones"
    );
}

/// Grayscale smoothing is what a user picks to keep antialiasing but turn
/// ClearType off, so it is an explicit "no subpixel" rather than an absence of
/// information.
#[test]
fn grayscale_smoothing_style_means_grayscale() {
    let environment = LcdDisplayEnvironment {
        font_smoothing_enabled: true,
        font_smoothing_style: LcdFontSmoothingStyle::Grayscale,
        stripe_order: LcdStripeOrder::Rgb,
        is_remote_session: false,
        display_rotated_quarter_turn: false,
    };

    assert!(!is_subpixel_appropriate(&environment));
}

#[test]
fn an_unknown_smoothing_style_means_grayscale() {
    let environment = LcdDisplayEnvironment {
        font_smoothing_enabled: true,
        font_smoothing_style: LcdFontSmoothingStyle::Unknown,
        stripe_order: LcdStripeOrder::Rgb,
        is_remote_session: false,
        display_rotated_quarter_turn: false,
    };

    assert!(
        !is_subpixel_appropriate(&environment),
        "nothing said this display is subpixel, and a guess would be visible on every glyph"
    );
}

/// The LCD coverage pipeline renders RGB stripe order only - see
/// `is_subpixel_appropriate` - so a BGR panel would get its fringes on the wrong
/// side of every stem. Grayscale is the honest fallback.
#[test]
fn bgr_stripe_order_falls_back_to_grayscale() {
    let environment = LcdDisplayEnvironment {
        font_smoothing_enabled: true,
        font_smoothing_style: LcdFontSmoothingStyle::ClearType,
        stripe_order: LcdStripeOrder::Bgr,
        is_remote_session: false,
        display_rotated_quarter_turn: false,
    };

    assert!(
        !is_subpixel_appropriate(&environment),
        "the renderer has no BGR path, so rendering RGB anyway would look worse than grayscale"
    );
}

#[test]
fn a_remote_session_falls_back_to_grayscale() {
    let environment = LcdDisplayEnvironment {
        font_smoothing_enabled: true,
        font_smoothing_style: LcdFontSmoothingStyle::ClearType,
        stripe_order: LcdStripeOrder::Rgb,
        is_remote_session: true,
        display_rotated_quarter_turn: false,
    };

    assert!(
        !is_subpixel_appropriate(&environment),
        "the pixels are re-encoded on the way to a panel whose geometry we cannot know"
    );
}

#[test]
fn a_quarter_turned_display_falls_back_to_grayscale() {
    let environment = LcdDisplayEnvironment {
        font_smoothing_enabled: true,
        font_smoothing_style: LcdFontSmoothingStyle::ClearType,
        stripe_order: LcdStripeOrder::Rgb,
        is_remote_session: false,
        display_rotated_quarter_turn: true,
    };

    assert!(
        !is_subpixel_appropriate(&environment),
        "a monitor on its side has vertical stripes, and the subpixel geometry is horizontal"
    );
}

/// No provider at all is every non-Windows platform, and a provider that cannot
/// read the display is a stripped or headless Windows host. Both mean "we do not
/// know", which must never render subpixel.
#[test]
fn a_missing_or_silent_provider_means_grayscale() {
    assert!(!is_subpixel_appropriate_for(None));

    assert!(
        !is_subpixel_appropriate_for(Some(&StubProvider::new(false, ordinary_desktop()))),
        "a provider that returned nothing has told us nothing, whatever it was holding"
    );
}

#[test]
fn a_provider_that_can_read_the_display_decides_from_what_it_read() {
    assert!(is_subpixel_appropriate_for(Some(&StubProvider::new(
        true,
        ordinary_desktop()
    ))));
}

fn ordinary_desktop() -> LcdDisplayEnvironment {
    LcdDisplayEnvironment {
        font_smoothing_enabled: true,
        font_smoothing_style: LcdFontSmoothingStyle::ClearType,
        stripe_order: LcdStripeOrder::Rgb,
        is_remote_session: false,
        display_rotated_quarter_turn: false,
    }
}

/// C#'s `StubProvider`: `can_read` is `TryGetEnvironment`'s return value, and
/// `environment` what it would have written to its out parameter.
struct StubProvider {
    can_read: bool,
    environment: LcdDisplayEnvironment,
}

impl StubProvider {
    fn new(can_read: bool, environment: LcdDisplayEnvironment) -> Self {
        Self {
            can_read,
            environment,
        }
    }
}

impl LcdDisplayEnvironmentProvider for StubProvider {
    fn try_get_environment(&self) -> Option<LcdDisplayEnvironment> {
        self.can_read.then_some(self.environment)
    }
}
