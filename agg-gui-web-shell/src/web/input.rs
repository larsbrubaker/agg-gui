//! DOM input → [`agg_gui::App`]: canvas pointer (mouse / pen / multi-touch),
//! wheel (a ctrl+wheel is a trackpad pinch), pointer-leave and context-menu
//! listeners, plus the window-level keyboard + clipboard bridge from
//! `agg_gui::web_adapter` (scoped to the canvas's focus when it shares its
//! page, `WebShellConfig::keyboard_needs_focus`). Each DOM event becomes an `agg_gui::shell_input::ForwarderEvent` carrying its own
//! position and modifiers, handed to the shell's forwarder ([`forward`]).
//!
//! Extracted from `demo-wgpu`'s `web_shell` (pointer events, touch pipeline,
//! wheel normaliser, cursor icon) with AtomArtist's additions (pointer-leave
//! clears hover, `buttons` resync for the idle guard). The numeric mapping is
//! in [`crate::dom_math`], unit tested natively.

use agg_gui::shell_input::{ClickCount, ForwarderEvent};
use agg_gui::trackpad_pinch::browser_wheel_notches_at_ux_scale;
use agg_gui::wheel::WheelDeltaMode;
use agg_gui::TouchPhase;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use super::{forward, sensors};
use super::{frame::input_scale, lifecycle, note_input, note_input_without_repaint};
use crate::dom_math::{
    canvas_tabindex_to_add, client_to_physical, keyboard_belongs_to_app, modifiers,
    mouse_button_from_dom, pressed_button_count, PageFocus, PointerKind,
};
use crate::pointer::move_forces_repaint;

/// The single web touchscreen, as far as the multi-touch pipeline is
/// concerned — pointer events don't expose per-digitizer identity.
const TOUCH_DEVICE: agg_gui::TouchDeviceId = agg_gui::TouchDeviceId(0);

/// Physical keyboard (down **and** up) and the copy/cut/paste clipboard
/// bridge. Window-level, so keys reach the app without canvas focus — except
/// while a real DOM editor has focus, which the adapter leaves alone, and,
/// with `needs_focus` ([`crate::WebShellConfig::keyboard_needs_focus`]),
/// whenever neither the canvas nor the shell's IME input has focus.
pub(super) fn install_keyboard(canvas: &web_sys::HtmlCanvasElement, needs_focus: bool) {
    // A canvas the page made no `tabindex` for can't take focus, so with
    // `needs_focus` it would never get a key.
    if let Some(tabindex) =
        canvas_tabindex_to_add(needs_focus, canvas.get_attribute("tabindex").as_deref())
    {
        let _ = canvas.set_attribute("tabindex", tabindex);
    }
    let canvas = canvas.clone();
    let belongs = move || keyboard_belongs_to_app(needs_focus, page_focus(&canvas));
    agg_gui::web_adapter::install_keyboard_listeners_while(belongs, |key, mods, pressed| {
        let modifiers = Some(mods);
        // A typed character the app now owns; the hidden IME input's `input`
        // event for it must not insert it a second time.
        super::ime::note_key(
            matches!(key, agg_gui::Key::Char(_)) && !mods.ctrl && !mods.meta,
            pressed,
        );
        forward(if pressed {
            ForwarderEvent::KeyDown { key, modifiers }
        } else {
            ForwarderEvent::KeyUp { key, modifiers }
        });
        note_input();
    });
}

/// Where the page's focus is now.
fn page_focus(canvas: &web_sys::HtmlCanvasElement) -> PageFocus {
    let Some(active) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
    else {
        return PageFocus::Elsewhere;
    };
    let canvas: &wasm_bindgen::JsValue = canvas.as_ref();
    if *canvas == wasm_bindgen::JsValue::from(active.clone()) {
        PageFocus::Canvas
    } else if active.has_attribute(agg_gui::web_adapter::IME_INPUT_ATTRIBUTE) {
        PageFocus::ImeInput
    } else {
        PageFocus::Elsewhere
    }
}

/// A press on an embedded canvas takes the keyboard: focus the canvas
/// (without scrolling the page) unless it or the IME input already has it.
/// A mouse press would focus the `tabindex` canvas by itself, but a touch
/// press's `preventDefault()` cancels that.
fn take_focus_on_press(canvas: &web_sys::HtmlCanvasElement) {
    if page_focus(canvas) != PageFocus::Elsewhere {
        return;
    }
    let options = web_sys::FocusOptions::new();
    options.set_prevent_scroll(true);
    let _ = canvas.focus_with_options(&options);
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

pub(super) fn install_pointer_listeners(canvas: &web_sys::HtmlCanvasElement, needs_focus: bool) {
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
                super::sync_cursor(&c);
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
            if needs_focus {
                take_focus_on_press(&c);
            }
            super::ime::note_canvas_press();
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
                super::sync_cursor(&c);
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
                super::sync_cursor(&c);
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
            // CSS pixels are DPR-free; dividing by ux_scale makes them the
            // App's logical units, so a trackpad scrolls with the fingers.
            let (dx, dy) = browser_wheel_notches_at_ux_scale(
                e.delta_x(),
                e.delta_y(),
                WheelDeltaMode::from_dom(e.delta_mode()),
                e.ctrl_key(),
                agg_gui::ux_scale::ux_scale(),
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
