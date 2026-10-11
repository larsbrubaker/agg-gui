//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ToolTipActivationTests.cs`, as
//! behaviour tests: building the App activates nothing tooltip-related, the
//! first pointer input is what arms the controller, and tearing the
//! controller down is safe and idempotent whether or not it was activated.
//!
//! The C# tests read `ToolTipManager.runningInterval` by reflection: the
//! polling interval the manager schedules on first mouse input, and whose
//! scheduling from the `SystemWindow` constructor was the bug they guard.
//! agg-gui has no interval to schedule — its controller is a thread-local
//! state machine the App drives once per frame — so "an interval is
//! scheduled" becomes "the controller has armed a hover", seen as a tip
//! appearing after the hover delay. `controller::reset` is C#'s
//! `ToolTipManager.Dispose`.
//!
//! `ConcurrentActivationInitializesExactlyOnce` is N/A: it races eight
//! threads through C#'s `Interlocked` initialization latch and counts
//! `MouseMove` subscriptions. agg-gui's tooltip state is thread-local and
//! subscribes to nothing, so there is no latch to race.

use crate::geometry::{Point, Rect};
use crate::layout_props::WidgetBase;
use crate::widgets::tooltip::{
    controller, reset_tooltip_test_state, set_content_factory, tooltip_timings,
};
use crate::{App, Size, Widget};

use super::tool_tip_placement::{Fixed, Restore};

/// A 200×200 window holding one tipped widget filling it.
fn window_app() -> App {
    let mut tipped: Box<dyn Widget> = Box::new(Fixed {
        bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
        children: Vec::new(),
        size: None,
        base: WidgetBase::new(),
    });
    tipped.set_tooltip_text(Some("help".to_string()));
    let mut app = App::new(Box::new(Fixed {
        bounds: Rect::default(),
        children: vec![tipped],
        size: None,
        base: WidgetBase::new(),
    }));
    app.layout(Size::new(200.0, 200.0));
    app
}

fn pin() -> Restore {
    reset_tooltip_test_state();
    controller::reset();
    crate::clock::start_virtual();
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
    // A fixed body, so a tip shows without a system font installed.
    set_content_factory(Some(Box::new(|_text: &str| -> Box<dyn Widget> {
        Box::new(Fixed {
            bounds: Rect::default(),
            children: Vec::new(),
            size: Some(Size::new(80.0, 40.0)),
            base: WidgetBase::new(),
        })
    })));
    Restore
}

fn move_to(app: &mut App, p: Point) {
    app.on_mouse_move(p.x, 200.0 - p.y);
    app.update_tooltips_for_test();
}

#[test]
fn construction_does_not_schedule_tool_tip_interval() {
    let _restore = pin();
    let mut app = window_app();

    // Construction alone (and a frame with no pointer) must not have armed a tip.
    app.update_tooltips_for_test();
    crate::clock::advance(tooltip_timings().initial_delay);
    app.update_tooltips_for_test();
    assert!(!controller::is_visible());
    assert_eq!(controller::current_text(), "");

    // First mouse input activates tooltip tracking.
    let half = tooltip_timings().initial_delay / 2;
    move_to(&mut app, Point::new(5.0, 5.0));
    // Activation is idempotent - further input over the same widget must not
    // re-arm the hover: the tip shows one delay after the first move.
    crate::clock::advance(half);
    move_to(&mut app, Point::new(6.0, 6.0));
    crate::clock::advance(tooltip_timings().initial_delay - half);
    app.update_tooltips_for_test();
    assert_eq!(controller::current_text(), "help");

    // Teardown clears it.
    controller::reset();
    assert!(!controller::is_visible());
    assert_eq!(controller::current_text(), "");

    // Teardown is idempotent.
    controller::reset();
    assert!(!controller::is_visible());
    assert_eq!(controller::current_text(), "");
}

#[test]
fn dispose_is_safe_when_never_activated() {
    let _restore = pin();
    let _app = window_app();

    // Dispose before any activation must not panic and must leave nothing behind.
    controller::reset();
    assert!(!controller::is_visible());

    // And activation gating still holds after an early dispose.
    assert_eq!(controller::current_text(), "");
}
