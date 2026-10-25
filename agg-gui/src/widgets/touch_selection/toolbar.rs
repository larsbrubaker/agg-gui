//! The floating edit toolbar of [`super::TouchSelection`]: the horizontal
//! row of Cut / Copy / Paste / Select all that Android and iOS float above a
//! selection made by touch, in place of the right-click menu.
//!
//! It offers the text context menu's actions
//! ([`TextMenuAction`](crate::widgets::text_context_menu::TextMenuAction)),
//! which the host widget runs through the same verbs as its menu and its
//! shortcuts; only the actions that apply are shown, as on both platforms.
//! Android draws a pill; iOS a rounded rectangle with hairlines between the
//! buttons. The layout is in the host widget's local, Y-up coordinates and is
//! kept on screen with the root origin the host recorded at its last paint.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};
use crate::text::{measure_advance, Font};
use crate::widgets::text_context_menu::TextMenuAction;

use super::handles::HandleStyle;

/// Label size of the toolbar's buttons (logical points).
pub(crate) const FONT_SIZE: f64 = 14.0;

/// Space between the selection and the toolbar (logical points).
const GAP: f64 = 8.0;

/// Padding either side of each button's label (logical points).
const BUTTON_PAD: f64 = 14.0;

/// Margin kept between the toolbar and the window's edges.
const EDGE_MARGIN: f64 = 4.0;

/// What the toolbar offers given the host's state, in the platforms' order.
/// `copyable` is a selection that may be copied (not a password's);
/// `can_paste` that the clipboard holds text.
pub(crate) fn actions(
    copyable: bool,
    editable: bool,
    can_paste: bool,
    has_text: bool,
    all_selected: bool,
) -> Vec<TextMenuAction> {
    let mut out = Vec::new();
    if copyable && editable {
        out.push(TextMenuAction::Cut);
    }
    if copyable {
        out.push(TextMenuAction::Copy);
    }
    if editable && can_paste {
        out.push(TextMenuAction::Paste);
    }
    if has_text && !all_selected {
        out.push(TextMenuAction::SelectAll);
    }
    out
}

/// The button label of `action` on this platform.
pub(crate) fn label(action: TextMenuAction, style: HandleStyle) -> &'static str {
    match action {
        TextMenuAction::Cut => "Cut",
        TextMenuAction::Copy => "Copy",
        TextMenuAction::Paste => "Paste",
        TextMenuAction::SelectAll => match style {
            HandleStyle::Lollipop => "Select All",
            HandleStyle::Teardrop => "Select all",
        },
    }
}

/// A laid-out toolbar (local, Y-up).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolbarLayout {
    pub rect: Rect,
    pub buttons: Vec<(TextMenuAction, Rect)>,
}

impl ToolbarLayout {
    /// The button under `pos`, if any.
    pub(crate) fn button_at(&self, pos: Point) -> Option<TextMenuAction> {
        self.buttons
            .iter()
            .find(|(_, r)| contains(*r, pos))
            .map(|(a, _)| *a)
    }

    pub(crate) fn contains(&self, pos: Point) -> bool {
        contains(self.rect, pos)
    }
}

pub(crate) fn contains(r: Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

/// Where the selection the toolbar floats over is (local, Y-up): the middle
/// of its span, the top of its first line and the bottom of its last.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub center_x: f64,
    pub top: f64,
    pub bottom: f64,
}

/// Lay out the toolbar over `anchor`: centred on it and [`GAP`] above its
/// top (and above iOS's start knob), or below the handles when that would
/// leave `room`, and kept inside `room` both ways (below its top, above its
/// bottom, between its sides). `origin` is the host's local origin in root
/// logical space; `room` is the visible window above the on-screen
/// keyboard, in root logical space.
pub(crate) fn layout(
    actions: &[TextMenuAction],
    style: HandleStyle,
    font: &Arc<Font>,
    anchor: Anchor,
    origin: Point,
    room: Rect,
) -> Option<ToolbarLayout> {
    if actions.is_empty() {
        return None;
    }
    let h = super::handles::hit_size();
    let widths: Vec<f64> = actions
        .iter()
        .map(|a| measure_advance(font, label(*a, style), FONT_SIZE) + BUTTON_PAD * 2.0)
        .collect();
    let w: f64 = widths.iter().sum::<f64>() + h * 0.25;
    let mut x = anchor.center_x - w * 0.5;
    // iOS's start knob stands above the line: clear it too.
    let knob = match style {
        HandleStyle::Lollipop => super::handles::KNOB_RADIUS * 2.0,
        HandleStyle::Teardrop => 0.0,
    };
    let mut y = anchor.top + GAP + knob;
    // Root-space limits, mapped into the host's local space.
    let top_limit = room.y + room.height - origin.y - EDGE_MARGIN;
    let bottom_limit = room.y - origin.y + EDGE_MARGIN;
    if y + h > top_limit {
        y = anchor.bottom - super::handles::drop_below_line(style) - GAP - h;
    }
    if y < bottom_limit {
        y = bottom_limit;
    }
    if y + h > top_limit {
        y = top_limit - h;
    }
    let left_limit = room.x - origin.x + EDGE_MARGIN;
    let right_limit = room.x + room.width - origin.x - EDGE_MARGIN;
    if x + w > right_limit {
        x = right_limit - w;
    }
    if x < left_limit {
        x = left_limit;
    }
    let rect = Rect::new(x, y, w, h);
    let mut bx = x + h * 0.125;
    let buttons = actions
        .iter()
        .zip(widths)
        .map(|(a, bw)| {
            let r = Rect::new(bx, y, bw, h);
            bx += bw;
            (*a, r)
        })
        .collect();
    Some(ToolbarLayout { rect, buttons })
}

/// Draw the toolbar: a shadowed pill (Android) or rounded rectangle with
/// hairline separators (iOS), with the labels centred in their buttons.
pub(crate) fn paint(
    ctx: &mut dyn DrawCtx,
    layout: &ToolbarLayout,
    style: HandleStyle,
    font: &Arc<Font>,
) {
    let v = ctx.visuals();
    let r = layout.rect;
    let radius = match style {
        HandleStyle::Teardrop => r.height * 0.5,
        HandleStyle::Lollipop => 8.0,
    };
    ctx.set_fill_color(v.window_shadow);
    ctx.begin_path();
    ctx.rounded_rect(r.x, r.y - 2.0, r.width, r.height, radius);
    ctx.fill();
    ctx.set_fill_color(v.window_fill);
    ctx.begin_path();
    ctx.rounded_rect(r.x, r.y, r.width, r.height, radius);
    ctx.fill();
    ctx.set_stroke_color(v.window_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(r.x + 0.5, r.y + 0.5, r.width - 1.0, r.height - 1.0, radius);
    ctx.stroke();
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(FONT_SIZE);
    let m = ctx.measure_text("Ag").unwrap_or_default();
    for (i, (action, b)) in layout.buttons.iter().enumerate() {
        if style == HandleStyle::Lollipop && i > 0 {
            ctx.set_fill_color(v.separator);
            ctx.begin_path();
            ctx.rect(b.x, b.y + b.height * 0.2, 1.0, b.height * 0.6);
            ctx.fill();
        }
        let text = label(*action, style);
        let tw = measure_advance(font, text, FONT_SIZE);
        // `descent` is positive: the ink spans baseline - descent .. + ascent.
        let baseline = b.y + b.height * 0.5 - (m.ascent - m.descent) * 0.5;
        ctx.set_fill_color(v.text_color);
        ctx.fill_text(text, b.x + (b.width - tw) * 0.5, baseline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_actions_that_apply_are_offered() {
        use TextMenuAction::*;
        assert_eq!(
            actions(true, true, true, true, false),
            vec![Cut, Copy, Paste, SelectAll]
        );
        assert_eq!(
            actions(false, true, true, true, false),
            vec![Paste, SelectAll]
        );
        assert_eq!(
            actions(true, false, true, true, false),
            vec![Copy, SelectAll]
        );
        assert_eq!(
            actions(true, true, true, true, true),
            vec![Cut, Copy, Paste]
        );
        assert_eq!(actions(false, true, true, false, false), vec![Paste]);
        assert_eq!(
            actions(true, true, false, true, false),
            vec![Cut, Copy, SelectAll]
        );
        assert!(actions(false, false, false, false, false).is_empty());
    }

    fn font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(BYTES).expect("font"))
    }

    /// Lay out the full toolbar for a selection spanning `top`..`bottom`
    /// (local) in a host at the root origin, inside `room`.
    fn place(center_x: f64, top: f64, bottom: f64, room: Rect) -> Rect {
        let _profile = crate::input_profile::profile_test_lock();
        crate::ux_scale::set_ux_scale(1.0);
        let all = [
            TextMenuAction::Cut,
            TextMenuAction::Copy,
            TextMenuAction::Paste,
            TextMenuAction::SelectAll,
        ];
        let anchor = Anchor {
            center_x,
            top,
            bottom,
        };
        layout(
            &all,
            HandleStyle::Teardrop,
            &font(),
            anchor,
            Point::ORIGIN,
            room,
        )
        .expect("laid out")
        .rect
    }

    #[test]
    fn the_toolbar_sits_above_or_flips_below_and_never_leaves_the_room() {
        let room = Rect::new(0.0, 0.0, 412.0, 800.0);
        let above = place(200.0, 400.0, 380.0, room);
        assert!(above.y >= 400.0, "above the selection: {above:?}");
        let below = place(200.0, 790.0, 770.0, room);
        assert!(below.y + below.height < 770.0, "flipped below: {below:?}");
        // Against the sides.
        for x in [0.0, 412.0] {
            let r = place(x, 400.0, 380.0, room);
            assert!(r.x >= 0.0 && r.x + r.width <= 412.0, "{r:?}");
        }
        // A selection from the top of the screen down into the keyboard
        // (the room starts at the keyboard's top, 300): the flipped toolbar
        // rests on the keyboard instead of going under it.
        let keyboard = Rect::new(0.0, 300.0, 412.0, 500.0);
        let r = place(200.0, 790.0, 100.0, keyboard);
        assert!(r.y >= 300.0 && r.y + r.height <= 800.0, "{r:?}");
    }
}
