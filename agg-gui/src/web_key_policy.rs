//! Which browser keydowns the web keyboard bridge (`web_adapter`'s
//! `install_keyboard_listeners`) `preventDefault()`s after forwarding them
//! to the app. Pure logic with no DOM types, so it compiles and is tested
//! on every target; only the wasm `web_adapter` calls it.

use crate::event::{Key, Modifiers};

/// Whether a forwarded keydown should suppress the browser's default
/// action.  The listeners sit at window level, so this must block only
/// page side effects of in-app typing (space scrolls, arrows scroll,
/// `'` / `/` open Firefox quick-find, Backspace navigates back) while
/// leaving browser chrome usable: `Tab` focus-navigation, modified
/// shortcuts like Ctrl+R / Cmd+L, and the F-keys stay with the browser.
/// Ctrl/Cmd C, X, A, Z, Y are the app's clipboard/undo set and are
/// claimed.  Alt combos (Alt+Left = history back, AltGr chars report
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
        return matches!(k, Key::Char(c)
            if matches!(c.to_ascii_lowercase(), 'c' | 'x' | 'a' | 'z' | 'y'))
            && !mods.alt;
    }
    if mods.alt {
        return false;
    }
    match k {
        Key::Other(name) => name == "F1",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Modifiers {
        Modifiers::default()
    }

    fn other(name: &str) -> Key {
        Key::Other(name.to_string())
    }

    #[test]
    fn f1_is_claimed_for_the_app() {
        assert!(should_prevent_default(&other("F1"), plain()));
        let shift = Modifiers {
            shift: true,
            ..plain()
        };
        assert!(should_prevent_default(&other("F1"), shift));
    }

    #[test]
    fn other_f_keys_stay_with_the_browser() {
        for name in ["F3", "F5", "F6", "F7", "F11", "F12"] {
            assert!(!should_prevent_default(&other(name), plain()), "{name}");
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
        assert!(!should_prevent_default(&other("F1"), ctrl));
        assert!(!should_prevent_default(&other("F1"), alt));
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
}
