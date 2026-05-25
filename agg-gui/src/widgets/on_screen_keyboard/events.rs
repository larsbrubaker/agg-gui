//! Thread-local queue of synthetic `(Key, Modifiers)` events the
//! keyboard has produced but the App has not yet dispatched.
//!
//! Follows the same pattern as `widgets::combo_box::popup_paint`'s
//! popup queue: producers push, the App drains. This decouples the key
//! paint / hit-test logic (no `App` reference required) from the actual
//! event dispatch (which lives on `App` because it walks the focus
//! path).

use std::cell::RefCell;

use crate::event::{Key, Modifiers};

thread_local! {
    static QUEUE: RefCell<Vec<(Key, Modifiers)>> = RefCell::new(Vec::new());
}

/// Enqueue a synthetic key event. Called by the keyboard module when a
/// key tap commits.
pub fn push_synthetic_key(key: Key, modifiers: Modifiers) {
    QUEUE.with(|q| q.borrow_mut().push((key, modifiers)));
}

/// Drain every pending event. Called by [`App`](crate::widget::App)
/// after each pointer event so synthesized keys land on the focused
/// widget in the same frame as the tap that produced them.
pub fn drain_synthetic_keys() -> Vec<(Key, Modifiers)> {
    QUEUE.with(|q| q.borrow_mut().drain(..).collect())
}

/// Convenience for tests / inspection — does not consume the queue.
#[cfg(test)]
pub fn peek_pending_count() -> usize {
    QUEUE.with(|q| q.borrow().len())
}

#[cfg(test)]
pub fn clear() {
    QUEUE.with(|q| q.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_and_drain() {
        clear();
        push_synthetic_key(Key::Char('a'), Modifiers::default());
        push_synthetic_key(Key::Backspace, Modifiers::default());
        assert_eq!(peek_pending_count(), 2);
        let drained = drain_synthetic_keys();
        assert_eq!(drained.len(), 2);
        assert!(matches!(drained[0].0, Key::Char('a')));
        assert!(matches!(drained[1].0, Key::Backspace));
        // Drained again: empty.
        assert_eq!(drain_synthetic_keys().len(), 0);
    }
}
