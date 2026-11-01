//! Tests of how a file or folder name is cut short to fit its row or tile (`fit_name_with`,
//! `fit_keeping_extension`): a file's name is cut in its stem and keeps its extension, so an
//! archive and the same-named folder beside it read apart; names without an extension, with a
//! number after their last dot, or whose extension would crowd out the stem cut at their end.

use agg_gui_file_browser::{fit_keeping_extension, fit_name_with, name_extension};

/// Seven units per character.
fn seven(s: &str) -> f64 {
    s.chars().count() as f64 * 7.0
}

const KNIFE: &str = "Bad+Batch+Hunter+Knife+and+Sheath+";

#[test]
fn an_archive_cut_short_keeps_its_extension_and_its_folder_cuts_at_the_end() {
    let archive = format!("{KNIFE}.zip");
    // 20 characters of room.
    let shown = fit_name_with(&archive, 140.0, seven);
    assert_eq!(shown, "Bad+Batch+Hunte\u{2026}.zip");
    assert!(seven(&shown) <= 140.0);
    let folder = fit_name_with(KNIFE, 140.0, seven);
    assert!(
        folder.ends_with("...") && !folder.contains(".zip"),
        "{folder}"
    );
    assert_ne!(shown, folder);
}

#[test]
fn a_name_that_fits_is_unchanged() {
    assert_eq!(fit_name_with("Part.stl", 140.0, seven), "Part.stl");
    assert_eq!(fit_keeping_extension("Part.stl", 140.0, seven), None);
}

#[test]
fn only_a_short_lettered_suffix_after_a_stem_is_an_extension() {
    assert_eq!(name_extension("Knife.zip"), Some(".zip"));
    assert_eq!(name_extension("Bracket.3mf"), Some(".3mf"));
    assert_eq!(name_extension("Clip.MP4"), Some(".MP4"));
    // Numbers, words with spaces, long tails, dotfiles and dotless names are not.
    assert_eq!(name_extension("Version 1.5"), None);
    assert_eq!(name_extension("Notes. Read me"), None);
    assert_eq!(name_extension("Archive.verylongext"), None);
    assert_eq!(name_extension(".gitignore"), None);
    assert_eq!(name_extension("Brackets"), None);
}

#[test]
fn a_folder_name_with_a_version_number_cuts_at_its_end() {
    let shown = fit_name_with("Brackets for the shelf version 1.5", 140.0, seven);
    assert!(shown.ends_with("..."), "{shown}");
    assert!("Brackets for the shelf version 1.5".starts_with(shown.trim_end_matches("...")));
}

#[test]
fn an_extension_that_would_crowd_out_the_stem_cuts_the_name_at_its_end() {
    // The tail "….stepz" is 7 characters: more than half of 10 characters of room.
    let shown = fit_name_with("Bracket for shelf.stepz", 70.0, seven);
    assert!(shown.ends_with("..."), "{shown}");
    assert_eq!(
        fit_keeping_extension("Bracket for shelf.stepz", 70.0, seven),
        None
    );
}

#[test]
fn spaces_before_the_cut_are_trimmed() {
    // Room for "Shelf " plus the tail: the space before the cut goes.
    let room = seven("Shelf \u{2026}.stl");
    assert_eq!(
        fit_name_with("Shelf bracket left side.stl", room, seven),
        "Shelf\u{2026}.stl"
    );
}

#[test]
fn fit_name_measures_with_the_font_and_keeps_the_extension() {
    let font = agg_gui::text::Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("font");
    let archive = format!("{KNIFE}.zip");
    let room = 120.0;
    let shown = agg_gui_file_browser::fit_name(&font, &archive, 14.0, room);
    assert!(shown.ends_with("\u{2026}.zip"), "{shown}");
    let width = agg_gui::measure_text_metrics(&font, &shown, 14.0).width;
    assert!(width <= room, "{shown} is {width} wide");
    // A name that fits is left alone.
    assert_eq!(
        agg_gui_file_browser::fit_name(&font, "Part.stl", 14.0, room),
        "Part.stl"
    );
}
