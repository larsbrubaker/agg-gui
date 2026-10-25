//! Voice input (dictation) for the on-screen keyboard: the engine-neutral
//! interface a speech-to-text provider implements, and its registration.
//!
//! The keyboard owns the user-facing side: a microphone key, its states, and
//! putting the words into the focused field. A provider owns the rest:
//! loading a model, capturing audio, recognising speech. agg-gui knows
//! nothing about any engine; an app or platform shell registers one provider
//! with [`set_voice_input`] and every text field of every agg-gui app can
//! then take dictation, with no change to
//! [`TextField`](crate::widgets::TextField) or
//! [`TextArea`](crate::widgets::TextArea).
//!
//! `voice_session.rs` drives a provider (when to start, stop and poll it,
//! and where its text goes), `voice_paint.rs` draws the microphone
//! key and the status line, and `widget/app/voice.rs` delivers the text to
//! the focused widget.
//!
//! ## Design
//!
//! ### The interface
//!
//! [`VoiceInput`] is a plain trait of non-blocking calls, all made on the UI
//! thread: [`prepare`](VoiceInput::prepare), [`start`](VoiceInput::start),
//! [`stop`](VoiceInput::stop), [`cancel`](VoiceInput::cancel) and
//! [`poll`](VoiceInput::poll). No call may block and none is async: on wasm
//! everything runs on the one UI thread, so a call that waited for audio or
//! a model would freeze the page. A provider starts work and returns; the
//! work runs elsewhere (a Web Worker, a recorder thread) and its results wait
//! in the provider until the keyboard polls for them. A state change may
//! show at any later poll: nothing has to happen inside `start` or `stop`.
//!
//! The keyboard polls once per frame ([`App::paint`](crate::widget::App::paint))
//! and after each input event; the key paints the state of the last poll. A
//! provider whose state or text changes while the app is idle wakes it:
//! [`crate::animation::request_draw`] from the UI thread (a Worker's
//! `onmessage` handler runs there), or, from another thread, the
//! `signal_async_state_change` of the UI thread's queue (captured with
//! [`crate::ui_thread::current_queue`] while still on the UI thread). Once
//! the provider reports listening, the keyboard draws every frame (the
//! microphone pulses), so text arriving then is picked up within a frame.
//!
//! ### States
//!
//! [`VoiceState`] is what the microphone key shows:
//!
//! | state        | key                                   | tap               |
//! |--------------|---------------------------------------|-------------------|
//! | `Unavailable`| slashed microphone, dimmed            | nothing           |
//! | `Ready`      | microphone                            | dictate: `start` (the provider may load first) |
//! | `Preparing`  | dimmed, with a progress ring (and the percentage in the status line) when the provider knows its progress; "Starting…" for a dictation waiting without one | a dictation waiting for it: cancel it (`stop`); none: dictate (`start`) |
//! | `Listening`  | lit, pulsing with the input level; "Speak now", only once audio flows | stop (commit) |
//! | `Finishing`  | lit                                   | nothing           |
//! | `Error`      | slashed microphone; the message shows for a few seconds | `retry`: start again; otherwise show the message again |
//!
//! ### Loading on the first tap
//!
//! The keyboard never calls [`prepare`](VoiceInput::prepare): opening it,
//! registering a provider and every frame ask the provider for nothing but
//! [`poll`](VoiceInput::poll), so a page that shows the keyboard but never
//! dictates downloads nothing. A provider that hasn't loaded yet reports
//! `Ready` ("tap to dictate"), not `Preparing`; `Unavailable` is only for a
//! provider that can never dictate here.
//!
//! The first tap means "dictate": the keyboard calls
//! [`start`](VoiceInput::start), and a provider that has to load first does
//! so, reporting `Preparing` (with its progress) meanwhile, then begins
//! listening by itself: the user is not asked to tap again. A tap while that
//! dictation waits calls [`stop`](VoiceInput::stop), which aborts the start:
//! nothing listens when the load completes, though the load may carry on so
//! the next tap is quick. What belongs to the tap the provider does
//! synchronously inside that `start`, which runs in the tap's input event,
//! even when listening has to wait for the load: starting audio, which iOS
//! Safari allows only inside a user gesture, and asking for the microphone,
//! so the browser sees the gesture and its prompt shows at the tap rather
//! than after a download (an awaited step before the request would lose
//! the gesture). A provider may capture from then on, keeping what is said
//! during the load for when it can recognise it, so a user who speaks at
//! once isn't cut off; it then catches up, its text trailing the speech by
//! as long as recognising that backlog takes. A stop or cancel meanwhile
//! releases the microphone and drops that audio. A provider whose
//! microphone is refused while it loads reports the error, which ends the
//! dictation.
//!
//! An app that wants the model warm before the first tap calls `prepare`
//! itself: on the provider before registering it, or through a handle it
//! keeps (agg-gui-whistle's `WhistleVoice::prepare`). It must be cheap and
//! idempotent once prepared, and may retry after an error; a provider
//! loading for it may report `Preparing` or `Ready`, and a tap then still
//! means "dictate".
//!
//! The keyboard keeps the provider and its own session in step: a provider
//! it finds listening or finishing with no dictation in progress (a start
//! that completed after the user stopped it, say) is cancelled. After a tap
//! retries an error, the error reported until then is taken as stale until
//! the provider reports something else, so a provider that keeps reporting
//! the old error for a poll or two after `start` doesn't end the new
//! dictation at once. (A retry that fails at once with the same message
//! looks the same as one not begun: report another state first, such as
//! `Preparing`, then the new error.)
//!
//! ### Text
//!
//! [`poll`](VoiceInput::poll) returns the state and, when there is any, a
//! [`VoiceText`]: `committed` is the final text recognised since the last
//! poll, inserted at the caret as given; `pending` is the whole unconfirmed
//! tail, which replaces the previous tail. The keyboard inserts committed
//! text as [`Event::TextInput`](crate::Event::TextInput) and shows the tail
//! in the field as composition text
//! ([`Event::ImePreedit`](crate::Event::ImePreedit), underlined at the
//! caret), the path an IME takes, so every text widget that takes IME input
//! takes dictation too. Text polled while no dictation is in progress (left
//! over from one that ended) is dropped.
//!
//! Spacing: the provider spaces its own words and updates (the space before
//! each update's text but the first, none when a word continues). The
//! keyboard adds the one space before a dictation's first text when the
//! field has text and the character before the caret isn't whitespace, so
//! "Hello" dictated after "Say" reads "Say Hello". [`VoiceContext`] tells the
//! provider whether the caret starts a sentence, for its capitalisation.
//!
//! - Tapping the microphone while listening calls
//!   [`stop`](VoiceInput::stop): the provider finishes the audio it has,
//!   reports `Finishing` while it does, delivers the rest as committed text
//!   and goes back to `Ready`. A tail the provider leaves pending at the end
//!   is committed by the keyboard: what the user saw stays.
//! - [`cancel_voice_input`] calls [`cancel`](VoiceInput::cancel): the shown
//!   tail is removed and nothing more is inserted; committed text stays.
//! - Typing a key, moving focus to another widget, or closing the keyboard
//!   ends the dictation: the tail shown in the field is committed there and
//!   the provider is cancelled, so no late words land after the typing or in
//!   the next field. A field the app rebuilds in place (from its text cell,
//!   or its shared edit state's epoch, on every change) keeps the
//!   dictation: focus follows the rebuilt field and so does the dictation,
//!   the new widget taking over the caret and the tail shown
//!   ([`Widget::resume_text_input`](crate::widget::Widget::resume_text_input)).
//!   It counts as the same field when it is the same kind of widget with
//!   the same focus id, holding the committed text the dictation left
//!   (empty text proves nothing without an id). Any other widget put in the
//!   field's place ends the dictation and gets none of its words.
//! - Password fields ([`TextInputHints::secure`](super::TextInputHints::secure))
//!   get no microphone, as on Gboard.
//!
//! ### Plugging in a provider
//!
//! - **Web (Web Worker).** The provider object lives on the UI thread and
//!   owns the Worker. `start`, inside the tap, posts "load" the first time
//!   (the worker downloads and caches the model, posting progress), posts
//!   the stream's start, and calls `getUserMedia` in that same task (the
//!   dictation's one request, so its prompt shows at the tap), wiring the
//!   stream to an `AudioWorklet` (its module prefetched with the provider)
//!   the moment it arrives and streaming its audio to the worker, which
//!   queues it behind the load; the worker posts `{committed, pending}`
//!   updates. `onmessage` accumulates them, updates the state and calls
//!   `request_draw`; `poll` hands over what accumulated. `stop` posts
//!   "finish"; `cancel` posts "cancel" and drops any update still in flight.
//! - **Native (recorder thread).** The first `start` loads the model on a
//!   background thread and opens the audio device at once, its audio
//!   queueing behind the load; a worker thread feeds the engine and sends
//!   updates over a channel. `poll` drains the channel
//!   without blocking; the worker wakes the UI thread through its queue
//!   handle.
//!
//! The provider registered is the one for the calling thread (the keyboard's
//! state is per UI thread, like the rest of the keyboard).

use super::voice_session;

/// What the voice provider is doing, as the microphone key shows it. See the
/// table in the module docs.
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceState {
    /// Voice input can never be used here (the platform has no way to
    /// record or recognise speech), known before any tap. The key shows a
    /// slashed microphone and does nothing. A provider that can tell only
    /// once tapped, or wants to say why, reports `Ready` and then, at the
    /// tap, a [`VoiceState::Error`] without `retry`.
    Unavailable,
    /// Getting ready to listen: downloading or loading a model, or opening
    /// the microphone, for a dictation a tap started (perhaps capturing
    /// already, for when the model is in), or for an app's
    /// [`VoiceInput::prepare`]. `progress` runs 0..=1 when the provider
    /// knows it.
    Preparing { progress: Option<f32> },
    /// A tap may dictate: [`VoiceInput::start`] listens at once, or first
    /// loads what it needs (reporting `Preparing`). The state of a provider
    /// that has loaded nothing yet.
    Ready,
    /// Listening: the cue to speak (the status line says "Speak now").
    /// Report it only once audio is flowing (the microphone's first audio
    /// has reached the recogniser's pipeline), not merely once it opened,
    /// so nothing said after the cue is lost. `level` is the input level
    /// 0..=1 when the provider measures it; the key pulses with it (or on a
    /// timer without one).
    Listening { level: Option<f32> },
    /// Stopped listening; finishing the audio it already has.
    Finishing,
    /// Something failed. The keyboard shows `message` for a few seconds.
    /// With `retry`, a tap on the key calls [`VoiceInput::start`] again;
    /// without (a platform that can't record at all), a tap only shows
    /// the message again.
    Error { message: String, retry: bool },
}

impl VoiceState {
    /// Listening or finishing: a dictation is in progress.
    pub fn is_active(&self) -> bool {
        matches!(self, VoiceState::Listening { .. } | VoiceState::Finishing)
    }
}

/// Text recognised since the last [`VoiceInput::poll`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceText {
    /// Final text recognised since the last poll, inserted at the caret as
    /// given (including the space before it, between the provider's own
    /// updates). Never retracted.
    pub committed: String,
    /// The whole unconfirmed tail after everything committed so far; it
    /// replaces the previous tail. Empty when there is none.
    pub pending: String,
}

/// What [`VoiceInput::poll`] reports: the state now, and the text recognised
/// since the last poll (`None` when nothing changed: no new committed text
/// and the same pending tail).
#[derive(Debug, Clone, PartialEq)]
pub struct VoicePoll {
    pub state: VoiceState,
    pub text: Option<VoiceText>,
}

/// What the keyboard knows about where a dictation goes, handed to
/// [`VoiceInput::start`].
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceContext {
    /// The caret starts a sentence (the field is empty before it, or the
    /// text before it ends in `.`, `!`, `?` or a line break): capitalise the
    /// first word. The same rule the keyboard's Shift follows.
    pub sentence_start: bool,
    /// The language to recognise (a BCP 47 tag such as `"en"`), or `None` to
    /// let the provider decide. The keyboard has no language setting yet, so
    /// it passes `None`.
    pub language: Option<String>,
}

/// A speech-to-text provider for the on-screen keyboard's microphone key.
///
/// Every method is called on the UI thread and must return at once: start
/// the work and report its results through [`poll`](Self::poll), which the
/// keyboard calls every frame while it is up. See the module docs for the
/// full contract.
///
/// A provider must not call back into this module ([`set_voice_input`] and
/// friends) from inside these methods.
pub trait VoiceInput {
    /// Get ready to listen ahead of the first tap (download or load a
    /// model). Opt-in: the keyboard never calls it (it loads nothing before
    /// a tap; see the module docs); an app that wants the model warm early
    /// calls it. Cheap and idempotent once prepared, and free to retry what
    /// failed before.
    fn prepare(&mut self);
    /// Start a dictation. Called when the user taps the key in
    /// [`VoiceState::Ready`] or [`VoiceState::Preparing`], or in a
    /// [`VoiceState::Error`] with `retry`, inside the tap's input event (a
    /// user gesture). A provider that hasn't loaded yet loads first,
    /// reporting [`VoiceState::Preparing`] with its progress, then begins
    /// listening with no further call. The provider may go on reporting its
    /// old state for a while and report [`VoiceState::Listening`] at a later
    /// poll; after an error it may first retry what failed. A provider that
    /// can't start reports [`VoiceState::Error`].
    fn start(&mut self, ctx: &VoiceContext);
    /// Stop listening and finish: recognise the audio already captured,
    /// deliver it as committed text (the pending tail included), then return
    /// to [`VoiceState::Ready`]. Report [`VoiceState::Finishing`] meanwhile.
    /// Deliver the text no later than the poll that reports `Ready`. A start
    /// still in flight, one waiting for a load included, is aborted: the
    /// provider must not begin listening after this (the load may go on).
    fn stop(&mut self);
    /// Stop listening and discard: no more text, pending or committed, is
    /// delivered for this dictation, and a start still in flight is aborted.
    /// May be called when nothing is in progress; then it does nothing.
    fn cancel(&mut self);
    /// The current state and the text recognised since the last poll: every
    /// update accumulated since then (committed texts joined, the latest
    /// pending tail).
    fn poll(&mut self) -> VoicePoll;
}

/// Register the voice provider for this UI thread (`None` removes it). The
/// microphone key shows while one is registered. Registering asks nothing
/// of the provider but a poll (it loads on the first tap); replacing or
/// removing a provider during a dictation cancels it, removing the tail it
/// showed.
pub fn set_voice_input(provider: Option<Box<dyn VoiceInput>>) {
    voice_session::set_provider(provider);
}

/// Whether a voice provider is registered (the microphone key shows, except
/// on password fields).
pub fn has_voice_input() -> bool {
    voice_session::has_provider()
}

/// The registered provider's state at the last poll, or `None` without one.
pub fn voice_state() -> Option<VoiceState> {
    voice_session::provider_state()
}

/// Whether the keyboard is taking dictation now: started and not yet ended.
pub fn is_dictating() -> bool {
    voice_session::is_dictating()
}

/// Stop listening and discard the tail not yet committed (committed text
/// stays in the field). Does nothing when not dictating.
pub fn cancel_voice_input() {
    voice_session::cancel();
}
