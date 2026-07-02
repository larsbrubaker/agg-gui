//! Anchored info cards — measured, placed, and clamped by the library.
//!
//! A recurring overlay shape: a small rounded card with a title and a few
//! detail lines, floated next to an anchor point (a tapped body, a
//! hovered element, a reticle). Every app that hand-rolls one repeats
//! the same three mistakes:
//!
//! 1. **Guessed text width** (`chars × size × k`) — wrong for anything
//!    non-monospace or non-ASCII, so the card is too tight or too wide.
//! 2. **Clamping against its own widget bounds only** — the card lands
//!    underneath sibling edge chrome (button rails, trays, the on-screen
//!    keyboard) and looks cut off.
//! 3. **No side-flipping** — when the anchor nears an edge the card gets
//!    pushed over the anchor or off-screen instead of flipping sides.
//!
//! This module owns all three: [`measure`] uses the draw context's real
//! text metrics, and [`anchored_rect`] places the card against the
//! viewport-wide safe area ([`crate::overlay_insets`]) with symmetric
//! margins, flipping below/above the anchor as space demands. The
//! convenience wrapper [`paint_anchored`] does measure → place → paint in
//! one call — the pit of success for tap-info overlays.
//!
//! All coordinates are logical, Y-up, relative to the widget doing the
//! painting — pass that widget's size as `container`. When the widget
//! fills the viewport (the common overlay-host case) the safe-area
//! insets line up 1:1; a non-fullscreen host can still use
//! [`anchored_rect_with_insets`] with its own insets.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::Insets;
use crate::text::Font;

/// Visual + spacing parameters for an anchored card. The defaults give a
/// dark tooltip-style card; override the colors to match app chrome.
#[derive(Clone)]
pub struct CardStyle {
    pub title_size: f64,
    pub detail_size: f64,
    pub pad_x: f64,
    pub pad_y: f64,
    pub line_gap: f64,
    pub corner_radius: f64,
    /// Symmetric breathing room kept between the card and the safe-area
    /// boundary on every side.
    pub margin: f64,
    /// Distance kept between the anchor point and the card's near edge.
    pub anchor_clearance: f64,
    pub bg: Color,
    pub border_color: Color,
    pub border_width: f64,
    pub title_color: Color,
    pub detail_color: Color,
}

impl Default for CardStyle {
    fn default() -> Self {
        Self {
            title_size: 14.0,
            detail_size: 11.0,
            pad_x: 12.0,
            pad_y: 9.0,
            line_gap: 4.0,
            corner_radius: 7.0,
            margin: 8.0,
            anchor_clearance: 6.0,
            bg: Color::from_rgba8(15, 20, 38, 230),
            border_color: Color::from_rgba8(120, 140, 180, 180),
            border_width: 1.0,
            title_color: Color::from_rgb8(235, 238, 250),
            detail_color: Color::from_rgb8(200, 205, 225),
        }
    }
}

/// Measure the card needed for `title` + `details` using the context's
/// real text metrics (falls back to a monospace estimate only if the
/// backend can't measure).
pub fn measure(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    style: &CardStyle,
    title: &str,
    details: &[String],
) -> Size {
    ctx.set_font(font);
    let text_w = |ctx: &mut dyn DrawCtx, s: &str, size: f64| -> f64 {
        ctx.set_font_size(size);
        ctx.measure_text(s)
            .map(|m| m.width)
            .unwrap_or_else(|| s.chars().count() as f64 * size * 0.6)
    };

    let mut w = text_w(ctx, title, style.title_size);
    for d in details {
        w = w.max(text_w(ctx, d, style.detail_size));
    }

    let detail_block_h = if details.is_empty() {
        0.0
    } else {
        details.len() as f64 * style.detail_size
            + (details.len() as f64 - 1.0) * style.line_gap
    };
    Size::new(
        w + style.pad_x * 2.0,
        style.title_size + style.line_gap + detail_block_h + style.pad_y * 2.0,
    )
}

/// Place a card of `size` near `anchor` inside `container`, avoiding the
/// frame's [`crate::overlay_insets`]. See [`anchored_rect_with_insets`].
pub fn anchored_rect(container: Size, anchor: Point, size: Size, style: &CardStyle) -> Rect {
    anchored_rect_with_insets(container, anchor, size, style, crate::overlay_insets::current())
}

/// Pure placement: prefer sitting fully below the anchor (Y-up: spanning
/// downward from `anchor.y - clearance`), flip fully above when the
/// bottom lacks room, and otherwise clamp into the safe area on the side
/// with more space. Horizontally the card centres on the anchor and
/// clamps; if it is wider than the safe area it centres so any overflow
/// is symmetric. The result never leaves `container − insets − margin`
/// on any side the card fits.
pub fn anchored_rect_with_insets(
    container: Size,
    anchor: Point,
    size: Size,
    style: &CardStyle,
    insets: Insets,
) -> Rect {
    let m = style.margin;
    let safe_l = insets.left + m;
    let safe_r = container.width - insets.right - m;
    let safe_b = insets.bottom + m;
    let safe_t = container.height - insets.top - m;

    let x = if size.width >= safe_r - safe_l {
        safe_l + (safe_r - safe_l - size.width) / 2.0
    } else {
        (anchor.x - size.width / 2.0).clamp(safe_l, safe_r - size.width)
    };

    let clearance = style.anchor_clearance;
    let below_y = anchor.y - clearance - size.height;
    let above_y = anchor.y + clearance;
    let y = if size.height >= safe_t - safe_b {
        safe_b + (safe_t - safe_b - size.height) / 2.0
    } else if below_y >= safe_b {
        below_y
    } else if above_y + size.height <= safe_t {
        above_y
    } else {
        // Neither side holds the whole card: pin it inside the safe area
        // on the roomier side of the anchor.
        let below_room = anchor.y - safe_b;
        let above_room = safe_t - anchor.y;
        if below_room >= above_room {
            safe_b
        } else {
            safe_t - size.height
        }
    };

    Rect::new(x, y, size.width, size.height)
}

/// Paint the card chrome + text into `rect` (as produced by
/// [`anchored_rect`]).
pub fn paint(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    style: &CardStyle,
    rect: Rect,
    title: &str,
    details: &[String],
) {
    ctx.set_fill_color(style.bg);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, style.corner_radius);
    ctx.fill();
    if style.border_width > 0.0 {
        ctx.set_stroke_color(style.border_color);
        ctx.set_line_width(style.border_width);
        ctx.begin_path();
        ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, style.corner_radius);
        ctx.stroke();
    }

    // Y-up: baselines measured down from the card's top edge so the
    // title reads first.
    ctx.set_font(font);
    ctx.set_fill_color(style.title_color);
    ctx.set_font_size(style.title_size);
    let title_baseline = rect.y + rect.height - style.pad_y - style.title_size * 0.75;
    ctx.fill_text(title, rect.x + style.pad_x, title_baseline);

    ctx.set_fill_color(style.detail_color);
    ctx.set_font_size(style.detail_size);
    for (i, line) in details.iter().enumerate() {
        let dy = (i as f64) * (style.detail_size + style.line_gap);
        let baseline = title_baseline - style.title_size - style.line_gap - dy
            + (style.title_size - style.detail_size) * 0.25;
        ctx.fill_text(line, rect.x + style.pad_x, baseline);
    }
}

/// Greedy word-wrap of one line to `max_w`, using `text_width` to
/// measure candidates. A single word wider than `max_w` stays on its own
/// (overflowing) line — never split mid-word. Pure so the policy is unit
/// testable without a draw context.
fn wrap_with(text_width: &dyn Fn(&str) -> f64, line: &str, max_w: f64) -> Vec<String> {
    if text_width(line) <= max_w {
        return vec![line.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in line.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if !current.is_empty() && text_width(&candidate) > max_w {
            out.push(std::mem::take(&mut current));
            current = word.to_string();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(line.to_string());
    }
    out
}

/// Wrap every detail line so the card fits within `max_card_w` (card
/// width including horizontal padding).
fn wrap_details(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    style: &CardStyle,
    details: &[String],
    max_card_w: f64,
) -> Vec<String> {
    ctx.set_font(font);
    ctx.set_font_size(style.detail_size);
    let max_text_w = (max_card_w - style.pad_x * 2.0).max(1.0);
    let size = style.detail_size;
    let width = |s: &str| -> f64 {
        ctx.measure_text(s)
            .map(|m| m.width)
            .unwrap_or_else(|| s.chars().count() as f64 * size * 0.6)
    };
    let mut wrapped = Vec::new();
    for line in details {
        wrapped.extend(wrap_with(&width, line, max_text_w));
    }
    wrapped
}

/// Measure → wrap → place → paint in one call; returns the rect painted
/// so the caller can hit-test or decorate it. This is the intended entry
/// point: detail lines wrap to the safe width, so even a narrow phone
/// viewport with a wide reserved rail produces a taller card instead of
/// one that pokes underneath the chrome. A clipped or edge-hugging card
/// now requires deliberate effort rather than being the accidental
/// default.
///
/// The frame's [`crate::overlay_insets`] are converted into the painting
/// widget's local space through the context transform
/// ([`crate::overlay_insets::for_paint_ctx`]), so this is correct whether
/// or not the widget fills the viewport. `extra_insets` is merged in
/// per-edge max — pass the widget's own reserved strips (e.g. an
/// in-widget ruler) or `Insets::default()`.
pub fn paint_anchored(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    style: &CardStyle,
    container: Size,
    anchor: Point,
    extra_insets: Insets,
    title: &str,
    details: &[String],
) -> Rect {
    let frame = crate::overlay_insets::for_paint_ctx(ctx, container);
    let insets = Insets {
        left: frame.left.max(extra_insets.left),
        right: frame.right.max(extra_insets.right),
        top: frame.top.max(extra_insets.top),
        bottom: frame.bottom.max(extra_insets.bottom),
    };
    let safe_w = container.width - insets.left - insets.right - style.margin * 2.0;
    let details = wrap_details(ctx, Arc::clone(&font), style, details, safe_w);
    let size = measure(ctx, Arc::clone(&font), style, title, &details);
    let rect = anchored_rect_with_insets(container, anchor, size, style, insets);
    paint(ctx, font, style, rect, title, &details);
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> CardStyle {
        CardStyle {
            margin: 8.0,
            anchor_clearance: 6.0,
            ..CardStyle::default()
        }
    }

    const CONTAINER: Size = Size {
        width: 240.0,
        height: 500.0,
    };
    const CARD: Size = Size {
        width: 180.0,
        height: 60.0,
    };

    #[test]
    fn sits_below_anchor_with_room() {
        let r = anchored_rect_with_insets(
            CONTAINER,
            Point { x: 120.0, y: 300.0 },
            CARD,
            &style(),
            Insets::default(),
        );
        assert_eq!(r.y, 300.0 - 6.0 - 60.0);
        assert_eq!(r.x, 120.0 - 90.0, "centred on the anchor");
    }

    #[test]
    fn flips_above_when_bottom_is_reserved() {
        // A keyboard-sized bottom reservation eats the space below.
        let ins = Insets {
            bottom: 260.0,
            ..Insets::default()
        };
        let r = anchored_rect_with_insets(
            CONTAINER,
            Point { x: 120.0, y: 300.0 },
            CARD,
            &style(),
            ins,
        );
        assert_eq!(r.y, 300.0 + 6.0, "flipped fully above the anchor");
        assert!(r.y >= 260.0 + 8.0, "clear of the reserved strip");
    }

    #[test]
    fn left_rail_reservation_pushes_card_right() {
        let ins = Insets {
            left: 56.0,
            ..Insets::default()
        };
        let r = anchored_rect_with_insets(
            CONTAINER,
            Point { x: 10.0, y: 300.0 },
            Size {
                width: 120.0,
                height: 60.0,
            },
            &style(),
            ins,
        );
        assert_eq!(r.x, 56.0 + 8.0, "left edge clears rail + margin");
    }

    #[test]
    fn wider_than_safe_area_overflows_symmetrically() {
        let r = anchored_rect_with_insets(
            Size {
                width: 150.0,
                height: 500.0,
            },
            Point { x: 20.0, y: 300.0 },
            CARD, // 180 wide > 150 - 16 safe
            &style(),
            Insets::default(),
        );
        let overflow_left = 8.0 - r.x;
        let overflow_right = (r.x + 180.0) - 142.0;
        assert!(
            (overflow_left - overflow_right).abs() < 1e-9,
            "overflow must be symmetric: {overflow_left} vs {overflow_right}"
        );
    }

    #[test]
    fn wrap_splits_long_lines_on_word_boundaries() {
        // Fake measure: 6 px per char, like a monospace face.
        let w = |s: &str| s.chars().count() as f64 * 6.0;

        assert_eq!(
            wrap_with(&w, "Rises 11:34pm · Sets 1:04pm", 200.0),
            vec!["Rises 11:34pm · Sets 1:04pm"],
            "no wrapping when the line already fits"
        );

        // 27 chars × 6 = 162 px; a 100 px budget forces a split.
        let wrapped = wrap_with(&w, "Rises 11:34pm · Sets 1:04pm", 100.0);
        assert_eq!(wrapped, vec!["Rises 11:34pm ·", "Sets 1:04pm"]);
        for line in &wrapped {
            assert!(w(line) <= 100.0, "every wrapped line fits: {line}");
        }

        // A single over-long word stays whole rather than splitting.
        assert_eq!(
            wrap_with(&w, "Circumpolar", 30.0),
            vec!["Circumpolar"]
        );
    }

    #[test]
    fn no_room_either_side_pins_inside_safe_area() {
        // Anchor near the bottom with a top reservation squeezing space.
        let ins = Insets {
            top: 100.0,
            bottom: 40.0,
            ..Insets::default()
        };
        let r = anchored_rect_with_insets(
            CONTAINER,
            Point { x: 120.0, y: 70.0 },
            Size {
                width: 180.0,
                height: 330.0,
            },
            &style(),
            ins,
        );
        assert!(r.y >= 40.0 + 8.0 - 1e-9, "stays above the bottom strip");
        assert!(
            r.y + 330.0 <= 500.0 - 100.0 - 8.0 + 1e-9,
            "stays below the top strip"
        );
    }
}
