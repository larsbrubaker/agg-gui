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

    /// Recommended [`crate::ux_scale`] multiplier for this profile.
    /// `1.0` for desktop; ~`1.7` for mobile touch (phones held at
    /// arm's length need ~44 px touch targets and ~17 px body text,
    /// which is roughly 1.7× what reads well on a desktop monitor).
    ///
    /// Apps that want a different feel can override with
    /// [`crate::ux_scale::set_ux_scale`] *after* the profile is
    /// applied — accessibility settings, for example.
    pub fn recommended_ux_scale(self) -> f64 {
        match self {
            InputProfile::Desktop => 1.0,
            InputProfile::MobileIOS | InputProfile::MobileAndroid | InputProfile::MobileOther => {
                1.7
            }
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(profile_code(InputProfile::Desktop));

/// Replace the global input profile. Call once at startup from the
/// platform shell after detecting the device, and at most once more
/// if the device changes (e.g. a tablet docked into a desktop mode).
///
/// **Deliberately does NOT change [`crate::ux_scale`].** Earlier
/// drafts auto-applied [`InputProfile::recommended_ux_scale`] here,
/// but that meant programmatic profile changes (e.g. a demo's
/// "preview as iPhone" radio) silently resized the entire UI, which
/// is a surprise. The platform shell is the only place that knows
/// whether the user is really on a touch device; it calls
/// `set_ux_scale` explicitly. Demos / sandboxes can flip
/// `InputProfile` without affecting on-screen UI scale.
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

/// The signal the UI **sizing policy** consults to decide whether
/// interactive targets must meet the touch minimum (see
/// [`crate::widgets::menu::effective_metrics`]).
///
/// True when EITHER:
/// - the platform declared a mobile-touch profile ([`is_mobile_touch`]) —
///   the app called [`set_input_profile`] with a mobile variant, so we know
///   up front the user is on a phone/tablet; OR
/// - a real touch event has fired this session
///   ([`crate::touch_state::touch_seen_this_session`]) — a runtime fallback
///   for shells that *forgot* to call [`set_input_profile`] /
///   [`crate::ux_scale::set_ux_scale`] (which has happened in real apps).
///   Without it, such a phone would ship desktop-sized (26 CSS-px) menus;
///   with it, the first touch flips the latch and the menus can never stay
///   accidentally tiny.
///
/// Deliberately distinct from [`is_mobile_touch`]: that one drives the
/// on-screen keyboard and other "no physical keyboard" features, which must
/// NOT turn on just because a touchscreen laptop was tapped once.  Sizing
/// minimums are safe to raise on any touch input, so this signal is broader.
pub fn touch_ui_active() -> bool {
    is_mobile_touch() || crate::touch_state::touch_seen_this_session()
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

/// Serialization lock shared by every test that reads OR writes the
/// process-global input profile [`CURRENT`].
///
/// The profile is a process-wide atomic, so under cargo's parallel test
/// threads one test's `set_input_profile(Desktop)` can be clobbered by a
/// sibling test's `set_input_profile(MobileIOS)` before the first test
/// reads it back — a cross-thread flake window. Tests that touch the
/// profile (menu geometry / widget / strip metric tests, the on-screen
/// keyboard tests, and this module's own tests) hold this guard for their
/// full body so at most one runs at a time.
///
/// Poison-proof: a panicking test (a failed assertion unwinding while
/// holding the guard) still leaves the lock usable, so one real failure
/// can't cascade into spurious `PoisonError` panics across every other
/// profile test.
#[cfg(test)]
pub(crate) fn profile_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
            input_profile_from_hint(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit",
                true
            ),
            InputProfile::MobileIOS
        );
        // Same UA without a coarse pointer = desktop Mac.
        assert_eq!(
            input_profile_from_hint(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit",
                false
            ),
            InputProfile::Desktop
        );
        // Unknown touch device.
        assert_eq!(
            input_profile_from_hint("CrOS x86_64", true),
            InputProfile::MobileOther
        );
    }

    #[test]
    fn is_mobile_touch_helper() {
        assert!(!InputProfile::Desktop.is_mobile_touch());
        assert!(InputProfile::MobileIOS.is_mobile_touch());
        assert!(InputProfile::MobileAndroid.is_mobile_touch());
        assert!(InputProfile::MobileOther.is_mobile_touch());
    }

    #[test]
    fn touch_ui_active_latches_on_a_real_touch_even_on_desktop_profile() {
        let _guard = profile_test_lock();
        // The runtime fallback: a shell that never called `set_input_profile`
        // stays on the Desktop profile, yet the first real touch must flip
        // the sizing signal so menus can't stay accidentally tiny.
        set_input_profile(InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        assert!(
            !touch_ui_active(),
            "desktop profile with no touch yet must not force touch sizing"
        );

        crate::touch_state::note_touch_event();
        assert!(
            touch_ui_active(),
            "a real touch event must activate touch sizing regardless of profile"
        );

        // Restore for sibling tests (profile is a process-global atomic).
        crate::touch_state::clear_last_touch_event_for_testing();
    }

    #[test]
    fn touch_ui_active_follows_mobile_profile_without_any_touch() {
        let _guard = profile_test_lock();
        // The declared-profile half: an app that DID call `set_input_profile`
        // gets touch sizing immediately, before any touch has occurred.
        crate::touch_state::clear_last_touch_event_for_testing();
        set_input_profile(InputProfile::MobileIOS);
        assert!(touch_ui_active());
        set_input_profile(InputProfile::Desktop);
        assert!(!touch_ui_active());
    }
}
