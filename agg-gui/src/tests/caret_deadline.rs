//! Host-loop wakeup aggregation tests for the caret-blink deadline.
//!
//! The blink cadence is only useful if the *host event loop* is told when to
//! wake. Two channels feed that decision and both are exercised here against a
//! real [`App`] tree (not a bare widget):
//!
//! * `App::next_draw_deadline` must surface a focused editor's next 500 ms flip
//!   boundary — walked up through the container tree — so the native shell can
//!   arm `ControlFlow::WaitUntil(t)`. An unfocused tree schedules no wake.
//! * The loop-facing accessor reads the `animation` scheduled-draw channel
//!   *non-destructively* and re-arms cleanly across successive boundaries, so a
//!   shell that reads the deadline every idle iteration keeps arming the same
//!   pending wake (the reactive-host lost-wakeup fix). The channel drains only
//!   when the deadline comes due (surfaced via `wants_draw`, or served by the
//!   paint that clears it) or on a reset; a paint before it is due keeps it.

use super::*;
use crate::text::Font;
use std::sync::Arc;
use std::time::Duration;

/// A focused editor nested inside a normal container tree must surface its
/// blink deadline all the way up at `App::next_draw_deadline` — the value the
/// native winit shell turns into `ControlFlow::WaitUntil`. Without this the
/// loop never wakes on its own and the caret only advances on input events.
#[test]
fn focused_editor_deadline_reaches_app_root() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    const FIELD_ID: u64 = 5150;
    // Nest the field a couple of containers deep so the test exercises the
    // default child-walk propagation, not a direct root child.
    let mut inner = Container::new().with_padding(2.0);
    inner.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_focus_id(FIELD_ID),
    ));
    let mut root = Container::new().with_padding(6.0);
    root.children_mut().push(Box::new(inner));

    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 120.0));

    // Nothing focused yet: no scheduled wake anywhere in the tree.
    assert!(
        app.next_draw_deadline().is_none(),
        "an unfocused tree must not schedule a blink wake"
    );

    // Focus the field the way a Tab / programmatic focus would.
    crate::focus::request_focus(FIELD_ID);
    app.layout(Size::new(200.0, 120.0));
    assert_eq!(
        app.focused_widget_type_name(),
        Some("TextField"),
        "the field should now hold focus"
    );

    // The blink deadline must now be visible at the App root, within one
    // 500 ms interval of now.
    let now = crate::clock::now();
    let deadline = app
        .next_draw_deadline()
        .expect("a focused editor must surface its blink deadline at the App root");
    let remaining = deadline.saturating_duration_since(now);
    assert!(
        remaining > Duration::ZERO && remaining <= Duration::from_millis(500),
        "aggregated deadline should be within one blink interval, got {remaining:?}"
    );

    // The deadline is derived from the field's stable focus anchor, not from
    // per-frame draw flags: clearing the frame's draw request (what the host
    // does after every paint) must NOT drop it, so the loop keeps re-arming
    // `WaitUntil` across successive idle frames.
    crate::animation::clear_draw_request();
    assert!(
        app.next_draw_deadline().is_some(),
        "the blink deadline must persist across a cleared frame so the loop re-arms"
    );
}

/// The loop-facing accessor reads the `animation` scheduled-draw channel
/// **non-destructively** and re-arms cleanly across boundaries. A shell reads
/// `next_draw_deadline()` every idle iteration; successive reads must report
/// the SAME pending deadline (so an intervening non-repainting event cannot
/// strand the wake), and the channel drains only on a frame clear.
#[test]
fn loop_accessor_is_nondestructive_and_rearms_scheduled_channel() {
    // A plain (unfocused) tree contributes no widget deadline, isolating the
    // animation scheduled-draw channel that `App::next_draw_deadline` merges.
    let root = Container::new().with_padding(4.0);
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(100.0, 100.0));

    crate::animation::clear_draw_request();
    assert!(
        app.next_draw_deadline().is_none(),
        "no scheduled draw after a clear"
    );

    // A boundary armed (as a widget would during paint).
    crate::animation::request_draw_after(Duration::from_millis(500));
    let first = app
        .next_draw_deadline()
        .expect("scheduled draw should be surfaced");

    // Reading is non-destructive: a second idle read still sees the SAME
    // pending deadline, so the host re-arms `WaitUntil` idempotently and can
    // never lose the scheduled wake (the reactive-host lost-wakeup fix).
    let again = app
        .next_draw_deadline()
        .expect("the scheduled deadline must persist across reads (lost-wakeup fix)");
    assert_eq!(
        first, again,
        "successive idle reads report the same pending deadline"
    );

    // A frame clear (start of paint) before the deadline is due keeps it:
    // a deadline armed outside paint has nobody to re-arm it.
    crate::animation::clear_draw_request();
    assert_eq!(
        app.next_draw_deadline(),
        Some(first),
        "a frame before the deadline is due keeps it"
    );
    // A reset drains the channel; the next frame re-arms the following
    // boundary and the accessor serves it fresh.
    crate::animation::reset_draw_signals();
    assert!(
        app.next_draw_deadline().is_none(),
        "a reset drains the scheduled channel"
    );
    crate::animation::request_draw_after(Duration::from_millis(500));
    let second = app
        .next_draw_deadline()
        .expect("re-armed scheduled draw should be surfaced");
    assert!(
        second >= first,
        "the re-armed deadline should be at or after the first"
    );
}

/// A deadline armed before a frame (the idle queue's drain does so for its
/// next delayed action, just before the shell paints) is still pending after
/// `App::paint`, so the host's next idle read wakes for it.
#[test]
fn a_deadline_armed_before_paint_survives_paint() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut app = App::new(Box::new(Container::new().with_padding(4.0)));
    app.layout(Size::new(100.0, 100.0));
    crate::animation::reset_draw_signals();
    crate::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
    crate::ui_thread::run_on_idle_after(Duration::from_millis(250), || {});
    // The tick's drain: nothing due yet, so it arms a timed draw.
    crate::ui_thread::invoke_pending_actions();
    let armed = app
        .next_draw_deadline()
        .expect("the drain arms a timed draw");
    let mut fb = crate::framebuffer::Framebuffer::new(100, 100);
    let mut ctx = crate::gfx_ctx::GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    assert_eq!(
        app.next_draw_deadline(),
        Some(armed),
        "painting must not discard the deadline"
    );
    crate::ui_thread::reset_for_tests();
    crate::animation::reset_draw_signals();
}
