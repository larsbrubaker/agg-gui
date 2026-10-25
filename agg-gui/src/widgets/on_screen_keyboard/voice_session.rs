//! Drives the registered voice provider (`voice.rs`): when to prepare,
//! start, stop and cancel it, and what its text does to the focused field.
//!
//! A dictation is a session: it starts with a tap on the microphone key and
//! ends when the provider is done (after a stop, on its own, or with an
//! error), on cancel, or when a key is typed, focus moves or the keyboard
//! closes. While it runs, every [`poll`] (once a frame and after each event)
//! takes the provider's text and turns it into [`VoiceDelivery`]s queued in
//! an outbox: committed text is inserted, the pending tail is shown as
//! composition text. The App (`widget/app/voice.rs`) drains the outbox into
//! the focused widget, as it drains the keyboard's synthetic keys
//! (`events.rs`), and starts a dictation a tap asked for
//! ([`take_start_request`], [`begin`]), since only the App can read the
//! focused field's text before the caret.
//!
//! The provider and the session live in thread-locals of their own, apart
//! from the keyboard's `state.rs`, so a provider is never called while the
//! keyboard's state is borrowed. The microphone key paints the state the
//! last poll cached; painting never calls the provider.

use std::cell::RefCell;
use std::time::Duration;

use web_time::Instant;

use super::state::with_state_ref;
use super::voice::{VoiceContext, VoiceInput, VoicePoll, VoiceState, VoiceText};

/// Text for the focused widget, in the order it must arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VoiceDelivery {
    /// Insert at the caret ([`crate::Event::TextInput`]); ends any
    /// composition shown.
    Commit(String),
    /// Show as composition text at the caret ([`crate::Event::ImePreedit`]);
    /// empty removes it.
    Preedit(String),
}

/// How long an error message stays up.
pub(super) const ERROR_SHOWN_FOR: Duration = Duration::from_secs(4);

/// One dictation.
struct Session {
    /// The tail shown in the field now, as delivered (with the leading
    /// space when one is due).
    shown: String,
    /// The space before the dictation's first text is still due.
    lead_space: bool,
    /// When it started (the microphone's pulse without a level).
    started_at: Instant,
    /// The provider has reported listening or finishing since the start; a
    /// provider still `Ready` just after `start` hasn't begun yet.
    seen_active: bool,
    /// The user stopped it; the provider is finishing.
    stopping: bool,
    /// The user stopped it before the provider had begun: a provider that
    /// begins listening after that started late, and is cancelled.
    stopped_early: bool,
    /// The error the provider reported when a tap retried it: stale until
    /// the provider reports anything else.
    stale_error: Option<VoiceState>,
    /// The dictated field as its last delivery left it: how a field the app
    /// rebuilds is told from another widget put in its place
    /// ([`follow_plan`]).
    field: Option<FieldMark>,
}

/// What the dictated field was after the dictation's last delivery.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FieldMark {
    /// Its concrete type (`None` for a widget that can't say), and its
    /// type name.
    pub kind: (Option<std::any::TypeId>, &'static str),
    /// The app's id for it, which a rebuilt field carries over.
    pub focus_id: Option<crate::focus::FocusId>,
    /// Its committed text, the composition left out.
    pub text: String,
    /// The caret's byte offset in `text`.
    pub caret: usize,
}

#[derive(Default)]
struct Hub {
    session: Option<Session>,
    /// The provider's state at the last poll.
    state: Option<VoiceState>,
    /// The error message on show, and until when.
    banner: Option<(String, Instant)>,
    outbox: Vec<VoiceDelivery>,
    /// A tap asked for a dictation; the App starts it ([`begin`]).
    start_requested: bool,
}

thread_local! {
    static PROVIDER: RefCell<Option<Box<dyn VoiceInput>>> = RefCell::new(None);
    static HUB: RefCell<Hub> = RefCell::new(Hub::default());
}

/// Call `f` on the provider; `None` without one.
fn with_provider<R>(f: impl FnOnce(&mut dyn VoiceInput) -> R) -> Option<R> {
    PROVIDER.with(|p| p.borrow_mut().as_mut().map(|b| f(b.as_mut())))
}

fn with_hub<R>(f: impl FnOnce(&mut Hub) -> R) -> R {
    HUB.with(|h| f(&mut h.borrow_mut()))
}

/// The keyboard is up for a text field (dictation has somewhere to go).
fn keyboard_up() -> bool {
    with_state_ref(|s| s.enabled && s.text_input_focused)
}

/// The focused field is a password field: no microphone, no dictation.
fn field_is_secure() -> bool {
    with_state_ref(|s| s.hints.secure)
}

/// Put `provider` in place of the registered one and drop the old one
/// after the provider cell is released (its `Drop` may do anything).
fn swap_provider(provider: Option<Box<dyn VoiceInput>>) -> bool {
    let old = PROVIDER.with(|p| p.replace(provider));
    let had = old.is_some();
    drop(old);
    had
}

pub(super) fn set_provider(provider: Option<Box<dyn VoiceInput>>) {
    // A dictation in progress belongs to the old provider: end it first.
    cancel();
    let had = swap_provider(provider);
    with_hub(|h| {
        h.state = None;
        h.banner = None;
        h.start_requested = false;
    });
    if keyboard_up() {
        with_provider(|p| p.prepare());
    }
    poll();
    if had != has_provider() {
        // The Gboard toolbar that holds the key comes or goes with the
        // provider, and with it the keyboard's height.
        crate::animation::request_layout();
    }
    crate::animation::request_draw();
}

pub(super) fn has_provider() -> bool {
    // Borrowed means a provider is being called, so there is one.
    PROVIDER.with(|p| p.try_borrow().map(|b| b.is_some()).unwrap_or(true))
}

/// Whether the microphone key shows: a provider is registered and the
/// focused field isn't a password field.
pub(super) fn key_shown() -> bool {
    has_provider() && !field_is_secure()
}

/// The state cached at the last poll.
pub(super) fn provider_state() -> Option<VoiceState> {
    if !has_provider() {
        return None;
    }
    with_hub(|h| h.state.clone())
}

pub(super) fn is_dictating() -> bool {
    with_hub(|h| h.session.is_some())
}

/// The keyboard came up: let the provider get ready ahead of the first tap.
pub(super) fn keyboard_opened() {
    if with_provider(|p| p.prepare()).is_some() {
        crate::animation::request_draw();
    }
}

/// A tap on the microphone key, decided on the state of the last poll:
/// stop a dictation in progress; ask for one when ready, or after an error
/// that may be retried; show again an error that may not. Taps while the
/// provider prepares or finishes, or is unavailable, do nothing.
pub(super) fn toggle() {
    let state = with_hub(|h| h.state.clone());
    let running = with_hub(|h| h.session.as_ref().map(|s| s.stopping));
    match (running, state) {
        (Some(false), _) => {
            with_hub(|h| {
                if let Some(s) = h.session.as_mut() {
                    s.stopping = true;
                    s.stopped_early = !s.seen_active;
                }
            });
            with_provider(|p| p.stop());
        }
        (Some(true), _) => return,
        (None, Some(VoiceState::Ready | VoiceState::Error { retry: true, .. })) => {
            with_hub(|h| h.start_requested = true);
        }
        (None, Some(VoiceState::Error { message, .. })) => {
            show_banner(message);
        }
        _ => return,
    }
    crate::animation::request_draw();
}

/// Whether a tap asked for a dictation since the last call (the App then
/// calls [`begin`]).
pub(crate) fn take_start_request() -> bool {
    with_hub(|h| std::mem::take(&mut h.start_requested))
}

/// Start the dictation a tap asked for. `before_caret` is the focused
/// field's text up to the caret, which decides the leading space and
/// [`VoiceContext::sentence_start`].
pub(crate) fn begin(before_caret: &str) {
    if !keyboard_up() || field_is_secure() || is_dictating() || !has_provider() {
        return;
    }
    // Capitalise only where the keyboard would: in a field that holds
    // sentences, at a sentence start.
    let capitalises = with_state_ref(|s| s.hints.auto_capitalize);
    let ctx = VoiceContext {
        sentence_start: capitalises && super::is_sentence_start(before_caret),
        ..VoiceContext::default()
    };
    let lead_space = before_caret
        .chars()
        .last()
        .is_some_and(|c| !c.is_whitespace());
    with_hub(|h| {
        let stale_error = match &h.state {
            Some(error @ VoiceState::Error { .. }) => Some(error.clone()),
            _ => None,
        };
        h.banner = None;
        h.session = Some(Session {
            shown: String::new(),
            lead_space,
            started_at: crate::clock::now(),
            seen_active: false,
            stopping: false,
            stopped_early: false,
            stale_error,
            field: None,
        });
    });
    with_provider(|p| p.start(&ctx));
    crate::animation::request_draw();
}

/// Stop listening and discard the tail not yet committed.
pub(super) fn cancel() {
    let Some(session) = with_hub(|h| h.session.take()) else {
        return;
    };
    with_provider(|p| p.cancel());
    if !session.shown.is_empty() {
        with_hub(|h| h.outbox.push(VoiceDelivery::Preedit(String::new())));
    }
    crate::animation::request_draw();
}

/// End the dictation where it stands, because a key is typed, focus is
/// moving or the keyboard is going away: the tail shown in the field is
/// committed (what the user saw stays) and the provider is cancelled, so
/// nothing more arrives after the typing or in the next field. The caller
/// drains the outbox while the field still has focus.
pub(crate) fn end_keeping_tail() {
    let Some(session) = with_hub(|h| h.session.take()) else {
        return;
    };
    with_provider(|p| p.cancel());
    if !session.shown.is_empty() {
        with_hub(|h| h.outbox.push(VoiceDelivery::Commit(session.shown)));
    }
    crate::animation::request_draw();
}

/// Remember the dictated field as a delivery left it.
pub(crate) fn note_field(mark: Option<FieldMark>) {
    with_hub(|h| {
        if let Some(s) = h.session.as_mut() {
            s.field = mark;
        }
    });
}

/// The app put a new widget where the dictated field was and focus followed
/// it: whether the dictation follows too. It does when the newcomer is the
/// same field rebuilt: the same kind of widget with the same id, holding the
/// text the dictation left (an empty text proves nothing without an id:
/// every form field starts empty), or that text with the tail shown spliced
/// in at the caret (a text area sharing the old one's edit state, whose
/// buffer kept the composition). Then `Some((caret, tail, tail_in_text))`
/// for [`Widget::resume_text_input`](crate::widget::Widget::resume_text_input);
/// otherwise `None`, and the App abandons the dictation.
pub(crate) fn follow_plan(
    kind: (Option<std::any::TypeId>, &'static str),
    focus_id: Option<crate::focus::FocusId>,
    text: &str,
) -> Option<(usize, String, bool)> {
    with_hub(|h| {
        let s = h.session.as_ref()?;
        let m = s.field.as_ref()?;
        if m.kind != kind || m.focus_id != focus_id {
            return None;
        }
        if text == m.text && (!m.text.is_empty() || m.focus_id.is_some()) {
            return Some((m.caret, s.shown.clone(), false));
        }
        let (before, after) = m.text.split_at(m.caret.min(m.text.len()));
        let spliced = format!("{before}{}{after}", s.shown);
        (!s.shown.is_empty() && text == spliced).then(|| (m.caret, s.shown.clone(), true))
    })
}

/// End the dictation and drop everything queued for its field: the app
/// replaced the field with another widget, which must not get its words.
pub(crate) fn abandon() {
    let Some(_session) = with_hub(|h| h.session.take()) else {
        return;
    };
    with_provider(|p| p.cancel());
    with_hub(|h| h.outbox.clear());
    crate::animation::request_draw();
}

/// Poll the provider: cache its state, queue its new text, and end the
/// session when the provider is done or the keyboard is gone. A provider
/// found listening with no session is cancelled. Called by the App once a
/// frame and after each event, before it drains [`take_deliveries`].
pub(crate) fn poll() {
    let Some(VoicePoll { state, text }) = with_provider(|p| p.poll()) else {
        return;
    };
    let stale = with_hub(|h| match h.session.as_mut() {
        Some(s) if s.stale_error.as_ref() == Some(&state) => true,
        Some(s) => {
            s.stale_error = None;
            false
        }
        None => false,
    });
    note_state(&state, stale);
    if !is_dictating() {
        // Text left over from a dictation that ended is dropped; a provider
        // still (or newly) listening for none is stopped.
        if state.is_active() {
            with_provider(|p| p.cancel());
        }
        return;
    }
    if let Some(text) = text {
        with_hub(|h| apply_text(h, text));
    }
    if !keyboard_up() {
        end_keeping_tail();
        return;
    }
    let ended = with_hub(|h| {
        let Some(s) = h.session.as_mut() else {
            return false;
        };
        if s.stopped_early && state.is_active() {
            // A start that landed after the user stopped it.
            return true;
        }
        s.seen_active |= state.is_active();
        match state {
            VoiceState::Listening { .. } | VoiceState::Finishing => false,
            VoiceState::Unavailable => true,
            VoiceState::Error { .. } => !stale || s.stopping,
            VoiceState::Ready | VoiceState::Preparing { .. } => s.seen_active || s.stopping,
        }
    });
    if ended {
        // A tail the provider left pending is committed: what the user saw
        // stays.
        with_hub(|h| {
            if let Some(s) = h.session.take() {
                if !s.shown.is_empty() {
                    h.outbox.push(VoiceDelivery::Commit(s.shown));
                }
            }
        });
        if state.is_active() {
            with_provider(|p| p.cancel());
        }
        crate::animation::request_draw();
    }
}

/// Cache `state`, and put an error's message up when it first appears
/// (unless it is the stale error of a retry).
fn note_state(state: &VoiceState, stale: bool) {
    let message = with_hub(|h| {
        let new_error = match state {
            VoiceState::Error { message, .. } if !stale && h.state.as_ref() != Some(state) => {
                Some(message.clone())
            }
            VoiceState::Error { .. } => None,
            _ => {
                h.banner = None;
                None
            }
        };
        if h.state.as_ref() != Some(state) {
            crate::animation::request_draw();
        }
        h.state = Some(state.clone());
        new_error
    });
    if let Some(message) = message {
        show_banner(message);
    }
}

fn show_banner(message: String) {
    with_hub(|h| h.banner = Some((message, crate::clock::now() + ERROR_SHOWN_FOR)));
    crate::animation::request_draw_after(ERROR_SHOWN_FOR);
}

/// `text` after the leading space when the dictation still owes one.
fn with_lead(s: &Session, text: String) -> String {
    if s.lead_space && !text.starts_with(char::is_whitespace) {
        format!(" {text}")
    } else {
        text
    }
}

/// Queue `text`: committed text first (which ends the composition shown),
/// then the new tail if it differs from what the field shows.
fn apply_text(h: &mut Hub, text: VoiceText) {
    let Some(s) = h.session.as_mut() else {
        return;
    };
    if !text.committed.is_empty() {
        let committed = with_lead(s, text.committed);
        s.lead_space = false;
        h.outbox.push(VoiceDelivery::Commit(committed));
        s.shown.clear();
    }
    let want = if text.pending.is_empty() {
        String::new()
    } else {
        with_lead(s, text.pending)
    };
    if want != s.shown {
        s.shown = want.clone();
        h.outbox.push(VoiceDelivery::Preedit(want));
    }
    crate::animation::request_draw();
}

/// Whether anything is queued for the focused widget.
pub(crate) fn has_deliveries() -> bool {
    with_hub(|h| !h.outbox.is_empty())
}

/// Everything queued for the focused widget since the last call, in order.
pub(crate) fn take_deliveries() -> Vec<VoiceDelivery> {
    with_hub(|h| std::mem::take(&mut h.outbox))
}

/// Whether the keyboard wants every frame: while the provider listens or
/// finishes for a dictation, so the microphone pulses and the text is
/// picked up promptly (not while a start waits on a permission prompt).
pub(super) fn wants_frames() -> bool {
    with_hub(|h| h.session.as_ref().is_some_and(|s| s.seen_active))
}

/// What the microphone key and the status line show this frame.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct VoiceView {
    pub state: VoiceState,
    /// An error message on show.
    pub banner: Option<String>,
    /// How far the listening key is swollen, 0..=1: the input level, or a
    /// slow pulse without one.
    pub pulse: f64,
}

/// The view for this frame's paint, from the state the last poll cached;
/// `None` when the key doesn't show. Keeps the app awake for the moment the
/// error message comes down.
pub(super) fn paint_view() -> Option<VoiceView> {
    if !key_shown() {
        return None;
    }
    let now = crate::clock::now();
    with_hub(|h| {
        let state = h
            .state
            .clone()
            .unwrap_or(VoiceState::Preparing { progress: None });
        let banner = match &h.banner {
            Some((message, until)) if now < *until => {
                crate::animation::request_draw_after(*until - now);
                Some(message.clone())
            }
            _ => None,
        };
        let pulse = match (&state, &h.session) {
            (VoiceState::Listening { level: Some(level) }, _) => level.clamp(0.0, 1.0) as f64,
            (VoiceState::Listening { level: None }, Some(s)) => {
                let t = now.duration_since(s.started_at).as_secs_f64();
                0.5 - 0.5 * (t * std::f64::consts::TAU / 1.2).cos()
            }
            _ => 0.0,
        };
        Some(VoiceView {
            state,
            banner,
            pulse,
        })
    })
}

/// Drop the provider and the session (tests start from nothing).
#[cfg(test)]
pub(super) fn reset() {
    swap_provider(None);
    with_hub(|h| *h = Hub::default());
}
