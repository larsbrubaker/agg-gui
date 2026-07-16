//! Process-wide **rich clipboard slot** for [`RichTextEdit`](super::RichTextEdit).
//!
//! The system clipboard only carries plain text (see [`crate::clipboard`]), so a
//! styled Copy/Cut would lose every run's colour, size, and weight on paste.
//! This module keeps the last styled fragment in an in-process thread-local slot
//! alongside a **fingerprint** — the exact plain text that Copy wrote to the
//! system clipboard.
//!
//! On paste the editor asks [`matching`] for the stored fragment, passing the
//! plain text it just read back from the system clipboard. The fragment is
//! returned only when the fingerprints match, which means the clipboard has not
//! changed since our Copy (same session, nothing copied elsewhere in between).
//! On any mismatch the caller falls back to inserting the external plain text.
//!
//! The slot is process-wide (thread-local, and the UI runs on one thread on both
//! native and wasm), so a fragment copied from one `RichTextEdit` pastes with
//! full styling into another in the same app.

use std::cell::RefCell;

use super::model::Block;

/// The stored styled fragment plus the plain-text fingerprint that must match
/// the system clipboard for the fragment to be reused.
struct RichPayload {
    fingerprint: String,
    blocks: Vec<Block>,
}

thread_local! {
    static SLOT: RefCell<Option<RichPayload>> = const { RefCell::new(None) };
}

/// Store a styled `fragment` under the plain-text `fingerprint` that Copy/Cut
/// simultaneously wrote to the system clipboard. Replaces any previous payload.
pub fn set(fingerprint: String, fragment: Vec<Block>) {
    SLOT.with(|slot| {
        *slot.borrow_mut() = Some(RichPayload {
            fingerprint,
            blocks: fragment,
        });
    });
}

/// Return a clone of the stored fragment when its fingerprint equals `plain`
/// (the text just read from the system clipboard); otherwise `None`.
///
/// Cloning (not taking) lets the same Copy be pasted repeatedly.
pub fn matching(plain: &str) -> Option<Vec<Block>> {
    SLOT.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|p| p.fingerprint == plain)
            .map(|p| p.blocks.clone())
    })
}

/// Drop any stored payload. Test-only hook so a fingerprint-mismatch case can't
/// be masked by a fragment left over from an earlier test on the same thread.
#[cfg(test)]
pub fn clear() {
    SLOT.with(|slot| *slot.borrow_mut() = None);
}
