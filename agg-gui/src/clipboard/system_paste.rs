//! Paste from the system clipboard where it can only be read asynchronously:
//! in a browser, `navigator.clipboard.readText()`.
//!
//! A native build reads the clipboard on the spot ([`super::get_text`]), so
//! a Paste button just pastes. In a browser the page's own clipboard
//! ([`crate::wasm_clipboard`]) holds only what the app copied or what a
//! browser `paste` event carried, so the touch selection toolbar's Paste
//! (`widgets/touch_selection/gestures.rs`) would offer nothing a phone user
//! copied in another app. There the button asks for the system clipboard
//! instead ([`request`]), inside the tap that pressed it (the browser lets a
//! page read the clipboard only in a user gesture, and may ask the user);
//! when the promise resolves the text waits here until the next frame, where
//! `App` hands it to the focused widget as a paste ([`take_ready`]): the
//! text goes into the page's clipboard and the widget gets `Ctrl+V`, the
//! same delivery a browser `paste` event gets (`web_paste`).
//!
//! Only the newest request counts, and only if no other press happened
//! while it was pending (the user moved on). A refused or empty read pastes
//! nothing and is logged to the console once.
//!
//! Native tests stand in for the browser with [`simulate`], which makes
//! reads asynchronous and lets the test resolve them.

use std::cell::{Cell, RefCell};

/// A read in flight or resolved: the request's number and the press epoch
/// it was made in.
#[derive(Clone, Debug, Default)]
struct State {
    /// Number of the newest request.
    newest: u64,
    /// The press epoch of the newest request.
    epoch: u64,
    /// Text that arrived for the newest request, not yet delivered.
    ready: Option<String>,
    /// Requests made under [`simulate`], resolved by [`resolve_simulated`].
    #[cfg(test)]
    simulated: Option<Vec<u64>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static WARNED: Cell<bool> = const { Cell::new(false) };
}

/// Whether Paste must read the system clipboard asynchronously (in a
/// browser), so the toolbar always offers it in an editable field.
pub(crate) fn reads_asynchronously() -> bool {
    #[cfg(test)]
    if STATE.with(|s| s.borrow().simulated.is_some()) {
        return true;
    }
    cfg!(target_arch = "wasm32")
}

/// Start reading the system clipboard for a paste into the focused widget.
/// Call it while handling the tap that pressed Paste.
pub(crate) fn request() {
    let id = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.newest += 1;
        s.epoch = crate::animation::pointer_press_epoch();
        s.ready = None;
        s.newest
    });
    #[cfg(test)]
    if STATE.with(|s| {
        let mut s = s.borrow_mut();
        let simulated = s.simulated.as_mut();
        simulated.map(|pending| pending.push(id)).is_some()
    }) {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    crate::wasm_clipboard::read_system(move |text| arrived(id, text));
    #[cfg(not(target_arch = "wasm32"))]
    arrived(id, super::get_text());
}

/// The read numbered `id` finished with `text` (`None` when refused).
fn arrived(id: u64, text: Option<String>) {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        warn_once();
        return;
    };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.newest == id {
            s.ready = Some(text);
        }
    });
    // Wake an idle loop so the paste lands without waiting for input.
    crate::animation::request_draw();
}

fn warn_once() {
    let first = !WARNED.with(|w| w.replace(true));
    #[cfg(target_arch = "wasm32")]
    if first {
        web_sys::console::warn_1(
            &"agg-gui: the system clipboard could not be read for Paste (refused or empty)".into(),
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = first;
}

/// The text of a finished read, once, if no other press came since it was
/// requested; `App` pastes it into the focused widget.
pub(crate) fn take_ready() -> Option<String> {
    let epoch = crate::animation::pointer_press_epoch();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let text = s.ready.take()?;
        (s.epoch == epoch).then_some(text)
    })
}

/// Until the guard drops, reads on this thread are asynchronous as in a
/// browser: [`request`] only records the request and
/// [`resolve_simulated`] finishes it.
#[cfg(test)]
pub(crate) fn simulate() -> SimulatedReads {
    STATE.with(|s| {
        *s.borrow_mut() = State {
            simulated: Some(Vec::new()),
            ..State::default()
        }
    });
    SimulatedReads
}

/// Restores synchronous reads when dropped.
#[cfg(test)]
pub(crate) struct SimulatedReads;

#[cfg(test)]
impl Drop for SimulatedReads {
    fn drop(&mut self) {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }
}

#[cfg(test)]
impl SimulatedReads {
    /// How many reads are pending.
    pub(crate) fn pending(&self) -> usize {
        STATE.with(|s| s.borrow().simulated.as_ref().map_or(0, Vec::len))
    }

    /// Finish the oldest pending read with `text` (`None`: refused).
    pub(crate) fn resolve(&self, text: Option<&str>) {
        let id = STATE.with(|s| {
            let mut s = s.borrow_mut();
            let pending = s.simulated.as_mut()?;
            (!pending.is_empty()).then(|| pending.remove(0))
        });
        if let Some(id) = id {
            arrived(id, text.map(str::to_string));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_read_delivers_and_a_press_meanwhile_cancels_it() {
        let reads = simulate();
        assert!(reads_asynchronously());
        request();
        request();
        assert_eq!(reads.pending(), 2);
        reads.resolve(Some("old"));
        assert_eq!(take_ready(), None, "superseded");
        reads.resolve(Some("new"));
        assert_eq!(take_ready().as_deref(), Some("new"));
        assert_eq!(take_ready(), None, "once");

        request();
        crate::animation::bump_pointer_press_epoch();
        reads.resolve(Some("late"));
        assert_eq!(take_ready(), None, "the user pressed elsewhere meanwhile");

        request();
        reads.resolve(None);
        assert_eq!(take_ready(), None, "refused: nothing");
        request();
        reads.resolve(Some(""));
        assert_eq!(take_ready(), None, "empty: nothing");
        drop(reads);
        assert_eq!(reads_asynchronously(), cfg!(target_arch = "wasm32"));
    }
}
