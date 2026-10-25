//! The units touch selection moves by: user-perceived characters (grapheme
//! clusters) and the word a long press picks.
//!
//! agg-gui has no Unicode segmentation tables, so clusters are approximated
//! here from the cases that matter on a phone keyboard: a base character
//! keeps the combining marks, variation selectors, emoji skin-tone
//! modifiers, keycap and tag characters that follow it; a zero-width joiner
//! glues the next character on (family and profession emoji); two regional
//! indicators make one flag; CR LF is one break. A handle therefore never
//! splits 👍🏽, a flag or a ZWJ sequence, and an accent written as a
//! combining mark stays with its letter, in the selection and in the word.
//!
//! Words are runs of letters, digits and `_` (with their marks), as the
//! editors' double click takes them; a run of whitespace is one unit; any
//! other cluster (an emoji, a punctuation mark) is a unit of its own, so a
//! long press on an emoji selects just the emoji. CJK text has no spaces and
//! is not segmented into words: a long press there selects the whole run of
//! ideographs.

/// Whether `c` attaches to the character before it.
fn is_extend(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F // combining diacritical marks
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x0670
        | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4
        | 0x0900..=0x0903
        | 0x093A..=0x094F
        | 0x0951..=0x0957
        | 0x0962..=0x0963
        | 0x0E31
        | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x200C // zero-width non-joiner
        | 0x20D0..=0x20FF // combining marks for symbols, keycap
        | 0xFE00..=0xFE0F // variation selectors
        | 0xFE20..=0xFE2F
        | 0x1F3FB..=0x1F3FF // emoji skin tones
        | 0xE0020..=0xE007F // tags (subdivision flags)
        | 0xE0100..=0xE01EF)
}

fn is_regional_indicator(c: char) -> bool {
    matches!(c as u32, 0x1F1E6..=0x1F1FF)
}

const ZWJ: char = '\u{200D}';

/// Every cluster boundary of `s`, from 0 to `s.len()` inclusive.
pub(crate) fn boundaries(s: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut out = vec![0];
    let mut i = 0;
    while i < n {
        let c = chars[i].1;
        i += 1;
        if (is_regional_indicator(c) && i < n && is_regional_indicator(chars[i].1))
            || (c == '\r' && i < n && chars[i].1 == '\n')
        {
            i += 1;
        }
        while i < n {
            let next = chars[i].1;
            if is_extend(next) {
                i += 1;
            } else if next == ZWJ {
                // The joiner and the character it joins.
                i = (i + 2).min(n);
            } else {
                break;
            }
        }
        out.push(if i < n { chars[i].0 } else { s.len() });
    }
    out
}

/// The cluster boundary at or before `off`.
pub(crate) fn floor(s: &str, off: usize) -> usize {
    boundaries(s)
        .into_iter()
        .take_while(|b| *b <= off)
        .last()
        .unwrap_or(0)
}

/// The cluster boundary at or after `off`.
pub(crate) fn ceil(s: &str, off: usize) -> usize {
    boundaries(s)
        .into_iter()
        .find(|b| *b >= off)
        .unwrap_or(s.len())
}

/// The cluster boundary before `off` (0 at the start).
pub(crate) fn prev(s: &str, off: usize) -> usize {
    boundaries(s)
        .into_iter()
        .take_while(|b| *b < off)
        .last()
        .unwrap_or(0)
}

/// The cluster boundary after `off` (the end at the end).
pub(crate) fn next(s: &str, off: usize) -> usize {
    boundaries(s)
        .into_iter()
        .find(|b| *b > off)
        .unwrap_or(s.len())
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Word,
    Space,
    Other,
}

fn class_of(cluster: &str) -> Class {
    match cluster.chars().next() {
        Some(c) if c.is_alphanumeric() || c == '_' => Class::Word,
        Some(c) if c.is_whitespace() => Class::Space,
        _ => Class::Other,
    }
}

/// The unit a long press on the cluster starting at (or holding) `off`
/// selects: its word, its run of whitespace, or the cluster alone. Empty at
/// the end of the text.
pub(crate) fn word_at(s: &str, off: usize) -> (usize, usize) {
    let b = boundaries(s);
    let Some(i) = b.iter().rposition(|x| *x <= off) else {
        return (off, off);
    };
    if i + 1 >= b.len() {
        return (s.len(), s.len());
    }
    let class = class_of(&s[b[i]..b[i + 1]]);
    if class == Class::Other {
        return (b[i], b[i + 1]);
    }
    let mut start = i;
    while start > 0 && class_of(&s[b[start - 1]..b[start]]) == class {
        start -= 1;
    }
    let mut end = i + 1;
    while end + 1 < b.len() && class_of(&s[b[end]..b[end + 1]]) == class {
        end += 1;
    }
    (b[start], b[end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_sequences_flags_and_marks_are_single_clusters() {
        let thumbs = "👍🏽";
        let family = "👨\u{200D}👩\u{200D}👧";
        let flag = "🇨🇦";
        let accent = "e\u{0301}";
        for cluster in [thumbs, family, flag, accent, "1\u{FE0F}\u{20E3}", "\r\n"] {
            assert_eq!(boundaries(cluster), vec![0, cluster.len()], "{cluster:?}");
        }
        let s = format!("a{thumbs}b{flag}{flag}");
        let a = 1;
        let t = a + thumbs.len();
        let f = t + 1;
        assert_eq!(boundaries(&s), vec![0, a, t, f, f + flag.len(), s.len()]);
        assert_eq!(floor(&s, a + 2), a);
        assert_eq!(ceil(&s, a + 2), t);
        assert_eq!(prev(&s, t), a);
        assert_eq!(next(&s, a), t);
    }

    #[test]
    fn a_word_keeps_its_marks_and_an_emoji_is_its_own_unit() {
        let s = "cafe\u{0301} au lait";
        assert_eq!(word_at(s, 0), (0, 6), "the accent belongs to the word");
        assert_eq!(word_at(s, 5), (0, 6));
        let s = "hi 👍🏽 there";
        let e = 3 + "👍🏽".len();
        assert_eq!(word_at(s, 3), (3, e), "just the emoji");
        assert_eq!(word_at(s, 4), (3, e), "from inside it too");
        assert_eq!(word_at(s, 2), (2, 3), "the space alone");
        assert_eq!(word_at(s, 0), (0, 2));
        assert_eq!(word_at(s, s.len()), (s.len(), s.len()));
    }
}
