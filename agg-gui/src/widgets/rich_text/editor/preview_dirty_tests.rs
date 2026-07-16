//! Tests for the live-preview *dirty* flag that drives the colour dialog's
//! click-away dismissal.
//!
//! The dialog's click-away route commits a *changed* preview as one undo step
//! but silently cancels an *untouched* one. [`RichEditCore::is_preview_dirty`]
//! is the flag that tells the two apart; these pin its lifecycle and the undo
//! hygiene of each decision. Kept in a sibling file so `editor/tests.rs` stays
//! under the 800-line guardrail.

use crate::color::Color;
use crate::widgets::rich_text::commands::RichCommand;
use crate::widgets::rich_text::model::{Block, InlineStyle, RichDoc, TextRun};

use super::core::RichEditCore;

fn colored(text: &str, color: Color) -> TextRun {
    TextRun::new(
        text,
        InlineStyle {
            text_color: Some(color),
            ..Default::default()
        },
    )
}

#[test]
fn preview_dirty_tracks_document_mutation() {
    let red = Color::rgb(1.0, 0.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let doc = RichDoc::from_blocks(vec![Block::from_run(colored("hello", red))]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.select_all();

    // A fresh session opens clean.
    core.begin_preview();
    assert!(!core.is_preview_dirty(), "a just-opened preview is not dirty");

    // The first previewing exec (a colour drag) marks it dirty.
    core.exec(&RichCommand::SetTextColor(blue));
    assert!(core.is_preview_dirty(), "a previewing colour exec marks dirty");

    // Committing clears the flag.
    core.commit_preview();
    assert!(!core.is_preview_dirty(), "commit clears the dirty flag");

    // A second session that never mutates stays clean, and cancel keeps it so.
    core.begin_preview();
    assert!(!core.is_preview_dirty(), "reopened session starts clean again");
    core.cancel_preview();
    assert!(!core.is_preview_dirty(), "cancel clears the dirty flag");
}

#[test]
fn dirty_clickaway_commits_one_undo_step() {
    // Mirrors the click-away handler: dirty session -> commit -> one step.
    let red = Color::rgb(1.0, 0.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let doc = RichDoc::from_blocks(vec![Block::from_run(colored("hello", red))]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.select_all();
    core.feed_undo(0.0); // baseline (red)
    assert!(!core.can_undo());

    core.begin_preview();
    core.exec(&RichCommand::SetTextColor(blue));
    core.feed_undo(0.1);

    // Click-away with a changed colour: the handler commits because dirty.
    assert!(core.is_preview_dirty());
    core.commit_preview();
    core.feed_undo(3.0);
    core.feed_undo(4.5);

    assert!(core.can_undo(), "a committed click-away change is undoable");
    assert!(core.undo());
    assert_eq!(core.common_style_of_selection().text_color, Some(Some(red)));
    assert!(!core.can_undo(), "click-away commit is a single undo step");
}

#[test]
fn clean_clickaway_leaves_no_undo_entry() {
    // Mirrors the click-away handler: clean session -> cancel -> no residue.
    let red = Color::rgb(1.0, 0.0, 0.0);
    let doc = RichDoc::from_blocks(vec![Block::from_run(colored("hello", red))]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.select_all();
    core.feed_undo(0.0); // baseline (red)

    // Open then dismiss without touching anything.
    core.begin_preview();
    assert!(!core.is_preview_dirty());
    core.cancel_preview();
    core.feed_undo(3.0);
    core.feed_undo(4.5);

    assert!(!core.can_undo(), "an untouched click-away leaves no undo entry");
    assert_eq!(core.common_style_of_selection().text_color, Some(Some(red)));
}
