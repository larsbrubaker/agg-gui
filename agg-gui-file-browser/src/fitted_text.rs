//! Text a widget draws itself (with `DrawCtx::fill_text`) kept inside the room it has beside its
//! neighbours: cut back to "..." (agg-gui's `ellipsize_to_width`) and clipped to that room, so a
//! long name can never paint over the icon or button after it. A browser row fits its text when
//! it is given its bounds and shows the full text as its tooltip while it is shortened, so the
//! whole name is still one hover away. [`centered_text_baseline`] is where every such row (and the
//! breadcrumb's links, `link_label.rs`) puts the baseline of a vertically centred line.
//!
//! A file or folder name is fitted with [`fit_name`], which cuts a file's name in the middle of
//! its stem and keeps its extension ("Bad+Batch+Hun….zip"): two names that differ only in their
//! extension (a folder and its archive) still read differently when both are cut. An extension is
//! what follows the last dot when it is 1 to 8 letters and digits with at least one letter, after
//! a stem that is not empty: "3mf", "zip", "mp4" are, while the "5" of "Version 1.5" and the
//! trailing text of "Notes. Read me" are not, so folder names cut at their end as other text
//! does. The cut is marked with one ellipsis character before the extension's dot, since "..."
//! there would run into it. A name whose extension leaves no room to keep at least
//! [`MIN_STEM_CHARS`] of the stem, or that would take more than half the room, is cut at its end
//! instead: the start of a name says more than its extension alone.

use agg_gui::text::Font;
use agg_gui::{ellipsize_to_width, ellipsize_with, measure_text_metrics, DrawCtx};

/// The stem characters a name cut before its extension keeps at least.
pub const MIN_STEM_CHARS: usize = 3;
/// The longest suffix after a name's last dot that counts as a file extension.
const MAX_EXTENSION_CHARS: usize = 8;
/// What marks the cut in a name that keeps its extension.
const CUT_BEFORE_EXTENSION: &str = "\u{2026}";

/// The extension of a file name, with its dot (".zip"), when it has one ([module docs](self)).
pub fn name_extension(name: &str) -> Option<&str> {
    let dot = name.rfind('.')?;
    let (stem, extension) = (&name[..dot], &name[dot + 1..]);
    let chars = extension.chars().count();
    let looks_like_one = (1..=MAX_EXTENSION_CHARS).contains(&chars)
        && extension.chars().all(|c| c.is_ascii_alphanumeric())
        && extension.chars().any(|c| c.is_ascii_alphabetic())
        && !stem.trim_matches('.').is_empty();
    looks_like_one.then_some(&name[dot..])
}

/// `name` cut in its stem to fit `room` keeping its extension, measured by `measure` (a string's
/// advance in `room`'s units): `None` when it fits as it is, is not a file name, or has no room
/// for that ([module docs](self)), for the caller to cut it at its end its own way.
pub fn fit_keeping_extension(
    name: &str,
    room: f64,
    measure: impl Fn(&str) -> f64,
) -> Option<String> {
    if measure(name) <= room {
        return None;
    }
    let extension = name_extension(name)?;
    let stem = &name[..name.len() - extension.len()];
    let tail = format!("{CUT_BEFORE_EXTENSION}{extension}");
    if measure(&tail) > room / 2.0 {
        return None;
    }
    // The longest start of the stem that fits beside the tail, spaces before the cut trimmed (a
    // cut right after a word reads as the word's end).
    let chars: Vec<char> = stem.chars().collect();
    (MIN_STEM_CHARS..chars.len()).rev().find_map(|keep| {
        let start: String = chars[..keep].iter().collect();
        let shown = format!("{}{tail}", start.trim_end_matches(' '));
        (measure(&shown) <= room).then_some(shown)
    })
}

/// `name` as it fits in `room`, measured by `measure`: unchanged when it fits, cut in its stem
/// keeping its extension when it is a file name with room for that, else cut at its end and
/// ending in "..." ([module docs](self)).
pub fn fit_name_with(name: &str, room: f64, measure: impl Fn(&str) -> f64) -> String {
    fit_keeping_extension(name, room, &measure)
        .unwrap_or_else(|| ellipsize_with(name, room.max(0.0), &measure))
}

/// [`fit_name_with`] measuring with `font` at `size` pixels: a file or folder name as it fits in
/// `room` pixels.
pub fn fit_name(font: &Font, name: &str, size: f64, room: f64) -> String {
    fit_name_with(name, room, |s| measure_text_metrics(font, s, size).width)
}

/// The baseline that centres one line of `font` at `size` in a box `height` tall (Y up, from the
/// box's bottom): its one-em line box centred, and the baseline the descent above that box's
/// bottom (agg-gui's centred baseline, `TextMetrics::centered_baseline_y`).
pub fn centered_text_baseline(font: &Font, size: f64, height: f64) -> f64 {
    measure_text_metrics(font, "Ag", size).centered_baseline_y(height)
}

/// `text` as it fits in `room` pixels at `size` in `font`: unchanged when it fits, else cut back
/// and ending in "...". Never wider than `room` unless even "..." is.
pub fn fit_text(font: &Font, text: &str, size: f64, room: f64) -> String {
    ellipsize_to_width(font, text, size, room.max(0.0))
}

/// Draws `shown` (already fitted with [`fit_text`] or [`fit_name`]) at `(x, baseline)` clipped to
/// the room `[x, x + room]` and the widget's height `height`, with the font, size and colour
/// already set on `ctx`. The clip catches what fitting cannot: text so narrow a room holds not
/// even "...".
pub fn fill_text_within(
    ctx: &mut dyn DrawCtx,
    shown: &str,
    x: f64,
    baseline: f64,
    room: f64,
    height: f64,
) {
    ctx.save();
    ctx.clip_rect(x, 0.0, room.max(0.0), height);
    ctx.fill_text(shown, x, baseline);
    ctx.restore();
}
