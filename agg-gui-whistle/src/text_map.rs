//! Turns the engine's stream updates into what the on-screen keyboard
//! inserts: committed deltas, verbatim, and the whole pending tail.
//!
//! The engine (`engine.rs`, whistle's `Stream`) reports each pass as the
//! words it committed (`text`, with `continues` when they carry on the last
//! committed word) and the unconfirmed tail (`pending`). The keyboard puts a
//! space before a dictation's first insertion itself; between the
//! provider's own updates the spacing is this module's: one space before
//! each new run of committed words, none when it `continues`, and one before
//! the tail once anything is committed. Several updates between two polls
//! accumulate: the committed runs join in order and the last tail wins.
//!
//! When the dictation does not start a sentence (the caret follows text
//! mid-sentence), the first word of the dictation's first text is
//! lowercased, by a simple rule ([`decapitalise`]). `machine.rs` owns one
//! mapper per dictation.

use crate::protocol::Recognised;

/// What a poll hands the keyboard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Delivery {
    /// New final text, to insert as given.
    pub committed: String,
    /// The whole unconfirmed tail (replaces the one shown).
    pub pending: String,
}

/// One dictation's text.
#[derive(Debug, Default)]
pub(crate) struct TextMapper {
    sentence_start: bool,
    /// Anything committed in this dictation (delivered or not).
    any_committed: bool,
    /// Committed since the last take.
    committed: String,
    /// The tail as the keyboard should show it.
    pending: String,
    /// The tail the last take delivered.
    delivered_pending: String,
}

/// The first word of `text`, lowercased unless it should keep its capital:
/// "I" and its contractions ("I'm"), and words with a capital past the
/// first letter ("NASA", "McDonald", "iPhone"). The rule cannot tell a
/// plain proper noun ("Paris") from a sentence's first word, so those are
/// lowercased too.
pub(crate) fn decapitalise(text: &str) -> String {
    let word_end = text.find(char::is_whitespace).unwrap_or(text.len());
    let word = &text[..word_end];
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return text.to_string();
    };
    let rest = chars.as_str();
    let pronoun_i = first == 'I' && (rest.is_empty() || rest.starts_with(['\'', '\u{2019}']));
    if pronoun_i || rest.chars().any(char::is_uppercase) || !first.is_uppercase() {
        return text.to_string();
    }
    let mut out: String = first.to_lowercase().collect();
    out.push_str(rest);
    out.push_str(&text[word_end..]);
    out
}

impl TextMapper {
    /// A mapper for a dictation; `sentence_start` is whether the caret
    /// starts a sentence (the keyboard's `VoiceContext`).
    pub fn new(sentence_start: bool) -> TextMapper {
        TextMapper {
            sentence_start,
            ..TextMapper::default()
        }
    }

    /// `text` as the dictation's first text, if it is.
    fn shaped_first(&self, text: &str) -> String {
        if self.any_committed || self.sentence_start {
            text.to_string()
        } else {
            decapitalise(text)
        }
    }

    /// Append committed words; `continues` glues them to the last word.
    fn commit(&mut self, text: &str, continues: bool) {
        let text = if continues {
            text.trim_end()
        } else {
            text.trim()
        };
        if text.is_empty() {
            return;
        }
        if self.any_committed && !continues {
            self.committed.push(' ');
        }
        let shaped = self.shaped_first(text);
        self.committed.push_str(&shaped);
        self.any_committed = true;
    }

    /// One engine update.
    pub fn update(&mut self, update: &Recognised) {
        self.commit(&update.text, update.continues);
        let tail = update.pending.trim();
        self.pending = if tail.is_empty() {
            String::new()
        } else if self.any_committed {
            format!(" {tail}")
        } else {
            self.shaped_first(tail)
        };
    }

    /// The dictation is over: whatever tail is left becomes final.
    pub fn finish(&mut self) {
        let tail = std::mem::take(&mut self.pending);
        self.commit(&tail, false);
    }

    /// What changed since the last take; `None` when nothing did.
    pub fn take(&mut self) -> Option<Delivery> {
        if self.committed.is_empty() && self.pending == self.delivered_pending {
            return None;
        }
        self.delivered_pending = self.pending.clone();
        Some(Delivery {
            committed: std::mem::take(&mut self.committed),
            pending: self.pending.clone(),
        })
    }
}

#[cfg(test)]
#[path = "text_map_tests.rs"]
mod tests;
