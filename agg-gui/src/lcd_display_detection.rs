//! Whether LCD subpixel text suits the display the app runs on: the policy that
//! picks a *default* for subpixel text from what the OS says about the display.
//!
//! Port of agg-sharp's `agg/LcdCoverage/LcdDisplayDetection.cs`. Pure logic over
//! injected facts, so the cases a developer can never reproduce locally (a BGR
//! panel, a remote session, a rotated monitor) are all testable. The facts come
//! from an [`LcdDisplayEnvironmentProvider`]; `agg-gui-shell` has the Windows
//! reader (`WindowsLcdDisplayEnvironmentProvider`), which is the only platform
//! that reports them. An app owns the user setting that overrides the default
//! and publishes the result with [`crate::font_settings::set_lcd_enabled`];
//! the [`crate::lcd_coverage`] pipeline is what renders it.

/// What kind of font antialiasing the desktop is configured for. The variants
/// are the Windows `FE_FONTSMOOTHING*` values (see [`Self::from_windows`]),
/// because Windows is the only platform that reports this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LcdFontSmoothingStyle {
    /// Nothing the display told us about - treat as "not subpixel".
    Unknown,
    /// `FE_FONTSMOOTHINGSTANDARD`: grayscale antialiasing.
    Grayscale,
    /// `FE_FONTSMOOTHINGCLEARTYPE`: subpixel antialiasing.
    ClearType,
}

impl LcdFontSmoothingStyle {
    /// The style `SPI_GETFONTSMOOTHINGTYPE` reported: `1` grayscale, `2`
    /// ClearType, anything else unknown (C# casts the integer to its enum, and
    /// an unnamed value is likewise "not ClearType").
    pub fn from_windows(value: u32) -> Self {
        match value {
            1 => Self::Grayscale,
            2 => Self::ClearType,
            _ => Self::Unknown,
        }
    }
}

/// Which way the panel's colour stripes run within a pixel. The variants are
/// the Windows `FE_FONTSMOOTHINGORIENTATION*` values (see [`Self::from_windows`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LcdStripeOrder {
    /// `FE_FONTSMOOTHINGORIENTATIONBGR`.
    Bgr,
    /// `FE_FONTSMOOTHINGORIENTATIONRGB`.
    Rgb,
}

impl LcdStripeOrder {
    /// The order `SPI_GETFONTSMOOTHINGORIENTATION` reported: `1` is RGB, and
    /// every other value is "not RGB", which is all the decision asks (C#
    /// casts the integer to its enum, where an unnamed value is not RGB either).
    pub fn from_windows(value: u32) -> Self {
        if value == 1 {
            Self::Rgb
        } else {
            Self::Bgr
        }
    }
}

/// The facts about the display that decide whether subpixel text is a good
/// idea, gathered from the OS. Platform neutral on purpose: a provider fills it
/// in, and [`is_subpixel_appropriate`] decides from it without knowing where the
/// values came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LcdDisplayEnvironment {
    /// Whether the desktop smooths font edges at all (`SPI_GETFONTSMOOTHING`).
    pub font_smoothing_enabled: bool,
    /// Grayscale or subpixel smoothing (`SPI_GETFONTSMOOTHINGTYPE`).
    pub font_smoothing_style: LcdFontSmoothingStyle,
    /// Stripe order of the panel (`SPI_GETFONTSMOOTHINGORIENTATION`).
    pub stripe_order: LcdStripeOrder,
    /// Whether the app is being viewed over a remote desktop connection
    /// (`SM_REMOTESESSION`).
    pub is_remote_session: bool,
    /// Whether the display is turned 90 or 270 degrees (`DMDO_90` /
    /// `DMDO_270`), which puts the stripes on the vertical axis.
    pub display_rotated_quarter_turn: bool,
}

/// Reads the current display's [`LcdDisplayEnvironment`]. Implemented per
/// platform; only Windows can answer.
pub trait LcdDisplayEnvironmentProvider {
    /// Reads the display environment, or `None` when this platform cannot say -
    /// a provider that does not know must say so rather than guess, because a
    /// wrong guess turns subpixel geometry on under a display it does not suit.
    fn try_get_environment(&self) -> Option<LcdDisplayEnvironment>;
}

/// Whether subpixel rendering suits `environment`. Every condition has to hold;
/// any one of them failing means grayscale is the better default.
///
/// The conditions, and why each disqualifies subpixel:
/// - Font smoothing off - the user asked for hard edged text; adding colour
///   fringes to it is the opposite of what they asked for.
/// - Smoothing style not ClearType - Windows itself decided this display should
///   get grayscale, and it knows things we do not (it is what a user picks to
///   turn ClearType off while keeping antialiasing).
/// - BGR stripe order - **the LCD coverage pipeline only renders RGB order**.
///   Its mask, filter and composite all assume the coverage triple maps to red,
///   green, blue left to right ([`crate::lcd_coverage`]), and nothing takes a
///   stripe order. On a BGR panel that rendering puts the fringes on the wrong
///   side of each stem, which looks worse than grayscale, so BGR falls back
///   rather than rendering wrong.
/// - Remote session - the pixels are re-encoded and shipped over a wire, where
///   the colour fringes both compress badly and land on a panel whose geometry
///   we cannot know.
/// - Quarter turn rotation - a display on its side has its stripes running
///   vertically, and horizontal subpixel geometry addresses the wrong axis.
///
/// This only picks a **default**: an explicit user choice always wins over it.
pub fn is_subpixel_appropriate(environment: &LcdDisplayEnvironment) -> bool {
    environment.font_smoothing_enabled
        && environment.font_smoothing_style == LcdFontSmoothingStyle::ClearType
        && environment.stripe_order == LcdStripeOrder::Rgb
        && !environment.is_remote_session
        && !environment.display_rotated_quarter_turn
}

/// Whether subpixel rendering suits the display `provider` describes (C#'s
/// `IsSubpixelAppropriate(ILcdDisplayEnvironmentProvider)` overload). False when
/// there is no provider or it cannot read the display - "we do not know"
/// defaults to grayscale, which is what every non-Windows platform gets.
pub fn is_subpixel_appropriate_for(provider: Option<&dyn LcdDisplayEnvironmentProvider>) -> bool {
    provider
        .and_then(LcdDisplayEnvironmentProvider::try_get_environment)
        .is_some_and(|environment| is_subpixel_appropriate(&environment))
}
