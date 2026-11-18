//! `animation::last_input_at` through the real `App` entry points the
//! platform shells call, on the virtual UI clock:
//!
//! 1. Every kind of user input records the UI clock's time as it arrives.
//! 2. Events a platform sends unprompted are not input: the held modifier
//!    set repeated unchanged (an empty one as the window loses focus), empty
//!    committed text, and the empty composition a shell sends when the input
//!    method is switched off with no composition showing.
//! 3. Nor is what the framework does by itself — draw requests, scheduled
//!    draws, layout, paint, a cursor re-evaluation, window activation: it
//!    leaves the record where the last input put it, so a host waiting for
//!    quiet input is never kept waiting by its own redraws.
//! 4. A thread that has had no input reports none.
//!
//! Each test runs on its own thread, so the per-thread record and clock
//! start fresh.

use std::path::PathBuf;
use std::time::Duration;

use agg_gui::animation::{
    last_input_at, note_user_input, request_draw, request_draw_after, schedule_draw_after,
};
use agg_gui::{
    clock, App, DrawCtx, DroppedFileData, Event, EventResult, Framebuffer, GfxCtx, Key, Modifiers,
    MouseButton, Rect, Size, TouchDeviceId, TouchId, TrackpadGesturePhase, Widget,
};

/// Inert leaf filling the window; it requests nothing on its own.
struct Leaf {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Leaf {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

const VIEWPORT: Size = Size {
    width: 64.0,
    height: 64.0,
};

fn app() -> App {
    let mut app = App::new(Box::new(Leaf {
        bounds: Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height),
        children: Vec::new(),
    }));
    app.layout(VIEWPORT);
    app
}

fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// Each input kind, by name, as the shells deliver it.
type Input = (&'static str, fn(&mut App));

const INPUTS: &[Input] = &[
    ("mouse move", |a| a.on_mouse_move(10.0, 10.0)),
    ("mouse move with modifiers", |a| {
        a.on_mouse_move_mods(11.0, 10.0, Modifiers::default())
    }),
    ("mouse down", |a| {
        a.on_mouse_down(10.0, 10.0, MouseButton::Left, Modifiers::default())
    }),
    ("mouse up", |a| {
        a.on_mouse_up(10.0, 10.0, MouseButton::Left, Modifiers::default())
    }),
    ("mouse down with clicks", |a| {
        a.on_mouse_down_clicks(10.0, 10.0, MouseButton::Right, Modifiers::default(), 2)
    }),
    ("mouse leave", |a| a.on_mouse_leave()),
    ("wheel", |a| a.on_mouse_wheel(10.0, 10.0, 1.0)),
    ("horizontal wheel", |a| {
        a.on_mouse_wheel_xy(10.0, 10.0, 1.0, 0.0)
    }),
    ("trackpad pinch", |a| {
        a.on_trackpad_pinch(10.0, 10.0, 0.1, Modifiers::default())
    }),
    ("trackpad magnify", |a| {
        a.on_trackpad_magnify(
            10.0,
            10.0,
            0.1,
            TrackpadGesturePhase::Began,
            Modifiers::default(),
        )
    }),
    ("trackpad rotate", |a| {
        a.on_trackpad_rotate(10.0, 10.0, 5.0, TrackpadGesturePhase::Changed)
    }),
    ("modifiers changed", |a| {
        a.on_modifiers_changed(Modifiers {
            shift: true,
            ..Modifiers::default()
        })
    }),
    ("key down", |a| {
        a.on_key_down(Key::Char('a'), Modifiers::default())
    }),
    ("key up", |a| {
        a.on_key_up(Key::Char('a'), Modifiers::default())
    }),
    ("text input", |a| {
        a.on_text_input("a");
    }),
    ("ime preedit", |a| {
        a.on_ime_preedit("あ", Some((3, 3)));
    }),
    ("touch start", |a| {
        a.on_touch_start(TouchDeviceId(1), TouchId(1), 10.0, 10.0, None)
    }),
    ("touch move", |a| {
        a.on_touch_move(TouchDeviceId(1), TouchId(1), 12.0, 10.0, None)
    }),
    ("touch end", |a| {
        a.on_touch_end(TouchDeviceId(1), TouchId(1))
    }),
    ("touch cancel", |a| {
        a.on_touch_cancel(TouchDeviceId(1), TouchId(2))
    }),
    ("file drag hover", |a| {
        a.on_file_drag_hover(10.0, 10.0, vec![PathBuf::from("part.stl")])
    }),
    ("file drag leave", |a| a.on_file_drag_leave()),
    ("file dropped", |a| {
        a.on_file_dropped(10.0, 10.0, vec![PathBuf::from("part.stl")])
    }),
    ("file data dropped", |a| {
        a.on_file_data_dropped(
            10.0,
            10.0,
            vec![DroppedFileData::new("part.stl", vec![0u8; 4])],
        )
    }),
];

#[test]
fn every_input_kind_records_the_ui_clock_time() {
    let _clock = clock::scoped_virtual(None);
    let mut app = app();
    for (name, input) in INPUTS {
        clock::advance(Duration::from_secs(1));
        input(&mut app);
        assert_eq!(
            last_input_at(),
            Some(clock::now()),
            "{name} records the virtual time it arrived at"
        );
    }
}

#[test]
fn the_frameworks_own_work_is_not_input() {
    let _clock = clock::scoped_virtual(None);
    let mut app = app();
    app.on_mouse_move(10.0, 10.0);
    let input_at = clock::now();
    assert_eq!(last_input_at(), Some(input_at));

    clock::advance(Duration::from_secs(5));
    request_draw();
    request_draw_after(Duration::from_millis(10));
    let scheduled = schedule_draw_after(Duration::from_millis(20));
    app.layout(VIEWPORT);
    paint(&mut app);
    clock::advance(Duration::from_millis(30));
    app.layout(VIEWPORT);
    paint(&mut app);
    agg_gui::invalidate_cursor();
    app.refresh_cursor();
    app.on_window_deactivated();
    app.on_window_activated();
    drop(scheduled);

    assert_eq!(
        last_input_at(),
        Some(input_at),
        "redraws, scheduled draws, layout, paint, a cursor refresh and \
         window activation leave the last input where it was"
    );
}

#[test]
fn a_host_can_record_input_of_its_own() {
    let _clock = clock::scoped_virtual(None);
    clock::advance(Duration::from_secs(2));
    note_user_input();
    assert_eq!(last_input_at(), Some(clock::now()));
}

#[test]
fn no_input_reports_none() {
    let _clock = clock::scoped_virtual(None);
    let mut app = app();
    paint(&mut app);
    request_draw();
    assert_eq!(last_input_at(), None, "a thread with no input has none");
}

#[test]
fn only_a_change_in_the_held_modifiers_is_input() {
    let _clock = clock::scoped_virtual(None);
    let mut app = app();
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    app.on_modifiers_changed(shift);
    let changed_at = clock::now();
    assert_eq!(last_input_at(), Some(changed_at), "a change is input");

    clock::advance(Duration::from_secs(1));
    app.on_modifiers_changed(shift);
    assert_eq!(
        last_input_at(),
        Some(changed_at),
        "the same set again is not input"
    );

    clock::advance(Duration::from_secs(1));
    app.on_modifiers_changed(Modifiers::default());
    assert_eq!(last_input_at(), Some(clock::now()), "letting go is input");

    clock::advance(Duration::from_secs(1));
    let released_at = last_input_at();
    // What a window losing focus sends with nothing held.
    app.on_modifiers_changed(Modifiers::default());
    assert_eq!(
        last_input_at(),
        released_at,
        "an empty set repeated is not input"
    );
}

#[test]
fn empty_text_is_not_input() {
    let _clock = clock::scoped_virtual(None);
    let mut app = app();
    app.on_text_input("");
    assert_eq!(last_input_at(), None, "empty committed text");
    app.on_text_input("a");
    assert_eq!(last_input_at(), Some(clock::now()), "committed text");
}

#[test]
fn an_empty_composition_is_input_only_when_it_ends_one_showing() {
    let _clock = clock::scoped_virtual(None);
    let mut app = app();
    // A shell switching the input method off as focus leaves a text field.
    app.on_ime_preedit("", None);
    assert_eq!(last_input_at(), None, "nothing was composing");

    clock::advance(Duration::from_secs(1));
    app.on_ime_preedit("あ", Some((3, 3)));
    assert_eq!(last_input_at(), Some(clock::now()), "composing is input");

    clock::advance(Duration::from_secs(1));
    app.on_ime_preedit("", None);
    let ended_at = clock::now();
    assert_eq!(
        last_input_at(),
        Some(ended_at),
        "ending a showing composition is input"
    );

    clock::advance(Duration::from_secs(1));
    app.on_ime_preedit("", None);
    assert_eq!(
        last_input_at(),
        Some(ended_at),
        "a second empty composition ends nothing"
    );
}
