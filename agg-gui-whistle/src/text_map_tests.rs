//! Tests for `text_map.rs`: spacing between updates, `continues`, several
//! updates per poll, the tail, the end of a dictation, and lowercasing a
//! dictation that starts mid-sentence.

use super::*;

fn rec(text: &str, continues: bool, pending: &str) -> Recognised {
    Recognised {
        text: text.into(),
        continues,
        pending: pending.into(),
        pending_continues: false,
    }
}

/// `rec` whose tail carries on the last committed word.
fn rec_glued(text: &str, continues: bool, pending: &str) -> Recognised {
    Recognised {
        pending_continues: true,
        ..rec(text, continues, pending)
    }
}

fn delivery(committed: &str, pending: &str) -> Option<Delivery> {
    Some(Delivery {
        committed: committed.into(),
        pending: pending.into(),
    })
}

#[test]
fn the_first_commit_has_no_leading_space_and_later_ones_do() {
    let mut m = TextMapper::new(true);
    m.update(&rec("Hello there", false, ""));
    assert_eq!(m.take(), delivery("Hello there", ""));
    m.update(&rec("general Kenobi", false, ""));
    assert_eq!(m.take(), delivery(" general Kenobi", ""));
}

#[test]
fn a_continuing_update_glues_to_the_last_word() {
    let mut m = TextMapper::new(true);
    m.update(&rec("it costs 2", false, ""));
    m.update(&rec("0 dollars", true, ""));
    assert_eq!(m.take(), delivery("it costs 20 dollars", ""));
}

#[test]
fn several_updates_between_polls_join_in_order_and_the_last_tail_wins() {
    let mut m = TextMapper::new(true);
    m.update(&rec("one", false, "two thre"));
    m.update(&rec("two", false, "three fo"));
    m.update(&rec("", false, "three four"));
    assert_eq!(m.take(), delivery("one two", " three four"));
    assert_eq!(m.take(), None, "nothing changed since");
}

#[test]
fn the_tail_alone_is_delivered_when_it_changes() {
    let mut m = TextMapper::new(true);
    m.update(&rec("", false, "Testing"));
    assert_eq!(m.take(), delivery("", "Testing"));
    m.update(&rec("", false, "Testing"));
    assert_eq!(m.take(), None);
    m.update(&rec("", false, "Testing one"));
    assert_eq!(m.take(), delivery("", "Testing one"));
    // The tail goes away when its words commit.
    m.update(&rec("Testing one", false, ""));
    assert_eq!(m.take(), delivery("Testing one", ""));
}

#[test]
fn the_tail_after_committed_words_starts_with_a_space() {
    let mut m = TextMapper::new(true);
    m.update(&rec("good", false, "morning"));
    assert_eq!(m.take(), delivery("good", " morning"));
}

#[test]
fn a_tail_that_continues_the_last_word_has_no_space_before_it() {
    let mut m = TextMapper::new(true);
    m.update(&rec_glued("2", false, "0 mm"));
    assert_eq!(m.take(), delivery("2", "0 mm"), "previews as 20 mm");
    // Ending the dictation while the tail shows commits it glued on.
    m.finish();
    assert_eq!(m.take(), delivery("0 mm", ""), "20 mm in all");
}

#[test]
fn a_tail_that_continues_only_glues_while_it_is_the_tail() {
    let mut m = TextMapper::new(true);
    m.update(&rec_glued("2", false, "0 mm"));
    assert_eq!(m.take(), delivery("2", "0 mm"));
    // A later update whose tail starts a new word spaces it again.
    m.update(&rec("0", true, "mm wide"));
    assert_eq!(m.take(), delivery("0", " mm wide"));
    m.finish();
    assert_eq!(m.take(), delivery(" mm wide", ""));
}

#[test]
fn a_continuing_tail_with_nothing_committed_is_just_the_tail() {
    let mut m = TextMapper::new(true);
    m.update(&rec_glued("", false, "0 mm"));
    assert_eq!(m.take(), delivery("", "0 mm"));
}

#[test]
fn finishing_commits_the_tail_left_over() {
    let mut m = TextMapper::new(true);
    m.update(&rec("good", false, "morning"));
    assert_eq!(m.take(), delivery("good", " morning"));
    m.update(&rec("", false, "morning all"));
    m.finish();
    assert_eq!(m.take(), delivery(" morning all", ""));
    assert_eq!(m.take(), None);
}

#[test]
fn stray_whitespace_from_the_engine_is_not_doubled() {
    let mut m = TextMapper::new(true);
    m.update(&rec(" one ", false, "  two  "));
    m.update(&rec(" three", false, ""));
    assert_eq!(m.take(), delivery("one three", ""));
}

#[test]
fn mid_sentence_the_first_word_is_lowercased_once() {
    let mut m = TextMapper::new(false);
    m.update(&rec("", false, "Then we"));
    assert_eq!(m.take(), delivery("", "then we"));
    m.update(&rec("Then we went", false, "Home"));
    assert_eq!(m.take(), delivery("then we went", " Home"));
    m.update(&rec("Home", false, ""));
    assert_eq!(m.take(), delivery(" Home", ""), "only the first word");
}

#[test]
fn decapitalising_keeps_i_acronyms_and_inner_capitals() {
    assert_eq!(decapitalise("Then I"), "then I");
    assert_eq!(decapitalise("I think"), "I think");
    assert_eq!(decapitalise("I'm here"), "I'm here");
    assert_eq!(decapitalise("I\u{2019}ll go"), "I\u{2019}ll go");
    assert_eq!(decapitalise("NASA called"), "NASA called");
    assert_eq!(decapitalise("McDonald's"), "McDonald's");
    assert_eq!(decapitalise("iPhone"), "iPhone");
    assert_eq!(decapitalise("Über alles"), "über alles");
    assert_eq!(decapitalise("Ice"), "ice");
    assert_eq!(decapitalise(""), "");
}
