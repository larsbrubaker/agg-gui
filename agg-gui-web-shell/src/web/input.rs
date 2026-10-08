//! DOM input → [`agg_gui::App`]: canvas pointer (mouse / pen / multi-touch),
//! wheel (a ctrl+wheel is a trackpad pinch), pointer-leave and context-menu
//! listeners, plus the window-level keyboard + clipboard bridge from
//! `agg_gui::web_adapter`. Each DOM event becomes an `agg_gui::shell_input::ForwarderEvent` carrying its own
//! position and modifiers, handed to the shell's forwarder ([`forward`]).
//!
//! Extracted from `demo-wgpu`'s `web_shell` (pointer events, touch pipeline,
//! wheel normaliser, cursor icon) with AtomArtist's additions (pointer-leave
//! clears hover, `buttons` resync for the idle guard). The numeric mapping is
//! in [`crate::dom_math`], unit tested natively.

use agg_gui::shell_input::{ClickCount, ForwarderEvent};
use agg_gui::trackpad_pinch::browser_wheel_notches;
use agg_gui::wheel::WheelDeltaMode;
use agg_gui::TouchPhase;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use super::{forward, sensors};
use super::{frame::input_scale, lifecycle, note_input, note_input_without_repaint};
use crate::dom_math::{
    client_to_physical, modifiers, mouse_button_from_dom, pressed_button_count, PointerKind,
};
use crate::pointer::move_forces_repaint;

/// The single web touchscreen, as far as the multi-touch pipeline is
/// concerned — pointer events don't expose per-digitizer identity.
const TOUCH_DEVICE: agg_gui::TouchDeviceId = agg_gui::TouchDeviceId(0);

/// Physical keyboard (down **and** up) and the copy/cut/paste clipboard
/// bridge. Window-level, so keys reach the app without canvas focus — except
/// while a real DOM editor has focus, which the adapter leaves alone.
pub(super) fn install_keyboard() {
    agg_gui::web_adapter::install_keyboard_listeners(|key, mods, pressed| {
        let modifiers = Some(mods);
        forward(if pressed {
            ForwarderEvent::KeyDown { key, modifiers }
        } else {
            ForwarderEvent::KeyUp { key, modifiers }
        });
        note_input();
    });
}

pub(super) fn add_listener<E>(
    target: &web_sys::EventTarget,
    event: &str,
    handler: impl FnMut(E) + 'static,
) where
    E: wasm_bindgen::convert::FromWasmAbi + 'static,
{
    let cb = Closure::<dyn FnMut(E)>::new(handler);
    if target
        .add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())
        .is_ok()
    {
        // Leaked deliberately: listeners live as long as the page.
        cb.forget();
    }
}

pub(super) fn pos(canvas: &web_sys::HtmlCanvasElement, client_x: i32, client_y: i32) -> (f64, f64) {
    let rect = canvas.get_bounding_client_rect();
    client_to_physical(
        client_x as f64,
        client_y as f64,
        rect.left(),
        rect.top(),
        // The backing store's own scale, not the raw DPR: they differ when
        // the texture limit forced a smaller surface.
        input_scale(),
    )
}

fn event_mods(e: &web_sys::MouseEvent) -> agg_gui::Modifiers {
    modifiers(e.shift_key(), e.ctrl_key(), e.alt_key(), e.meta_key())
}

fn touch_id(e: &web_sys::PointerEvent) -> agg_gui::TouchId {
    agg_gui::TouchId(e.pointer_id() as u64)
}

/// A touch pointer event at canvas position `(x, y)`. The pressure rides
/// along on start and move; end and cancel ignore it.
fn touch(phase: TouchPhase, e: &web_sys::PointerEvent, x: f64, y: f64) -> ForwarderEvent {
    ForwarderEvent::Touch {
        phase,
        device: TOUCH_DEVICE,
        id: touch_id(e),
        x,
        y,
        force: Some(e.pressure()),
    }
}

pub(super) fn install_pointer_listeners(canvas: &web_sys::HtmlCanvasElement) {
    // Touch pointers feed the App's raw-touch entry points (gesture
    // recogniser, per-finger registry, primary-finger mouse emulation); only
    // mouse/pen take the direct mouse path — synthesising mouse events for
    // touch here would double-fire them. `touch-action: none` stops the
    // browser panning/zooming the page out from under the app.
    let _ = canvas.style().set_property("touch-action", "none");
    let target: &web_sys::EventTarget = canvas.as_ref();

    {
        let c = canvas.clone();
        add_listener(target, "pointermove", move |e: web_sys::PointerEvent| {
            let (x, y) = pos(&c, e.client_x(), e.client_y());
            let kind = PointerKind::from_dom(&e.pointer_type());
            if kind == PointerKind::Touch {
                forward(touch(TouchPhase::Move, &e, x, y));
            } else {
                // Self-healing idle guard: re-derive held buttons from the event.
                lifecycle::sync_buttons(pressed_button_count(e.buttons()));
                // Mouse events carry modifier state; feeding it here keeps
                // `current_modifiers()` exact during drags and delivers a
                // mid-drag Shift change to the captured widget.
                let mods = event_mods(&e);
                forward(ForwarderEvent::MouseMove {
                    x,
                    y,
                    modifiers: Some(mods),
                });
                // Reflect the hovered widget's preferred cursor on the canvas.
                let icon = agg_gui::current_cursor_icon();
                let _ = c.style().set_property("cursor", icon.to_css());
            }
            if move_forces_repaint(kind) {
                note_input();
            } else {
                // Hover that matters invalidates a widget → `wants_draw()`.
                note_input_without_repaint();
            }
        });
    }
    {
        let c = canvas.clone();
        add_listener(target, "pointerdown", move |e: web_sys::PointerEvent| {
            // Capture so drags keep reporting positions outside the canvas.
            let _ = c.set_pointer_capture(e.pointer_id());
            // A pointerdown is a user gesture — when iOS grants tilt access.
            sensors::service_tilt_permission_gesture();
            let (x, y) = pos(&c, e.client_x(), e.client_y());
            if PointerKind::from_dom(&e.pointer_type()) == PointerKind::Touch {
                e.prevent_default();
                lifecycle::touch_down(e.pointer_id());
                forward(touch(TouchPhase::Start, &e, x, y));
            } else {
                lifecycle::sync_buttons(pressed_button_count(e.buttons()));
                let button = mouse_button_from_dom(e.button());
                let mods = event_mods(&e);
                forward(ForwarderEvent::MouseDown {
                    at: Some((x, y)),
                    button,
                    modifiers: Some(mods),
                    clicks: ClickCount::Auto,
                });
                // A press can claim a drag cursor before any move arrives.
                let icon = agg_gui::current_cursor_icon();
                let _ = c.style().set_property("cursor", icon.to_css());
            }
            note_input();
        });
    }
    for (event, cancel) in [("pointerup", false), ("pointercancel", true)] {
        let c = canvas.clone();
        add_listener(target, event, move |e: web_sys::PointerEvent| {
            let (x, y) = pos(&c, e.client_x(), e.client_y());
            if PointerKind::from_dom(&e.pointer_type()) == PointerKind::Touch {
                lifecycle::touch_up(e.pointer_id());
                let phase = if cancel {
                    TouchPhase::Cancel
                } else {
                    TouchPhase::End
                };
                forward(touch(phase, &e, x, y));
            } else {
                lifecycle::sync_buttons(pressed_button_count(e.buttons()));
                let button = mouse_button_from_dom(e.button());
                let mods = event_mods(&e);
                forward(ForwarderEvent::MouseUp {
                    at: Some((x, y)),
                    button,
                    modifiers: Some(mods),
                });
                // Release re-resolves hover at the release point, so a drag
                // cursor (splitter arrows) drops without waiting for a move.
                let icon = agg_gui::current_cursor_icon();
                let _ = c.style().set_property("cursor", icon.to_css());
            }
            note_input();
        });
    }
    add_listener(target, "pointerleave", move |e: web_sys::PointerEvent| {
        // No further move arrives to clear hover state, so a latched hover
        // (and its tooltip) would outlive a fast flick off the canvas.
        // Touch pointers "leave" on every lift — nothing to clear there.
        if PointerKind::from_dom(&e.pointer_type()) != PointerKind::Touch {
            forward(ForwarderEvent::MouseLeave);
            note_input();
        }
    });
    {
        let c = canvas.clone();
        // DOM deltas → agg-gui notches (fractional for precision devices).
        add_listener(target, "wheel", move |e: web_sys::WheelEvent| {
            e.prevent_default();
            let (x, y) = pos(&c, e.client_x(), e.client_y());
            // DOM deltaY is positive-scroll-DOWN; App wants positive =
            // wheel rotated forward (winit convention). A ctrl+wheel is a
            // trackpad pinch and zooms as the native pinch does.
            let (dx, dy) = browser_wheel_notches(
                e.delta_x(),
                e.delta_y(),
                WheelDeltaMode::from_dom(e.delta_mode()),
                e.ctrl_key(),
            );
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            let mods = event_mods(&e);
            forward(ForwarderEvent::Wheel {
                at: Some((x, y)),
                delta_x: dx,
                delta_y: dy,
                modifiers: Some(mods),
            });
            note_input();
        });
    }
    add_listener(target, "contextmenu", |e: web_sys::Event| {
        e.prevent_default()
    });
}
