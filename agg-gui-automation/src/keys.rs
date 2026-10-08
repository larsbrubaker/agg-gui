//! `Keys` — agg-sharp's key codes (`Gui/Keys.cs`, the Windows virtual-key
//! values), which typed strings parse into.
//!
//! C# keeps a key and its modifier bits in one `Keys` value (`Keys.Control |
//! Keys.Z`), and `TypedKeyParser` resolves brace tokens with
//! `Enum.TryParse<Keys>`; both are reproduced here so the parser and its
//! tests port 1:1.  [`Keys::to_agg_key`] and [`Keys::to_agg_modifiers`] turn a
//! stroke into the [`agg_gui::Key`] / [`agg_gui::Modifiers`] the `App` takes;
//! they live in the neighbouring `key_mapping.rs`.

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign};

/// A key code with any modifier bits or'd in (C# `MatterHackers.Agg.UI.Keys`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Keys(pub i32);

impl Keys {
    pub const MODIFIERS: Keys = Keys(-65536);
    pub const NONE: Keys = Keys(0);
    pub const L_BUTTON: Keys = Keys(1);
    pub const R_BUTTON: Keys = Keys(2);
    pub const CANCEL: Keys = Keys(3);
    pub const M_BUTTON: Keys = Keys(4);
    pub const X_BUTTON1: Keys = Keys(5);
    pub const X_BUTTON2: Keys = Keys(6);
    pub const BACK: Keys = Keys(8);
    pub const TAB: Keys = Keys(9);
    pub const LINE_FEED: Keys = Keys(10);
    pub const CLEAR: Keys = Keys(12);
    pub const ENTER: Keys = Keys(13);
    pub const RETURN: Keys = Keys(13);
    pub const SHIFT_KEY: Keys = Keys(16);
    pub const CONTROL_KEY: Keys = Keys(17);
    pub const MENU: Keys = Keys(18);
    pub const PAUSE: Keys = Keys(19);
    pub const CAPS_LOCK: Keys = Keys(20);
    pub const CAPITAL: Keys = Keys(20);
    pub const KANA_MODE: Keys = Keys(21);
    pub const HANGUEL_MODE: Keys = Keys(21);
    pub const HANGUL_MODE: Keys = Keys(21);
    pub const JUNJA_MODE: Keys = Keys(23);
    pub const FINAL_MODE: Keys = Keys(24);
    pub const KANJI_MODE: Keys = Keys(25);
    pub const HANJA_MODE: Keys = Keys(25);
    pub const ESCAPE: Keys = Keys(27);
    pub const IME_CONVERT: Keys = Keys(28);
    pub const IME_NONCONVERT: Keys = Keys(29);
    pub const IME_ACEEPT: Keys = Keys(30);
    pub const IME_ACCEPT: Keys = Keys(30);
    pub const IME_MODE_CHANGE: Keys = Keys(31);
    pub const SPACE: Keys = Keys(32);
    pub const PRIOR: Keys = Keys(33);
    pub const PAGE_UP: Keys = Keys(33);
    pub const NEXT: Keys = Keys(34);
    pub const PAGE_DOWN: Keys = Keys(34);
    pub const END: Keys = Keys(35);
    pub const HOME: Keys = Keys(36);
    pub const LEFT: Keys = Keys(37);
    pub const UP: Keys = Keys(38);
    pub const RIGHT: Keys = Keys(39);
    pub const DOWN: Keys = Keys(40);
    pub const SELECT: Keys = Keys(41);
    pub const PRINT: Keys = Keys(42);
    pub const EXECUTE: Keys = Keys(43);
    pub const PRINT_SCREEN: Keys = Keys(44);
    pub const SNAPSHOT: Keys = Keys(44);
    pub const INSERT: Keys = Keys(45);
    pub const DELETE: Keys = Keys(46);
    pub const HELP: Keys = Keys(47);
    pub const D0: Keys = Keys(48);
    pub const D1: Keys = Keys(49);
    pub const D2: Keys = Keys(50);
    pub const D3: Keys = Keys(51);
    pub const D4: Keys = Keys(52);
    pub const D5: Keys = Keys(53);
    pub const D6: Keys = Keys(54);
    pub const D7: Keys = Keys(55);
    pub const D8: Keys = Keys(56);
    pub const D9: Keys = Keys(57);
    pub const A: Keys = Keys(65);
    pub const B: Keys = Keys(66);
    pub const C: Keys = Keys(67);
    pub const D: Keys = Keys(68);
    pub const E: Keys = Keys(69);
    pub const F: Keys = Keys(70);
    pub const G: Keys = Keys(71);
    pub const H: Keys = Keys(72);
    pub const I: Keys = Keys(73);
    pub const J: Keys = Keys(74);
    pub const K: Keys = Keys(75);
    pub const L: Keys = Keys(76);
    pub const M: Keys = Keys(77);
    pub const N: Keys = Keys(78);
    pub const O: Keys = Keys(79);
    pub const P: Keys = Keys(80);
    pub const Q: Keys = Keys(81);
    pub const R: Keys = Keys(82);
    pub const S: Keys = Keys(83);
    pub const T: Keys = Keys(84);
    pub const U: Keys = Keys(85);
    pub const V: Keys = Keys(86);
    pub const W: Keys = Keys(87);
    pub const X: Keys = Keys(88);
    pub const Y: Keys = Keys(89);
    pub const Z: Keys = Keys(90);
    pub const L_WIN: Keys = Keys(91);
    pub const R_WIN: Keys = Keys(92);
    pub const APPS: Keys = Keys(93);
    pub const SLEEP: Keys = Keys(95);
    pub const NUM_PAD0: Keys = Keys(96);
    pub const NUM_PAD1: Keys = Keys(97);
    pub const NUM_PAD2: Keys = Keys(98);
    pub const NUM_PAD3: Keys = Keys(99);
    pub const NUM_PAD4: Keys = Keys(100);
    pub const NUM_PAD5: Keys = Keys(101);
    pub const NUM_PAD6: Keys = Keys(102);
    pub const NUM_PAD7: Keys = Keys(103);
    pub const NUM_PAD8: Keys = Keys(104);
    pub const NUM_PAD9: Keys = Keys(105);
    pub const MULTIPLY: Keys = Keys(106);
    pub const ADD: Keys = Keys(107);
    pub const SEPARATOR: Keys = Keys(108);
    pub const SUBTRACT: Keys = Keys(109);
    pub const DECIMAL: Keys = Keys(110);
    pub const DIVIDE: Keys = Keys(111);
    pub const F1: Keys = Keys(112);
    pub const F2: Keys = Keys(113);
    pub const F3: Keys = Keys(114);
    pub const F4: Keys = Keys(115);
    pub const F5: Keys = Keys(116);
    pub const F6: Keys = Keys(117);
    pub const F7: Keys = Keys(118);
    pub const F8: Keys = Keys(119);
    pub const F9: Keys = Keys(120);
    pub const F10: Keys = Keys(121);
    pub const F11: Keys = Keys(122);
    pub const F12: Keys = Keys(123);
    pub const F13: Keys = Keys(124);
    pub const F14: Keys = Keys(125);
    pub const F15: Keys = Keys(126);
    pub const F16: Keys = Keys(127);
    pub const F17: Keys = Keys(128);
    pub const F18: Keys = Keys(129);
    pub const F19: Keys = Keys(130);
    pub const F20: Keys = Keys(131);
    pub const F21: Keys = Keys(132);
    pub const F22: Keys = Keys(133);
    pub const F23: Keys = Keys(134);
    pub const F24: Keys = Keys(135);
    pub const NUM_LOCK: Keys = Keys(144);
    pub const SCROLL: Keys = Keys(145);
    pub const L_SHIFT_KEY: Keys = Keys(160);
    pub const R_SHIFT_KEY: Keys = Keys(161);
    pub const L_CONTROL_KEY: Keys = Keys(162);
    pub const R_CONTROL_KEY: Keys = Keys(163);
    pub const L_MENU: Keys = Keys(164);
    pub const R_MENU: Keys = Keys(165);
    pub const BROWSER_BACK: Keys = Keys(166);
    pub const BROWSER_FORWARD: Keys = Keys(167);
    pub const BROWSER_REFRESH: Keys = Keys(168);
    pub const BROWSER_STOP: Keys = Keys(169);
    pub const BROWSER_SEARCH: Keys = Keys(170);
    pub const BROWSER_FAVORITES: Keys = Keys(171);
    pub const BROWSER_HOME: Keys = Keys(172);
    pub const VOLUME_MUTE: Keys = Keys(173);
    pub const VOLUME_DOWN: Keys = Keys(174);
    pub const VOLUME_UP: Keys = Keys(175);
    pub const MEDIA_NEXT_TRACK: Keys = Keys(176);
    pub const MEDIA_PREVIOUS_TRACK: Keys = Keys(177);
    pub const MEDIA_STOP: Keys = Keys(178);
    pub const MEDIA_PLAY_PAUSE: Keys = Keys(179);
    pub const LAUNCH_MAIL: Keys = Keys(180);
    pub const SELECT_MEDIA: Keys = Keys(181);
    pub const LAUNCH_APPLICATION1: Keys = Keys(182);
    pub const LAUNCH_APPLICATION2: Keys = Keys(183);
    pub const OEM1: Keys = Keys(186);
    pub const OEM_SEMICOLON: Keys = Keys(186);
    pub const OEMPLUS: Keys = Keys(187);
    pub const OEMCOMMA: Keys = Keys(188);
    pub const OEM_MINUS: Keys = Keys(189);
    pub const OEM_PERIOD: Keys = Keys(190);
    pub const OEM_QUESTION: Keys = Keys(191);
    pub const OEM2: Keys = Keys(191);
    pub const OEMTILDE: Keys = Keys(192);
    pub const OEM3: Keys = Keys(192);
    pub const OEM4: Keys = Keys(219);
    pub const OEM_OPEN_BRACKETS: Keys = Keys(219);
    pub const OEM_PIPE: Keys = Keys(220);
    pub const OEM5: Keys = Keys(220);
    pub const OEM6: Keys = Keys(221);
    pub const OEM_CLOSE_BRACKETS: Keys = Keys(221);
    pub const OEM7: Keys = Keys(222);
    pub const OEM_QUOTES: Keys = Keys(222);
    pub const OEM8: Keys = Keys(223);
    pub const OEM102: Keys = Keys(226);
    pub const OEM_BACKSLASH: Keys = Keys(226);
    pub const PROCESS_KEY: Keys = Keys(229);
    pub const PACKET: Keys = Keys(231);
    pub const ATTN: Keys = Keys(246);
    pub const CRSEL: Keys = Keys(247);
    pub const EXSEL: Keys = Keys(248);
    pub const ERASE_EOF: Keys = Keys(249);
    pub const PLAY: Keys = Keys(250);
    pub const ZOOM: Keys = Keys(251);
    pub const NO_NAME: Keys = Keys(252);
    pub const PA1: Keys = Keys(253);
    pub const OEM_CLEAR: Keys = Keys(254);
    pub const KEY_CODE: Keys = Keys(65535);
    pub const SHIFT: Keys = Keys(65536);
    pub const CONTROL: Keys = Keys(131072);
    pub const ALT: Keys = Keys(262144);
}

/// Every C# member name with its value, in declaration order (aliases
/// included, e.g. `Return` = `Enter`), for `Enum.TryParse`-style lookup.
const NAMES: &[(&str, i32)] = &[
    ("Modifiers", -65536),
    ("None", 0),
    ("LButton", 1),
    ("RButton", 2),
    ("Cancel", 3),
    ("MButton", 4),
    ("XButton1", 5),
    ("XButton2", 6),
    ("Back", 8),
    ("Tab", 9),
    ("LineFeed", 10),
    ("Clear", 12),
    ("Enter", 13),
    ("Return", 13),
    ("ShiftKey", 16),
    ("ControlKey", 17),
    ("Menu", 18),
    ("Pause", 19),
    ("CapsLock", 20),
    ("Capital", 20),
    ("KanaMode", 21),
    ("HanguelMode", 21),
    ("HangulMode", 21),
    ("JunjaMode", 23),
    ("FinalMode", 24),
    ("KanjiMode", 25),
    ("HanjaMode", 25),
    ("Escape", 27),
    ("IMEConvert", 28),
    ("IMENonconvert", 29),
    ("IMEAceept", 30),
    ("IMEAccept", 30),
    ("IMEModeChange", 31),
    ("Space", 32),
    ("Prior", 33),
    ("PageUp", 33),
    ("Next", 34),
    ("PageDown", 34),
    ("End", 35),
    ("Home", 36),
    ("Left", 37),
    ("Up", 38),
    ("Right", 39),
    ("Down", 40),
    ("Select", 41),
    ("Print", 42),
    ("Execute", 43),
    ("PrintScreen", 44),
    ("Snapshot", 44),
    ("Insert", 45),
    ("Delete", 46),
    ("Help", 47),
    ("D0", 48),
    ("D1", 49),
    ("D2", 50),
    ("D3", 51),
    ("D4", 52),
    ("D5", 53),
    ("D6", 54),
    ("D7", 55),
    ("D8", 56),
    ("D9", 57),
    ("A", 65),
    ("B", 66),
    ("C", 67),
    ("D", 68),
    ("E", 69),
    ("F", 70),
    ("G", 71),
    ("H", 72),
    ("I", 73),
    ("J", 74),
    ("K", 75),
    ("L", 76),
    ("M", 77),
    ("N", 78),
    ("O", 79),
    ("P", 80),
    ("Q", 81),
    ("R", 82),
    ("S", 83),
    ("T", 84),
    ("U", 85),
    ("V", 86),
    ("W", 87),
    ("X", 88),
    ("Y", 89),
    ("Z", 90),
    ("LWin", 91),
    ("RWin", 92),
    ("Apps", 93),
    ("Sleep", 95),
    ("NumPad0", 96),
    ("NumPad1", 97),
    ("NumPad2", 98),
    ("NumPad3", 99),
    ("NumPad4", 100),
    ("NumPad5", 101),
    ("NumPad6", 102),
    ("NumPad7", 103),
    ("NumPad8", 104),
    ("NumPad9", 105),
    ("Multiply", 106),
    ("Add", 107),
    ("Separator", 108),
    ("Subtract", 109),
    ("Decimal", 110),
    ("Divide", 111),
    ("F1", 112),
    ("F2", 113),
    ("F3", 114),
    ("F4", 115),
    ("F5", 116),
    ("F6", 117),
    ("F7", 118),
    ("F8", 119),
    ("F9", 120),
    ("F10", 121),
    ("F11", 122),
    ("F12", 123),
    ("F13", 124),
    ("F14", 125),
    ("F15", 126),
    ("F16", 127),
    ("F17", 128),
    ("F18", 129),
    ("F19", 130),
    ("F20", 131),
    ("F21", 132),
    ("F22", 133),
    ("F23", 134),
    ("F24", 135),
    ("NumLock", 144),
    ("Scroll", 145),
    ("LShiftKey", 160),
    ("RShiftKey", 161),
    ("LControlKey", 162),
    ("RControlKey", 163),
    ("LMenu", 164),
    ("RMenu", 165),
    ("BrowserBack", 166),
    ("BrowserForward", 167),
    ("BrowserRefresh", 168),
    ("BrowserStop", 169),
    ("BrowserSearch", 170),
    ("BrowserFavorites", 171),
    ("BrowserHome", 172),
    ("VolumeMute", 173),
    ("VolumeDown", 174),
    ("VolumeUp", 175),
    ("MediaNextTrack", 176),
    ("MediaPreviousTrack", 177),
    ("MediaStop", 178),
    ("MediaPlayPause", 179),
    ("LaunchMail", 180),
    ("SelectMedia", 181),
    ("LaunchApplication1", 182),
    ("LaunchApplication2", 183),
    ("Oem1", 186),
    ("OemSemicolon", 186),
    ("Oemplus", 187),
    ("Oemcomma", 188),
    ("OemMinus", 189),
    ("OemPeriod", 190),
    ("OemQuestion", 191),
    ("Oem2", 191),
    ("Oemtilde", 192),
    ("Oem3", 192),
    ("Oem4", 219),
    ("OemOpenBrackets", 219),
    ("OemPipe", 220),
    ("Oem5", 220),
    ("Oem6", 221),
    ("OemCloseBrackets", 221),
    ("Oem7", 222),
    ("OemQuotes", 222),
    ("Oem8", 223),
    ("Oem102", 226),
    ("OemBackslash", 226),
    ("ProcessKey", 229),
    ("Packet", 231),
    ("Attn", 246),
    ("Crsel", 247),
    ("Exsel", 248),
    ("EraseEof", 249),
    ("Play", 250),
    ("Zoom", 251),
    ("NoName", 252),
    ("Pa1", 253),
    ("OemClear", 254),
    ("KeyCode", 65535),
    ("Shift", 65536),
    ("Control", 131072),
    ("Alt", 262144),
];

impl Keys {
    /// The key code without modifier bits (C# `key & Keys.KeyCode`).
    pub fn key_code(self) -> Keys {
        self & Keys::KEY_CODE
    }

    /// The modifier bits alone (C# `key & Keys.Modifiers`).
    pub fn modifiers(self) -> Keys {
        self & Keys::MODIFIERS
    }

    /// Whether every bit of `flags` is set.
    pub fn contains(self, flags: Keys) -> bool {
        (self.0 & flags.0) == flags.0
    }

    /// The first C# member name with exactly this value, if any.
    pub fn name(self) -> Option<&'static str> {
        NAMES.iter().find(|(_, v)| *v == self.0).map(|(n, _)| *n)
    }

    /// `Enum.TryParse<Keys>(text, ignoreCase: true)`: surrounding white space
    /// is ignored; a leading digit, `+` or `-` reads an integer value;
    /// otherwise a comma-separated list of member names (any case) is or'd
    /// together.  `None` when any part names no member.
    pub fn try_parse(text: &str) -> Option<Keys> {
        let text = text.trim();
        let first = text.chars().next()?;
        if first.is_ascii_digit() || first == '-' || first == '+' {
            return text.parse::<i32>().ok().map(Keys);
        }
        let mut value = 0;
        for part in text.split(',') {
            let part = part.trim();
            let (_, v) = NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(part))?;
            value |= *v;
        }
        Some(Keys(value))
    }
}

impl BitOr for Keys {
    type Output = Keys;
    fn bitor(self, rhs: Keys) -> Keys {
        Keys(self.0 | rhs.0)
    }
}

impl BitOrAssign for Keys {
    fn bitor_assign(&mut self, rhs: Keys) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Keys {
    type Output = Keys;
    fn bitand(self, rhs: Keys) -> Keys {
        Keys(self.0 & rhs.0)
    }
}

impl PartialOrd for Keys {
    fn partial_cmp(&self, other: &Keys) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Keys {
    fn cmp(&self, other: &Keys) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

/// Shows the member name when the value has one, otherwise the modifier
/// names joined to the key's name (`Control+Z`), otherwise the number.
impl fmt::Display for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = self.name() {
            return f.write_str(name);
        }
        let mut parts = Vec::new();
        for (flag, label) in [
            (Keys::CONTROL, "Control"),
            (Keys::SHIFT, "Shift"),
            (Keys::ALT, "Alt"),
        ] {
            if self.contains(flag) {
                parts.push(label.to_string());
            }
        }
        let code = self.key_code();
        match code.name() {
            Some(name) if !parts.is_empty() => parts.push(name.to_string()),
            _ => return write!(f, "{}", self.0),
        }
        f.write_str(&parts.join("+"))
    }
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Keys({self})")
    }
}
