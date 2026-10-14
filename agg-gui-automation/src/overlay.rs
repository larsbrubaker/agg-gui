//! `RenderMouse`: the simulated pointer drawn over a live automation window,
//! the port of agg-sharp `AutomationRunner.RenderMouse` (drawn there from the
//! window's `AfterDraw` by `AggInputMethods` while `DrawSimulatedMouse` is
//! set).
//!
//! It is a watcher's aid: someone looking at a run sees where the test's
//! pointer is, whether the left button is held, which of Shift and Control
//! are held, and the click count of a multi-click. The live driver
//! (`driver/live.rs`) draws it on presented frames only, through the shell's
//! presented-only overlay pass, so it never reaches `get_current_screen` and
//! image searches never see it. Headless runs have no watcher and draw
//! nothing.

use std::sync::Arc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::shell_input::InputForwarder;
use agg_gui::text::Font;
use agg_gui::{Color, Modifiers, MouseButton};

use crate::keys::Keys;

/// C#'s circle radius, in window units.
pub const MOUSE_RADIUS: f64 = 10.0;

/// C#'s label size, in window units.
pub const LABEL_SIZE: f64 = 8.0;

/// What the overlay shows of the simulated pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SimulatedMouse {
    /// The pointer in physical pixels, Y down (the forwarder's cursor).
    pub x: f64,
    pub y: f64,
    /// C# `inputSystem.LeftButtonDown`.
    pub left_button_down: bool,
    /// The held modifiers.
    pub modifiers: Modifiers,
    /// C# `inputSystem.ClickCount`: the most recent press's count.
    pub click_count: u32,
}

impl SimulatedMouse {
    /// The pointer as a window's input forwarder tracks it; simulated input
    /// goes through the forwarder, so this is the runner's pointer.
    pub fn from_forwarder(forwarder: &InputForwarder) -> Self {
        let (x, y) = forwarder.cursor();
        Self {
            x,
            y,
            left_button_down: forwarder.is_button_down(MouseButton::Left),
            modifiers: forwarder.modifiers(),
            click_count: forwarder.click_count(),
        }
    }
}

/// The label drawn in the filled circle while the left button is down: "S"
/// for a held Shift, "C" for a held Control (the command modifier, which is
/// what the runner's `^` holds), then the click count when above 1. Empty
/// means no label.
pub fn mouse_label(mouse: &SimulatedMouse) -> String {
    let command = Keys::CONTROL.to_agg_modifiers();
    let control_held =
        (command.ctrl && mouse.modifiers.ctrl) || (command.meta && mouse.modifiers.meta);
    let mut label = String::new();
    if mouse.modifiers.shift {
        label.push('S');
    }
    if control_held {
        label.push('C');
    }
    if mouse.click_count > 1 {
        label.push_str(&mouse.click_count.to_string());
    }
    label
}

/// C# `RenderMouse`: a radius-10 circle at the pointer, filled green while
/// the left button is down (with [`mouse_label`] centred in it), stroked
/// black at width 3 and then white at width 2. `ctx` draws in physical
/// pixels, Y up, over a window `height_px` tall; `scale` is the window's
/// device scale, which C#'s window units are multiplied by.
pub fn render_mouse(ctx: &mut dyn DrawCtx, mouse: &SimulatedMouse, height_px: f64, scale: f64) {
    let (cx, cy) = (mouse.x, height_px - mouse.y);
    let radius = MOUSE_RADIUS * scale;
    if mouse.left_button_down {
        ctx.set_fill_color(Color::rgb(0.0, 128.0 / 255.0, 0.0));
        ctx.begin_path();
        ctx.circle(cx, cy, radius);
        ctx.fill();

        let label = mouse_label(mouse);
        if !label.is_empty() {
            draw_label(ctx, &label, cx, cy, LABEL_SIZE * scale);
        }
    }
    for (width, color) in [(3.0, Color::black()), (2.0, Color::white())] {
        ctx.set_stroke_color(color);
        ctx.set_line_width(width * scale);
        ctx.begin_path();
        ctx.circle(cx, cy, radius);
        ctx.stroke();
    }
}

/// The label centred on the pointer, both ways (C# `Justification.Center`,
/// `Baseline.BoundsCenter`), in black (C#'s default text colour).
fn draw_label(ctx: &mut dyn DrawCtx, label: &str, cx: f64, cy: f64, size: f64) {
    ctx.set_font(label_font());
    ctx.set_font_size(size);
    ctx.set_fill_color(Color::black());
    let (width, middle) = ctx
        .measure_text(label)
        .map_or((0.0, 0.0), |m| (m.width, (m.ascent - m.descent) / 2.0));
    ctx.fill_text(label, cx - width / 2.0, cy - middle);
}

/// The system font when one is set, else agg-gui's standard UI font (made
/// once per thread).
fn label_font() -> Arc<Font> {
    thread_local! {
        static STANDARD: Arc<Font> = Arc::new(agg_gui::fonts::standard_ui_font());
    }
    agg_gui::font_settings::current_system_font().unwrap_or_else(|| STANDARD.with(Arc::clone))
}
