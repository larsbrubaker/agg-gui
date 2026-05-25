//! Runtime hint describing the user's primary input device.
//!
//! Distinct from [`crate::platform::Platform`] (which tracks the OS family
//! for shortcut labels — Cmd vs. Ctrl) because a Mac user with a
//! touchscreen MacBook and an iPad user both run `Platform::MacOS` but
//! need very different text-entry experiences.
//!
//! The input profile drives features that should only exist on mobile
//! touch devices:
//!
//! - The agg-gui on-screen software keyboard
//!   ([`crate::widgets::on_screen_keyboard`])
//! - Hit-target padding around small interactive widgets (future)
//! - Long-press gesture timing (future)
//!
//! Native builds default to [`InputProfile::Desktop`]. WASM hosts call
//! [`set_input_profile`] after sniffing `navigator.userAgent` +
//! `matchMedia("(pointer: coarse)")` so the agg-gui-side mobile features
//! activate. The host can also call [`platform_from_name`] /
//! [`set_platform`](crate::platform::set_platform) so shortcut labels match
//! the user's keyboard while the on-screen keyboard mimics their phone OS.

use std::sync::atomic::{AtomicU8, Ordering};

/// Where keyboard / pointer events originate and how text entry should
/// behave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputProfile {
    /// Physical keyboard + precise pointer (mouse / trackpad). The default.
    /// No on-screen keyboard.
    Desktop,
    /// iPhone / iPad / iPad-mode Safari. Touch primary, no physical
    /// keyboard. On-screen keyboard renders with iOS-style chrome
    /// (rounded keys, light surface, blue accent).
    MobileIOS,
    /// Android phone or tablet (Chrome / Firefox / Samsung Internet).
    /// On-screen keyboard renders with Material-style chrome (flatter
    /// keys, system accent).
    MobileAndroid,
    /// Touch device we can't otherwise classify — e.g. a Linux tablet.
    /// On-screen keyboard renders with a neutral default.
    MobileOther,
}

impl InputProfile {
    /// `true` when the profile implies the user has no physical keyboard
    /// and the on-screen keyboard should be available.
    pub fn is_mobile_touch(self) -> bool {
        matches!(
            self,
            InputProfile::MobileIOS | InputProfile::MobileAndroid | InputProfile::MobileOther
        )
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(profile_code(InputProfile::Desktop));

/// Replace the global input profile. Call once at startup from the
/// platform shell after detecting the device.
pub fn set_input_profile(profile: InputProfile) {
    CURRENT.store(profile_code(profile), Ordering::Relaxed);
}

/// Read the global input profile.
pub fn current_input_profile() -> InputProfile {
    profile_from_code(CURRENT.load(Ordering::Relaxed))
}

/// Convenience: detect mobile-touch from current profile.
pub fn is_mobile_touch() -> bool {
    current_input_profile().is_mobile_touch()
}

/// Parse a coarse browser identifier ("iPhone", "iPad", "Android", …)
/// into an [`InputProfile`]. Defaults to [`InputProfile::Desktop`] so a
/// non-matching string (any desktop UA) keeps mobile features disabled.
///
/// `pointer_coarse` should reflect `window.matchMedia('(pointer: coarse)')`
/// — true on iPad-mode Safari that hides "iPad" from the UA, false on a
/// MacBook trackpad. Set it to `false` if you don't have a reliable read.
pub fn input_profile_from_hint(user_agent_or_platform: &str, pointer_coarse: bool) -> InputProfile {
    let ua = user_agent_or_platform.to_ascii_lowercase();
    if ua.contains("iphone") || ua.contains("ipad") || ua.contains("ipod") {
        return InputProfile::MobileIOS;
    }
    if ua.contains("android") {
        return InputProfile::MobileAndroid;
    }
    // iPad-mode Safari masquerades as macOS in the UA. Coarse-pointer +
    // mac signals an iPad-class device in practice.
    if pointer_coarse && (ua.contains("mac") || ua.contains("darwin")) {
        return InputProfile::MobileIOS;
    }
    if pointer_coarse {
        return InputProfile::MobileOther;
    }
    InputProfile::Desktop
}

const fn profile_code(p: InputProfile) -> u8 {
    match p {
        InputProfile::Desktop => 0,
        InputProfile::MobileIOS => 1,
        InputProfile::MobileAndroid => 2,
        InputProfile::MobileOther => 3,
    }
}

fn profile_from_code(c: u8) -> InputProfile {
    match c {
        1 => InputProfile::MobileIOS,
        2 => InputProfile::MobileAndroid,
        3 => InputProfile::MobileOther,
        _ => InputProfile::Desktop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ua_routes_to_correct_profile() {
        assert_eq!(
            input_profile_from_hint("Mozilla/5.0 (iPhone; CPU iPhone OS 17_4)", true),
            InputProfile::MobileIOS
        );
        assert_eq!(
            input_profile_from_hint("Mozilla/5.0 (Linux; Android 14; Pixel 8)", true),
            InputProfile::MobileAndroid
        );
        // iPad-mode Safari reports macOS in the UA but the pointer-coarse
        // hint pulls us back to MobileIOS.
        assert_eq!(
            input_profile_from_hint("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit", true),
            InputProfile::MobileIOS
        );
        // Same UA without a coarse pointer = desktop Mac.
        assert_eq!(
            input_profile_from_hint("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit", false),
            InputProfile::Desktop
        );
        // Unknown touch device.
        assert_eq!(input_profile_from_hint("CrOS x86_64", true), InputProfile::MobileOther);
    }

    #[test]
    fn is_mobile_touch_helper() {
        assert!(!InputProfile::Desktop.is_mobile_touch());
        assert!(InputProfile::MobileIOS.is_mobile_touch());
        assert!(InputProfile::MobileAndroid.is_mobile_touch());
        assert!(InputProfile::MobileOther.is_mobile_touch());
    }
}
