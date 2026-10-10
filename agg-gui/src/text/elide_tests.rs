//! Tests for [`super::elide::elide_text`]: the middle and path-aware middle
//! ellipsis modes, and the `End` delegation to agg-sharp's `ellipsize_with`.
//! Most tests measure with a fixed seven pixels per character (the same rule
//! the test `PaintRecorder` uses) so expected strings are exact; one test
//! measures with a real face to show the result fits real glyph advances.

use super::elide::{cluster_bounds, elide_text, EllipsisMode, ELLIPSIS};
use super::{measure_text_metrics, Font};

/// Seven pixels per character.
fn seven(s: &str) -> f64 {
    s.chars().count() as f64 * 7.0
}

/// Seven pixels per grapheme cluster, so combining marks and joined emoji
/// count as one character the way they render.
fn seven_per_cluster(s: &str) -> f64 {
    (cluster_bounds(s).len() - 1) as f64 * 7.0
}

const PATH: &str = "/Users/alex/projects/web/app/node_modules/esm";

#[test]
fn text_that_already_fits_is_unchanged_in_every_mode() {
    for mode in [
        EllipsisMode::End,
        EllipsisMode::Middle,
        EllipsisMode::PathMiddle,
    ] {
        assert_eq!(elide_text(PATH, seven(PATH), mode, seven), PATH);
        assert_eq!(elide_text("", 0.0, mode, seven), "");
    }
}

#[test]
fn middle_result_fits_and_keeps_head_and_tail() {
    let text = "abcdefghijklmnopqrstuvwxyz";
    for width in [7.0, 14.0, 21.0, 35.0, 70.0, 140.0, 175.0] {
        let shown = elide_text(text, width, EllipsisMode::Middle, seven);
        assert!(seven(&shown) <= width, "{shown:?} wider than {width}");
        let (head, tail) = shown.split_once(ELLIPSIS).expect("has ellipsis");
        assert!(text.starts_with(head), "{head:?}");
        assert!(text.ends_with(tail), "{tail:?}");
    }
    // 70 px holds ten characters: the ellipsis plus five head and four tail.
    assert_eq!(
        elide_text(text, 70.0, EllipsisMode::Middle, seven),
        "abcde\u{2026}wxyz"
    );
}

#[test]
fn path_middle_cuts_at_separators_keeping_the_first_segment() {
    let path = "/Users/alex/projects/node_modules/esm";
    // "/Users/alex/…/node_modules/esm" is 30 characters: the tail is grown
    // first ("/projects/node_modules/esm" would not fit), then the head.
    let shown = elide_text(path, 210.0, EllipsisMode::PathMiddle, seven);
    assert_eq!(shown, "/Users/alex/\u{2026}/node_modules/esm");
    // Narrower: the head shrinks to the first segment while
    // "/node_modules/esm" survives.
    let shown = elide_text(path, 175.0, EllipsisMode::PathMiddle, seven);
    assert_eq!(shown, "/Users/\u{2026}/node_modules/esm");
    // Narrower still: only the last segment fits after the first.
    let shown = elide_text(path, 84.0, EllipsisMode::PathMiddle, seven);
    assert_eq!(shown, "/Users/\u{2026}/esm");
}

#[test]
fn path_middle_prefers_more_tail_over_more_head() {
    // 210 px = 30 chars: "/Users/…/app/node_modules/esm" (29) keeps three
    // tail segments rather than "/Users/alex/…/node_modules/esm" (30).
    let shown = elide_text(PATH, 210.0, EllipsisMode::PathMiddle, seven);
    assert_eq!(shown, "/Users/\u{2026}/app/node_modules/esm");
}

#[test]
fn path_middle_handles_backslash_separators() {
    let path = r"C:\Users\alex\AppData\Local\Temp\build.log";
    let shown = elide_text(path, 140.0, EllipsisMode::PathMiddle, seven);
    assert_eq!(shown, "C:\\\u{2026}\\Temp\\build.log");
    assert!(seven(&shown) <= 140.0);
}

#[test]
fn path_middle_falls_back_to_character_middle_when_no_segment_cut_fits() {
    // Even "/Users/…/esm" (12 chars) does not fit 70 px: cut by characters.
    let shown = elide_text(PATH, 70.0, EllipsisMode::PathMiddle, seven);
    assert!(seven(&shown) <= 70.0);
    assert_eq!(shown, "/User\u{2026}/esm");
    // No separators at all: plain middle elision.
    let shown = elide_text("abcdefghijklmnop", 49.0, EllipsisMode::PathMiddle, seven);
    assert_eq!(shown, "abc\u{2026}nop");
}

#[test]
fn a_width_too_small_for_anything_gives_just_the_ellipsis() {
    for mode in [EllipsisMode::Middle, EllipsisMode::PathMiddle] {
        assert_eq!(elide_text(PATH, 10.0, mode, seven), ELLIPSIS);
        assert_eq!(elide_text(PATH, 0.0, mode, seven), ELLIPSIS);
    }
}

#[test]
fn end_mode_keeps_the_agg_sharp_ellipsis() {
    assert_eq!(
        elide_text("Simple Proof of Bevel.mcx!", 70.0, EllipsisMode::End, seven),
        "Simple..."
    );
}

#[test]
fn middle_never_splits_a_character_or_grapheme() {
    // e + combining acute, a family emoji joined by ZWJ, a flag (two regional
    // indicators), and a skin-toned thumbs-up: each is one cluster.
    let text = "e\u{301}e\u{301}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{1F1EF}\u{1F1F5}x\u{1F44D}\u{1F3FD}e\u{301}e\u{301}";
    let clusters = cluster_bounds(text);
    for width in [7.0, 14.0, 21.0, 28.0, 35.0, 42.0, 49.0] {
        let shown = elide_text(text, width, EllipsisMode::Middle, seven_per_cluster);
        assert!(seven_per_cluster(&shown) <= width, "{shown:?} at {width}");
        let (head, tail) = shown.split_once(ELLIPSIS).expect("has ellipsis");
        assert!(
            clusters.contains(&head.len()),
            "head {head:?} ends mid-cluster"
        );
        assert!(
            clusters.contains(&(text.len() - tail.len())),
            "tail {tail:?} starts mid-cluster"
        );
    }
}

#[test]
fn cluster_bounds_group_marks_joiners_flags_and_modifiers() {
    let text =
        "a\u{301}\u{1F468}\u{200D}\u{1F469}\u{1F1EF}\u{1F1F5}\u{1F1FA}\u{1F1F8}\u{1F44D}\u{1F3FD}b";
    let starts: Vec<&str> = cluster_bounds(text)
        .windows(2)
        .map(|w| &text[w[0]..w[1]])
        .collect();
    assert_eq!(
        starts,
        [
            "a\u{301}",
            "\u{1F468}\u{200D}\u{1F469}",
            "\u{1F1EF}\u{1F1F5}",
            "\u{1F1FA}\u{1F1F8}",
            "\u{1F44D}\u{1F3FD}",
            "b",
        ]
    );
}

#[test]
fn middle_fits_real_font_advances() {
    let font =
        Font::from_slice(include_bytes!("../../../demo/assets/CascadiaCode.ttf")).expect("font");
    let measure = |s: &str| measure_text_metrics(&font, s, 14.0).width;
    for mode in [EllipsisMode::Middle, EllipsisMode::PathMiddle] {
        for width in [20.0, 60.0, 120.0, 200.0] {
            let shown = elide_text(PATH, width, mode, measure);
            assert!(measure(&shown) <= width, "{shown:?} at {width}");
        }
    }
}
