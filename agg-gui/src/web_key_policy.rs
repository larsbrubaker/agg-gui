//! Which browser keydowns the web keyboard bridge (`web_adapter`'s
//! `install_keyboard_listeners`) `preventDefault()`s after forwarding them
//! to the app, and which keyups it forwards ([`HeldKeys`]). Pure logic with no DOM types, so it compiles and is tested
//! on every target; only the wasm `web_adapter` calls it.
//!
//! An app claims more Ctrl/Cmd shortcuts from the browser with
//! [`claim_web_shortcut`] (Ctrl/Cmd+S for its own Save, so the browser's
//! "Save Page As" does not open). Claims are per thread, the browser's UI
//! thread being the only one that sees key events.

use std::cell::RefCell;

use crate::event::{Key, Modifiers};

std::thread_local! {
    /// Lowercased keys an app claimed through [`claim_web_shortcut`].
    static CLAIMED: RefCell<Vec<char>> = const { RefCell::new(Vec::new()) };
}

/// The keys every app gets: the clipboard and undo/redo set.
const BUILT_IN_CLAIMS: [char; 5] = ['c', 'x', 'a', 'z', 'y'];

/// Claim Ctrl/Cmd+`key` (with or without Shift) from the browser: the web
/// keyboard bridge then `preventDefault()`s it after forwarding it to the
/// app, so the browser's own action for it (Ctrl/Cmd+S "Save Page As",
/// Ctrl/Cmd+Shift+S, Ctrl/Cmd+P print, ...) does not run. Claim only
/// shortcuts the app handles. Alt combinations are never claimed (Alt+Left is
/// history back; AltGr characters report Ctrl+Alt). Matching ignores case.
/// Native builds have no browser, so a claim changes nothing there.
pub fn claim_web_shortcut(key: char) {
    let key = key.to_ascii_lowercase();
    CLAIMED.with(|c| {
        let mut c = c.borrow_mut();
        if !c.contains(&key) {
            c.push(key);
        }
    });
}

/// Give Ctrl/Cmd+`key` back to the browser after [`claim_web_shortcut`].
/// The built-in clipboard and undo set (C, X, A, Z, Y) stays claimed.
pub fn release_web_shortcut(key: char) {
    let key = key.to_ascii_lowercase();
    CLAIMED.with(|c| c.borrow_mut().retain(|k| *k != key));
}

/// Whether Ctrl/Cmd+`key` is claimed for the app: the built-in set or a
/// [`claim_web_shortcut`] claim.
pub fn is_web_shortcut_claimed(key: char) -> bool {
    let key = key.to_ascii_lowercase();
    BUILT_IN_CLAIMS.contains(&key) || CLAIMED.with(|c| c.borrow().contains(&key))
}

/// Whether a forwarded keydown should suppress the browser's default
/// action.  The listeners sit at window level, so this must block only
/// page side effects of in-app typing (space scrolls, arrows scroll,
/// `'` / `/` open Firefox quick-find, Backspace navigates back) while
/// leaving browser chrome usable: `Tab` focus-navigation, modified
/// shortcuts like Ctrl+R / Cmd+L, and the F-keys stay with the browser.
/// Ctrl/Cmd C, X, A, Z, Y are the app's clipboard/undo set and are
/// claimed, as is any key an app claimed with [`claim_web_shortcut`].
/// Alt combos (Alt+Left = history back, AltGr chars report
/// ctrl+alt) are never suppressed.
///
/// F1 is the one F-key claimed: desktop apps bind it to their own help,
/// and the browser's F1 (opening the browser's help page in a new tab)
/// is never what a user pressing F1 inside the app wants.  The other
/// F-keys keep their browser meaning (F3 find, F5 reload, F6 address
/// bar, F7 caret browsing, F11 full screen, F12 developer tools); keys
/// with no browser action (F2, F4, ...) are forwarded either way, so
/// suppressing them would change nothing.
// Only the wasm `web_adapter` calls this outside the tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn should_prevent_default(k: &Key, mods: Modifiers) -> bool {
    if mods.ctrl || mods.meta {
        return matches!(k, Key::Char(c) if is_web_shortcut_claimed(*c)) && !mods.alt;
    }
    if mods.alt {
        return false;
    }
    match k {
        Key::F(n) => *n == 1,
        _ => matches!(
            k,
            Key::Char(_)
                | Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::Backspace
                | Key::Delete
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
                | Key::Enter
        ),
    }
}

/// The physical keys whose keydown went to the app and whose keyup has not
/// yet arrived. A keyup for one of them goes to the app even when focus has
/// moved meanwhile (Tab moving focus off a canvas, a click into a page field
/// while a key is held), so no key the app saw pressed stays down in it.
#[derive(Default, Debug)]
// Only the wasm `web_adapter` uses this outside the tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct HeldKeys {
    codes: Vec<String>,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl HeldKeys {
    /// A keydown for physical key `code` (`KeyboardEvent.code`) went to the app.
    pub(crate) fn pressed(&mut self, code: &str) {
        if !self.codes.iter().any(|c| c == code) {
            self.codes.push(code.to_string());
        }
    }

    /// Whether the keyup for `code` goes to the app: always when its keydown
    /// did, else when the keyboard belongs to the app now (`belongs_now`).
    /// Either way `code` is no longer held.
    pub(crate) fn released(&mut self, code: &str, belongs_now: bool) -> bool {
        let held = match self.codes.iter().position(|c| c == code) {
            Some(i) => {
                self.codes.remove(i);
                true
            }
            None => false,
        };
        held || belongs_now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyup_reaches_the_app_after_focus_left_when_its_keydown_did() {
        let mut held = HeldKeys::default();
        held.pressed("Tab");
        // Tab moved focus off the canvas before its keyup.
        assert!(held.released("Tab", false));
        // Released once: a second keyup elsewhere is the page's.
        assert!(!held.released("Tab", false));
    }

    #[test]
    fn a_keyup_the_app_never_saw_pressed_follows_focus() {
        let mut held = HeldKeys::default();
        assert!(!held.released("KeyA", false));
        assert!(held.released("KeyA", true));
        held.pressed("KeyA");
        held.pressed("KeyA"); // auto-repeat
        assert!(held.released("KeyA", false));
        assert!(!held.released("KeyA", false));
    }

    fn plain() -> Modifiers {
        Modifiers::default()
    }

    fn f(n: u8) -> Key {
        Key::F(n)
    }

    #[test]
    fn f1_is_claimed_for_the_app() {
        assert!(should_prevent_default(&f(1), plain()));
        let shift = Modifiers {
            shift: true,
            ..plain()
        };
        assert!(should_prevent_default(&f(1), shift));
    }

    #[test]
    fn other_f_keys_stay_with_the_browser() {
        for n in [3, 5, 6, 7, 11, 12] {
            assert!(!should_prevent_default(&f(n), plain()), "F{n}");
        }
    }

    #[test]
    fn modified_f1_stays_with_the_browser() {
        let ctrl = Modifiers {
            ctrl: true,
            ..plain()
        };
        let alt = Modifiers {
            alt: true,
            ..plain()
        };
        assert!(!should_prevent_default(&f(1), ctrl));
        assert!(!should_prevent_default(&f(1), alt));
    }

    #[test]
    fn typing_keys_claimed_tab_and_chrome_shortcuts_not() {
        assert!(should_prevent_default(&Key::Char(' '), plain()));
        assert!(should_prevent_default(&Key::ArrowDown, plain()));
        assert!(!should_prevent_default(&Key::Tab, plain()));
        let cmd = Modifiers {
            meta: true,
            ..plain()
        };
        assert!(should_prevent_default(&Key::Char('z'), cmd));
        assert!(!should_prevent_default(&Key::Char('r'), cmd));
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..plain()
        }
    }

    fn cmd_shift() -> Modifiers {
        Modifiers {
            meta: true,
            shift: true,
            ..plain()
        }
    }

    #[test]
    fn save_stays_with_the_browser_until_the_app_claims_it() {
        release_web_shortcut('s');
        assert!(!should_prevent_default(&Key::Char('s'), ctrl()));
        claim_web_shortcut('S');
        // Ctrl+S, Cmd+S and Shift+Cmd+S (the browser reports 'S') all claimed.
        assert!(should_prevent_default(&Key::Char('s'), ctrl()));
        let cmd = Modifiers {
            meta: true,
            ..plain()
        };
        assert!(should_prevent_default(&Key::Char('s'), cmd));
        assert!(should_prevent_default(&Key::Char('S'), cmd_shift()));
        // Never with Alt, and an unmodified 's' is plain typing either way.
        let ctrl_alt = Modifiers {
            alt: true,
            ..ctrl()
        };
        assert!(!should_prevent_default(&Key::Char('s'), ctrl_alt));
        // Other chrome shortcuts are untouched by the claim.
        assert!(!should_prevent_default(&Key::Char('p'), ctrl()));
        assert!(!should_prevent_default(&Key::Char('r'), ctrl()));
        release_web_shortcut('s');
        assert!(!should_prevent_default(&Key::Char('s'), ctrl()));
        assert!(!should_prevent_default(&Key::Char('S'), cmd_shift()));
    }

    #[test]
    fn releasing_a_built_in_key_keeps_the_clipboard_set() {
        claim_web_shortcut('c');
        release_web_shortcut('c');
        assert!(should_prevent_default(&Key::Char('c'), ctrl()));
        assert!(is_web_shortcut_claimed('Z'));
    }
}
