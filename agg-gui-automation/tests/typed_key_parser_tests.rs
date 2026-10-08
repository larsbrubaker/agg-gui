//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/TypedKeyParserTests.cs`.
//!
//! What `type_text` turns a string into.  This is the whole reason keyboard
//! shortcuts can be tested at all: everything the automation runner sends a
//! widget tree comes through here, and a string it silently misreads is a
//! test that types garbage and asserts nothing about the shortcut it meant to
//! press.  The `rust_only_*` tests pin the mapping onto agg-gui key events.

use agg_gui::platform::{current_platform, Platform};
use agg_gui::{Key, Modifiers};
use agg_gui_automation::{Keys, TypedKeyParser};

#[test]
fn caret_is_the_control_modifier_on_the_key_after_it() {
    let strokes = TypedKeyParser::parse("^z").unwrap();

    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].key(), Keys::CONTROL | Keys::Z);
    assert!(
        !strokes[0].has_character(),
        "a control chord is not a printable character, so no KeyPress follows it"
    );
}

#[test]
fn control_applies_to_one_key_only() {
    let strokes = TypedKeyParser::parse("^zz").unwrap();

    assert_eq!(strokes.len(), 2);
    assert_eq!(strokes[0].key(), Keys::CONTROL | Keys::Z);
    assert_eq!(strokes[1].key(), Keys::Z);
    assert_eq!(strokes[1].character(), 'z');
}

#[test]
fn control_shift_chords_are_spelled_caret_plus() {
    let strokes = TypedKeyParser::parse("^+z").unwrap();

    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].key(), Keys::CONTROL | Keys::SHIFT | Keys::Z);
}

#[test]
fn a_plus_that_is_not_part_of_a_chord_is_just_a_plus() {
    // Real SendKeys reads a bare + as shift, which would quietly break every
    // test that types an expression into a field. Only a + directly after a
    // ^ is a modifier here.
    let strokes = TypedKeyParser::parse("=40 + 5").unwrap();

    let typed: String = strokes.iter().map(|stroke| stroke.character()).collect();
    assert_eq!(typed, "=40 + 5");
    assert!(!strokes
        .iter()
        .any(|stroke| stroke.key().contains(Keys::SHIFT)));
}

#[test]
fn braced_tokens_become_their_named_key_and_carry_no_character() {
    let strokes = TypedKeyParser::parse("{Enter}").unwrap();

    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].key(), Keys::ENTER);
    assert!(!strokes[0].has_character());
}

#[test]
fn the_spellings_tests_already_use_all_resolve() {
    // C# [Arguments] rows.
    for (text, expected) in [
        ("{Esc}", Keys::ESCAPE),
        ("{ESC}", Keys::ESCAPE),
        ("{BACKSPACE}", Keys::BACK),
        ("{LEFT}", Keys::LEFT),
        ("{Delete}", Keys::DELETE),
        ("{F4}", Keys::F4),
    ] {
        let strokes = TypedKeyParser::parse(text).unwrap();

        assert_eq!(strokes.len(), 1, "{text}");
        assert_eq!(strokes[0].key(), expected, "{text}");
    }
}

#[test]
fn an_unknown_token_is_an_error() {
    // The failure this replaces: an unrecognised token used to be typed out
    // one brace and letter at a time, so the test went green having pressed
    // nothing it meant to press.
    assert!(TypedKeyParser::parse("{NotAKey}").is_err());
}

#[test]
fn plain_text_is_one_stroke_per_character() {
    let strokes = TypedKeyParser::parse("a1.").unwrap();

    assert_eq!(strokes.len(), 3);
    assert_eq!(strokes[0].key(), Keys::A);
    assert_eq!(strokes[0].character(), 'a');
    assert_eq!(strokes[1].key(), Keys::D1);

    // The period has a key code of its own; typing it as (Keys)'.' would
    // land on a key that is not on the keyboard at all.
    assert_eq!(strokes[2].key(), Keys::OEM_PERIOD);
    assert_eq!(strokes[2].character(), '.');
}

#[test]
fn punctuation_never_lands_on_a_navigation_key() {
    // (Keys)'!' is PageUp, (Keys)'$' is Home, (Keys)'(' is Down: every ASCII
    // character from 33 to 47 shares its value with a key a text field acts
    // on, and acting on it swallows the character.
    const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]_`|~";
    let strokes = TypedKeyParser::parse(PUNCTUATION).unwrap();

    let typed: String = strokes.iter().map(|stroke| stroke.character()).collect();
    assert_eq!(typed, PUNCTUATION);
    for stroke in &strokes {
        let key = stroke.key().key_code();
        assert!(
            key >= Keys::OEM1 || (key >= Keys::D0 && key <= Keys::D9),
            "'{}' must be typed on its own key, not on {}",
            stroke.character(),
            key
        );
    }
}

#[test]
fn a_modifier_inside_braces_points_at_the_caret_spelling() {
    // SendKeys has no {Ctrl+a}; the Windows input method would type it
    // literally, so the agg one refuses it too and says what to write
    // instead.
    let error = TypedKeyParser::parse("{Ctrl+a}").unwrap_err();
    assert!(error.message().contains("^a"), "{error}");
}

#[test]
fn modifiers_can_prefix_a_named_key() {
    let strokes = TypedKeyParser::parse("^{Home}").unwrap();

    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].key(), Keys::CONTROL | Keys::HOME);
}

fn command() -> Modifiers {
    match current_platform() {
        Platform::MacOS => Modifiers {
            meta: true,
            ..Modifiers::default()
        },
        _ => Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    }
}

#[test]
fn rust_only_caret_is_the_platform_command_modifier() {
    let strokes = TypedKeyParser::parse("^a^+z").unwrap();

    assert_eq!(strokes[0].agg_key(), Key::Char('a'));
    assert_eq!(strokes[0].agg_modifiers(), command());
    assert_eq!(strokes[1].agg_key(), Key::Char('z'));
    assert_eq!(
        strokes[1].agg_modifiers(),
        Modifiers {
            shift: true,
            ..command()
        }
    );
}

#[test]
fn rust_only_strokes_map_to_the_keys_the_shells_report() {
    let strokes = TypedKeyParser::parse("A!{Enter}{BACKSPACE}{Left}{F4}{Space}").unwrap();
    let keys: Vec<Key> = strokes.iter().map(|s| s.agg_key()).collect();
    assert_eq!(
        keys,
        vec![
            Key::Char('A'),
            Key::Char('!'),
            Key::Enter,
            Key::Backspace,
            Key::ArrowLeft,
            Key::Other("F4".into()),
            Key::Char(' '),
        ]
    );
    assert!(strokes
        .iter()
        .all(|s| s.agg_modifiers() == Modifiers::default()));
}

#[test]
fn rust_only_unterminated_token_names_the_string() {
    let error = TypedKeyParser::parse("ab{Enter").unwrap_err();
    assert_eq!(
        error.message(),
        "'ab{Enter' opens a key token with '{' that is never closed with '}'."
    );
}

#[test]
fn rust_only_tokens_parse_like_enum_try_parse() {
    // Any member name in any case, aliases of the same value, numbers, and
    // comma lists — what C#'s Enum.TryParse<Keys>(ignoreCase) accepts.
    assert_eq!(Keys::try_parse("return"), Some(Keys::ENTER));
    assert_eq!(Keys::try_parse(" pageup "), Some(Keys::PAGE_UP));
    assert_eq!(Keys::try_parse("13"), Some(Keys::ENTER));
    assert_eq!(Keys::try_parse("Shift, A"), Some(Keys::SHIFT | Keys::A));
    assert_eq!(Keys::try_parse("Ctrl+a"), None);
    assert_eq!(Keys::try_parse(""), None);
}
