//! Long-press alternates: holding a key opens a row of related characters
//! (accented letters, a top-row letter's digit, currency signs, typographic
//! dashes and quotes) above it, as the phone's own keyboard does.
//!
//! Holding a key that has alternates for [`LONG_PRESS_DELAY`] opens the
//! popup with the default alternate highlighted; sliding the finger along
//! the row moves the highlight; lifting types the highlighted character;
//! lifting away from the row types nothing. A key without alternates types
//! once when it lifts, however long it was held (Backspace repeats instead,
//! see `pointer.rs`). The alternates follow the key's case, so a capital
//! `E` offers `Ê É È Ë Ē`.
//!
//! This file holds the table and the popup's geometry and state;
//! `pointer.rs` drives it and `alternates_paint.rs` draws it.

use std::time::Duration;

use crate::geometry::{Point, Rect};

use super::layouts::LayoutFamily;

/// How long a key is held before its alternates open (the phone's long
/// press is about this long).
pub const LONG_PRESS_DELAY: Duration = Duration::from_millis(380);

/// A key's alternates and which one is highlighted when the popup opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alternates {
    /// The characters, left to right.
    pub chars: Vec<char>,
    /// Index of the default (highlighted when the popup opens, and drawn
    /// straight above the key).
    pub default: usize,
}

/// Gboard's alternates for a lower-case letter or a symbol, with the index
/// of its default; `None` for a key with none.
fn base_alternates(c: char) -> Option<(&'static str, usize)> {
    Some(match c {
        'e' => ("êéèëē", 1),
        'a' => ("áàâäãåāæ", 0),
        'o' => ("óòôöõøōœ", 0),
        'u' => ("úùûüū", 0),
        'i' => ("íìîïī", 0),
        'c' => ("ç", 0),
        'n' => ("ñ", 0),
        's' => ("ß", 0),
        'y' => ("ýÿ", 0),
        '$' => ("€£¥₹¢", 0),
        '-' => ("–—", 0),
        '\'' => ("‘’", 0),
        '"' => ("“”„", 0),
        '?' => ("¿", 0),
        '!' => ("¡", 0),
        _ => return None,
    })
}

/// The digit a top-row letter offers on a keyboard with no number row.
fn top_row_digit(c: char) -> Option<char> {
    let i = "qwertyuiop".find(c)?;
    "1234567890".chars().nth(i)
}

/// The alternates for the key that types `c` on a `family` keyboard, in
/// `c`'s case; `None` when it has none.
///
/// Without a number row (iOS) a top-row letter also offers its digit,
/// first in the row; the default stays the accented letter where there is
/// one, so a held `e` still types `é`, and the digit where there isn't.
pub fn alternates_for(c: char, family: LayoutFamily) -> Option<Alternates> {
    let lower = c.to_lowercase().next().unwrap_or(c);
    let upper = c != lower;
    let mut chars: Vec<char> = Vec::new();
    let mut default = 0;
    if !family.has_number_row() {
        if let Some(d) = top_row_digit(lower) {
            chars.push(d);
        }
    }
    if let Some((set, d)) = base_alternates(lower) {
        default = chars.len() + d;
        chars.extend(set.chars().map(|a| if upper { upper_case(a) } else { a }));
    }
    (!chars.is_empty()).then_some(Alternates { chars, default })
}

/// `c` in upper case when that is one character (`ß` stays `ß`, since its
/// capital is the two letters `SS`).
fn upper_case(c: char) -> char {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// An open alternates popup.
#[derive(Debug, Clone)]
pub struct AlternatesPopup {
    /// Index into the painted keys of the held key.
    pub key_index: usize,
    /// The held key's drawn face (viewport, Y-up).
    pub key_rect: Rect,
    /// The alternates, left to right.
    pub chars: Vec<char>,
    /// One cell per alternate (viewport, Y-up), left to right.
    pub cells: Vec<Rect>,
    /// The highlighted alternate: what lifting types. `None` while the
    /// finger is away from the row.
    pub highlighted: Option<usize>,
}

impl AlternatesPopup {
    /// Lay the popup out over the held key `key_rect` (key `key_index`):
    /// cells a little wider than a key pitch, the default straight above
    /// the key, the row kept inside `[left, right]`.
    pub fn open(
        key_index: usize,
        key_rect: Rect,
        alternates: Alternates,
        key_gap: (f64, f64),
        left: f64,
        right: f64,
    ) -> Self {
        let (h_gap, v_gap) = key_gap;
        let n = alternates.chars.len();
        let cell_w = (key_rect.width + h_gap) * 1.1;
        let row_w = cell_w * n as f64;
        let key_cx = key_rect.x + key_rect.width / 2.0;
        let x0 = key_cx - (alternates.default as f64 + 0.5) * cell_w;
        let x0 = x0.min(right - row_w).max(left);
        let y = key_rect.y + key_rect.height + v_gap;
        let cells = (0..n)
            .map(|i| Rect::new(x0 + cell_w * i as f64, y, cell_w, key_rect.height))
            .collect();
        Self {
            key_index,
            key_rect,
            chars: alternates.chars,
            cells,
            highlighted: Some(alternates.default),
        }
    }

    /// The row's bounds (the cells together).
    pub fn row(&self) -> Rect {
        let first = self.cells.first().copied().unwrap_or(self.key_rect);
        let last = self.cells.last().copied().unwrap_or(self.key_rect);
        Rect::new(
            first.x,
            first.y,
            last.x + last.width - first.x,
            first.height,
        )
    }

    /// Move the highlight to the alternate under a finger at `pos`: the
    /// cell its x falls in (the end cells reach half a cell past the row),
    /// while it stays between a key below the held one and a key above the
    /// row. Anywhere else nothing is highlighted, so lifting there cancels.
    pub fn track(&mut self, pos: Point) {
        let row = self.row();
        let cell_w = row.width / self.cells.len().max(1) as f64;
        let low = self.key_rect.y - self.key_rect.height;
        let high = row.y + row.height * 2.0;
        let inside_x = pos.x >= row.x - cell_w * 0.5 && pos.x <= row.x + row.width + cell_w * 0.5;
        self.highlighted = if inside_x && pos.y >= low && pos.y <= high {
            let i = ((pos.x - row.x) / cell_w).floor().max(0.0) as usize;
            Some(i.min(self.cells.len().saturating_sub(1)))
        } else {
            None
        };
    }

    /// The highlighted character, if any.
    pub fn chosen(&self) -> Option<char> {
        self.highlighted.and_then(|i| self.chars.get(i).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(c: char, family: LayoutFamily) -> (String, usize) {
        let a = alternates_for(c, family).expect("has alternates");
        (a.chars.into_iter().collect(), a.default)
    }

    #[test]
    fn gboard_offers_the_accents_with_the_acute_first_highlighted() {
        assert_eq!(set('e', LayoutFamily::Gboard), ("êéèëē".into(), 1));
        assert_eq!(set('a', LayoutFamily::Gboard), ("áàâäãåāæ".into(), 0));
        assert_eq!(set('n', LayoutFamily::Gboard), ("ñ".into(), 0));
        assert_eq!(set('$', LayoutFamily::Gboard), ("€£¥₹¢".into(), 0));
        assert!(alternates_for('q', LayoutFamily::Gboard).is_none());
        assert!(alternates_for('b', LayoutFamily::Gboard).is_none());
    }

    #[test]
    fn without_a_number_row_top_row_letters_offer_their_digit() {
        assert_eq!(set('q', LayoutFamily::Ios), ("1".into(), 0));
        assert_eq!(set('p', LayoutFamily::Ios), ("0".into(), 0));
        // The accent stays the default where there is one.
        assert_eq!(set('e', LayoutFamily::Ios), ("3êéèëē".into(), 2));
        assert!(alternates_for('b', LayoutFamily::Ios).is_none());
    }

    #[test]
    fn alternates_follow_the_keys_case() {
        assert_eq!(set('E', LayoutFamily::Gboard), ("ÊÉÈËĒ".into(), 1));
        assert_eq!(set('S', LayoutFamily::Gboard), ("ß".into(), 0));
        assert_eq!(set('A', LayoutFamily::Gboard).0, "ÁÀÂÄÃÅĀÆ");
    }

    #[test]
    fn the_popup_puts_the_default_over_the_key_and_stays_on_screen() {
        let key = Rect::new(100.0, 50.0, 30.0, 45.0);
        let a = alternates_for('e', LayoutFamily::Gboard).unwrap();
        let p = AlternatesPopup::open(3, key, a.clone(), (6.0, 9.0), 0.0, 400.0);
        let d = p.cells[1];
        assert!((d.x + d.width / 2.0 - 115.0).abs() < 1e-9);
        assert!(d.y >= key.y + key.height);
        // Near the left edge the row slides right to stay on screen.
        let edge = Rect::new(2.0, 50.0, 30.0, 45.0);
        let p = AlternatesPopup::open(0, edge, a, (6.0, 9.0), 0.0, 400.0);
        assert_eq!(p.cells[0].x, 0.0);
    }
}
