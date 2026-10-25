//! Voice input on the on-screen keyboard (`widgets/on_screen_keyboard/voice.rs`):
//! where the microphone key shows (only with a provider registered, never
//! on a password field; Gboard's toolbar, the iPhone's strip), that the hit
//! areas still tile the panel around it, that the provider is prepared when
//! the keyboard opens, and that the field stays above the keyboard when the
//! toolbar appears.
//!
//! Also the scripted provider and the phone rig the dictation tests in
//! `on_screen_keyboard_voice_session.rs` share. The fake changes state
//! asynchronously, as a Web Worker does: what `start` or `stop` sets in
//! motion shows at a later poll, never inside the call.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::geometry::{Point, Rect};
use crate::input_profile::{set_input_profile, InputProfile};
use crate::widgets::on_screen_keyboard::state::with_state_ref;
use crate::widgets::on_screen_keyboard::{
    self as osk, key::PaintedKey, test_hook, KeyAction, KeyCap, KeyGlyph, VoiceContext, VoiceInput,
    VoicePoll, VoiceState, VoiceText,
};
use crate::widgets::TextField;
use crate::{App, FlexColumn, Key, Modifiers, MouseButton, Size, SizedBox, Widget};

use super::on_screen_keyboard_touch::font;

pub(super) const PHONE: Size = Size {
    width: 412.0,
    height: 800.0,
};

/// What the fake provider was asked to do, and what it reports.
pub(super) struct Script {
    pub calls: Vec<&'static str>,
    /// The context of every `start`.
    pub contexts: Vec<VoiceContext>,
    pub state: VoiceState,
    /// Updates not yet polled; the next poll hands them all over.
    pub texts: Vec<VoiceText>,
    /// States still to come: `(polls to wait first, state)`, one applied
    /// per poll.
    pub queued: VecDeque<(u32, VoiceState)>,
    /// Polls a `start` or `stop` takes to show.
    pub lag: u32,
    /// `stop` and `cancel` don't abort a start in flight (a racy provider).
    pub ignores_abort: bool,
    /// Where the next `start` leads instead of listening.
    pub next_start_leads_to: Option<VoiceState>,
}

/// A scripted voice provider; clones share one script, so the test keeps a
/// handle after registering it.
#[derive(Clone)]
pub(super) struct FakeVoice(pub Rc<RefCell<Script>>);

impl FakeVoice {
    pub(super) fn new(state: VoiceState) -> Self {
        FakeVoice(Rc::new(RefCell::new(Script {
            calls: Vec::new(),
            contexts: Vec::new(),
            state,
            texts: Vec::new(),
            queued: VecDeque::new(),
            lag: 1,
            ignores_abort: false,
            next_start_leads_to: None,
        })))
    }

    pub(super) fn calls(&self) -> Vec<&'static str> {
        self.0.borrow().calls.clone()
    }

    pub(super) fn count(&self, call: &str) -> usize {
        self.calls().iter().filter(|c| **c == call).count()
    }

    pub(super) fn state(&self) -> VoiceState {
        self.0.borrow().state.clone()
    }

    /// Report `state` from the next poll on.
    pub(super) fn later(&self, state: VoiceState) {
        self.0.borrow_mut().queued.push_back((0, state));
    }

    /// Report `committed` new final text and `pending` as the tail at the
    /// next poll.
    pub(super) fn say(&self, committed: &str, pending: &str) {
        self.0.borrow_mut().texts.push(VoiceText {
            committed: committed.to_string(),
            pending: pending.to_string(),
        });
    }
}

impl VoiceInput for FakeVoice {
    fn prepare(&mut self) {
        self.0.borrow_mut().calls.push("prepare");
    }
    fn start(&mut self, ctx: &VoiceContext) {
        let mut s = self.0.borrow_mut();
        s.calls.push("start");
        s.contexts.push(ctx.clone());
        let to = s
            .next_start_leads_to
            .take()
            .unwrap_or(VoiceState::Listening { level: None });
        let lag = s.lag;
        s.queued.push_back((lag, to));
    }
    fn stop(&mut self) {
        let mut s = self.0.borrow_mut();
        s.calls.push("stop");
        if !s.ignores_abort {
            s.queued
                .retain(|(_, st)| !matches!(st, VoiceState::Listening { .. }));
        }
        if s.state.is_active() {
            let lag = s.lag;
            s.queued.push_back((lag, VoiceState::Finishing));
        }
    }
    fn cancel(&mut self) {
        let mut s = self.0.borrow_mut();
        s.calls.push("cancel");
        s.texts.clear();
        if !s.ignores_abort {
            s.queued.clear();
        }
        s.state = VoiceState::Ready;
    }
    fn poll(&mut self) -> VoicePoll {
        let mut s = self.0.borrow_mut();
        if let Some(front) = s.queued.front_mut() {
            if front.0 == 0 {
                if let Some((_, state)) = s.queued.pop_front() {
                    s.state = state;
                }
            } else {
                front.0 -= 1;
            }
        }
        let text = if s.texts.is_empty() {
            None
        } else {
            let texts = std::mem::take(&mut s.texts);
            Some(VoiceText {
                committed: texts.iter().map(|t| t.committed.as_str()).collect(),
                pending: texts.last().map(|t| t.pending.clone()).unwrap_or_default(),
            })
        };
        VoicePoll {
            state: s.state.clone(),
            text,
        }
    }
}

/// A phone-sized app on the `profile` keyboard, with `voice` registered
/// before the field takes focus, so the keyboard opens with it.
pub(super) struct Rig {
    _lock: std::sync::MutexGuard<'static, ()>,
    pub app: App,
}

impl Rig {
    pub(super) fn new(
        root: Box<dyn Widget>,
        profile: InputProfile,
        voice: Option<&FakeVoice>,
    ) -> Self {
        let lock = crate::input_profile::profile_test_lock();
        osk::dismiss();
        test_hook::reset();
        osk::events::clear();
        crate::widget::keyboard_scroll::reset_lift_for_test();
        crate::ux_scale::set_ux_scale(1.0);
        crate::set_device_scale(1.0);
        set_input_profile(profile);
        crate::theme::set_visuals(crate::theme::Visuals::light());
        osk::set_enabled(true);
        if let Some(voice) = voice {
            osk::set_voice_input(Some(Box::new(voice.clone())));
        }
        let mut app = App::new(root);
        app.layout(PHONE);
        app.on_key_down(Key::Tab, Modifiers::default());
        assert!(app.has_focus(), "the field takes focus");
        test_hook::force_visible();
        let mut rig = Rig { _lock: lock, app };
        rig.paint();
        rig
    }

    /// One frame: layout and paint (which polls the provider once).
    pub(super) fn paint(&mut self) -> crate::Framebuffer {
        self.app.layout(PHONE);
        let mut fb = crate::Framebuffer::new(PHONE.width as u32, PHONE.height as u32);
        {
            let mut ctx = crate::GfxCtx::new(&mut fb);
            self.app.paint(&mut ctx);
        }
        fb
    }

    /// Tap the middle of `rect` (Y-up) through the App, as a finger would.
    pub(super) fn tap(&mut self, rect: Rect) {
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let m = Modifiers::default();
        self.app
            .on_mouse_down(x, PHONE.height - y, MouseButton::Left, m);
        self.app
            .on_mouse_up(x, PHONE.height - y, MouseButton::Left, m);
    }

    pub(super) fn tap_mic(&mut self) {
        let mic = mic_key().expect("the keyboard shows the microphone");
        self.tap(mic.rect);
    }

    /// Tap the microphone and paint until the provider's start shows.
    pub(super) fn start_dictation(&mut self) {
        self.tap_mic();
        self.paint();
        assert!(osk::is_dictating());
        assert!(matches!(
            osk::voice_state(),
            Some(VoiceState::Listening { .. })
        ));
    }

    pub(super) fn field(&self) -> &TextField {
        self.app
            .root()
            .as_any()
            .and_then(|a| a.downcast_ref::<TextField>())
            .expect("root is a TextField")
    }

    pub(super) fn preedit(&self) -> Option<String> {
        self.field().preedit().map(|p| p.text.clone())
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        osk::set_enabled(false);
        test_hook::reset();
        osk::events::clear();
        crate::widget::keyboard_scroll::reset_lift_for_test();
        crate::theme::set_visuals(crate::theme::Visuals::light());
        set_input_profile(InputProfile::Desktop);
    }
}

pub(super) fn keys() -> Vec<PaintedKey> {
    with_state_ref(|s| s.last_painted_keys.clone())
}

pub(super) fn panel_height() -> f64 {
    with_state_ref(|s| s.last_panel_height.expect("the keyboard painted"))
}

pub(super) fn mic_key() -> Option<PaintedKey> {
    keys()
        .into_iter()
        .find(|k| matches!(k.action, KeyAction::Voice))
}

pub(super) fn key_where(f: impl Fn(&KeyAction) -> bool) -> PaintedKey {
    keys()
        .into_iter()
        .find(|k| f(&k.action))
        .expect("the keyboard shows the key")
}

/// A field bound to the returned text cell, holding `text`.
pub(super) fn bound_field(text: &str) -> (Box<dyn Widget>, Rc<RefCell<String>>) {
    let cell = Rc::new(RefCell::new(text.to_string()));
    let field = TextField::new(font()).with_text_cell(Rc::clone(&cell));
    (Box::new(field), cell)
}

/// A Gboard keyboard with a ready fake provider, typing into an empty
/// bound field.
pub(super) fn dictation_rig() -> (Rig, FakeVoice, Rc<RefCell<String>>) {
    let voice = FakeVoice::new(VoiceState::Ready);
    let (field, cell) = bound_field("");
    let rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
    (rig, voice, cell)
}

pub(super) fn pixel(fb: &crate::Framebuffer, p: Point) -> [u8; 4] {
    // Framebuffer row 0 is the bottom (Y-up), as the keyboard's coordinates.
    let (x, y) = (p.x as usize, p.y as usize);
    let i = (y * fb.width() as usize + x) * 4;
    let px = fb.pixels();
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// The painted keys' hit areas cover the panel exactly, never overlap, and
/// each holds its drawn key.
fn assert_hit_areas_tile_the_panel() {
    let keys = keys();
    let h = panel_height();
    let area: f64 = keys.iter().map(|k| k.hit.width * k.hit.height).sum();
    assert!(
        (area - PHONE.width * h).abs() < 1e-6,
        "hit areas cover {area}, the panel is {}",
        PHONE.width * h
    );
    for (i, a) in keys.iter().enumerate() {
        assert!(
            a.hit.x <= a.rect.x + 1e-9
                && a.hit.y <= a.rect.y + 1e-9
                && a.hit.x + a.hit.width >= a.rect.x + a.rect.width - 1e-9
                && a.hit.y + a.hit.height >= a.rect.y + a.rect.height - 1e-9,
            "{:?}: the hit area holds the drawn key",
            a.action
        );
        for b in &keys[i + 1..] {
            let w = (a.hit.x + a.hit.width).min(b.hit.x + b.hit.width) - a.hit.x.max(b.hit.x);
            let hh = (a.hit.y + a.hit.height).min(b.hit.y + b.hit.height) - a.hit.y.max(b.hit.y);
            assert!(
                w <= 1e-9 || hh <= 1e-9,
                "{:?} and {:?} overlap",
                a.action,
                b.action
            );
        }
    }
}

#[test]
fn the_microphone_shows_only_with_a_provider() {
    let (field, _) = bound_field("");
    let rig = Rig::new(field, InputProfile::MobileAndroid, None);
    assert!(mic_key().is_none(), "no provider, no microphone");
    let plain = panel_height();
    drop(rig);

    let (_rig, _voice, _) = dictation_rig();
    let mic = mic_key().expect("a provider shows the microphone");
    let h = panel_height();
    // Gboard's toolbar row above the keys holds it, at the right end.
    assert!((h - (plain + 43.0)).abs() < 1e-6, "{h} vs {plain}");
    let top_of_keys = keys()
        .iter()
        .filter(|k| !matches!(k.action, KeyAction::Voice | KeyAction::Inert))
        .map(|k| k.rect.y + k.rect.height)
        .fold(f64::MIN, f64::max);
    assert!(
        mic.rect.y > top_of_keys,
        "the microphone sits above the keys"
    );
    assert!(
        mic.rect.x + mic.rect.width > PHONE.width - 20.0,
        "at the right end: {:?}",
        mic.rect
    );
    assert!(
        (mic.rect.width - 34.0).abs() < 1e-6,
        "Gboard's 34 dp circle"
    );
}

#[test]
fn with_the_toolbar_every_point_still_belongs_to_exactly_one_key() {
    let (_rig, _voice, _) = dictation_rig();
    assert_hit_areas_tile_the_panel();
    // A touch on the toolbar beside the microphone types nothing.
    let inert = key_where(|a| matches!(a, KeyAction::Inert));
    let at = Point::new(inert.hit.x + 30.0, inert.hit.y + inert.hit.height / 2.0);
    let i = test_hook::key_at(at).expect("the toolbar takes the touch");
    assert!(matches!(keys()[i].action, KeyAction::Inert));
}

#[test]
fn on_the_iphone_the_microphone_sits_below_return_and_its_strip_types_nothing() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let (field, _) = bound_field("");
    let rig = Rig::new(field, InputProfile::MobileIOS, None);
    let plain = panel_height();
    drop(rig);
    let (field, cell) = bound_field("");
    let mut rig = Rig::new(field, InputProfile::MobileIOS, Some(&voice));
    let mic = mic_key().expect("the iPhone shows the microphone");
    assert_eq!(panel_height(), plain, "the strip already has room for it");
    let enter = key_where(|a| matches!(a, KeyAction::Enter));
    assert!(
        mic.rect.y + mic.rect.height <= enter.rect.y,
        "below the keys"
    );
    let mid = mic.rect.x + mic.rect.width / 2.0;
    assert!(
        (mid - (enter.rect.x + enter.rect.width / 2.0)).abs() < 1e-6,
        "under Return"
    );
    let emoji = keys()
        .into_iter()
        .find(|k| matches!(k.cap, KeyCap::Glyph(KeyGlyph::Emoji)))
        .expect("the iPhone shows the emoji key");
    assert!(emoji.hit.x + emoji.hit.width <= mic.hit.x);
    assert_hit_areas_tile_the_panel();

    // The status line's strip, under the space bar, takes a tap and types
    // nothing.
    let status = key_where(|a| matches!(a, KeyAction::Inert));
    let space = key_where(|a| matches!(a, KeyAction::Space));
    assert!(status.hit.y + status.hit.height <= space.hit.y + 1e-9);
    rig.tap(status.hit);
    rig.paint();
    assert_eq!(*cell.borrow(), "");
}

#[test]
fn password_fields_get_no_microphone() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let (field, _) = bound_field("");
    let rig = Rig::new(field, InputProfile::MobileAndroid, None);
    let plain = panel_height();
    drop(rig);

    let secret = TextField::new(font()).with_password_mode(true);
    assert!(secret.text_input_hints().secure, "a masked field says so");
    assert!(!TextField::new(font()).text_input_hints().secure);
    let _rig = Rig::new(Box::new(secret), InputProfile::MobileAndroid, Some(&voice));
    assert!(mic_key().is_none(), "Gboard offers no voice on a password");
    assert_eq!(panel_height(), plain, "and no toolbar for it");
    assert_eq!(
        osk::target_panel_height(PHONE.width),
        plain,
        "the layout's height agrees"
    );
}

#[test]
fn the_provider_is_prepared_when_the_keyboard_opens() {
    let (mut rig, voice, _) = dictation_rig();
    assert_eq!(voice.calls(), vec!["prepare"], "before any tap");
    rig.paint();
    rig.paint();
    assert_eq!(voice.count("prepare"), 1, "once per opening, not per frame");

    // Closed and opened again: prepared again (a failed download retries).
    rig.tap(key_where(|a| matches!(a, KeyAction::Dismiss)).rect);
    assert!(!rig.app.has_focus());
    rig.app.on_key_down(Key::Tab, Modifiers::default());
    assert_eq!(voice.count("prepare"), 2);
}

#[test]
fn a_provider_registered_while_the_keyboard_is_up_is_prepared_at_once() {
    let (field, _) = bound_field("");
    let mut rig = Rig::new(field, InputProfile::MobileAndroid, None);
    let voice = FakeVoice::new(VoiceState::Ready);
    osk::set_voice_input(Some(Box::new(voice.clone())));
    assert_eq!(voice.calls(), vec!["prepare"]);
    rig.paint();
    assert!(mic_key().is_some());
}

#[test]
fn the_field_stays_above_the_keyboard_when_the_toolbar_appears() {
    // A field at the very bottom of the screen, lifted above the keyboard.
    let mut col = FlexColumn::new();
    col.push(
        Box::new(SizedBox::new().with_height(PHONE.height - 40.0)),
        0.0,
    );
    col.push(Box::new(TextField::new(font())), 0.0);
    let mut rig = Rig::new(Box::new(col), InputProfile::MobileAndroid, None);
    let before = crate::widget::keyboard_scroll::lift_target_for_test();
    assert!(before > 0.0, "the field is lifted");

    let voice = FakeVoice::new(VoiceState::Ready);
    osk::set_voice_input(Some(Box::new(voice)));
    rig.paint();
    let after = crate::widget::keyboard_scroll::lift_target_for_test();
    assert!(
        (after - (before + 43.0)).abs() < 1e-6,
        "lifted by the toolbar's height too: {before} -> {after}"
    );
}

#[test]
fn text_widgets_report_their_committed_text_before_the_caret() {
    use crate::widgets::rich_text::{uniform_resolver, Block, DocPos, RichDoc, RichTextEdit};
    let doc = RichDoc::from_blocks(vec![Block::plain("First"), Block::plain("second")]);
    let mut rich = RichTextEdit::new(doc, uniform_resolver(font()));
    rich.set_caret(DocPos::new(1, 3));
    assert_eq!(rich.text_before_caret().as_deref(), Some("First\nsec"));

    // A text field's composition is not committed text.
    let mut field = TextField::new(font()).with_text("ab");
    field.set_cursor_position(1);
    field.on_event(&crate::Event::FocusGained);
    field.on_event(&crate::Event::ImePreedit {
        text: "zz".into(),
        cursor: None,
    });
    assert_eq!(field.text_before_caret().as_deref(), Some("a"));
    assert_eq!(field.text_input_value().as_deref(), Some("ab"));

    // Nor is a text area's, though its buffer holds it.
    let mut area = crate::widgets::TextArea::new(font()).with_text("ab");
    area.on_event(&crate::Event::FocusGained);
    area.on_event(&crate::Event::ImePreedit {
        text: "zz".into(),
        cursor: None,
    });
    assert_eq!(area.text_input_value().as_deref(), Some("ab"));
    assert_eq!(area.text(), "abzz", "the buffer holds it");
    // The caret sits at the end of new text, where the composition went.
    assert_eq!(area.text_before_caret().as_deref(), Some("ab"));
}

#[test]
fn sentence_start_follows_the_text_before_the_caret() {
    use crate::widgets::on_screen_keyboard::is_sentence_start;
    for (text, start) in [
        ("", true),
        ("   ", true),
        ("Done.", true),
        ("Done. ", true),
        ("Really?", true),
        ("Wow!", true),
        ("first line\n", true),
        ("first line\r\n", true),
        ("Done.\u{a0}", true),
        ("Done.\u{3000}", true),
        ("Say\u{a0}", false),
        ("Say", false),
        ("Say ", false),
        ("e.g", false),
    ] {
        assert_eq!(is_sentence_start(text), start, "{text:?}");
    }
}
