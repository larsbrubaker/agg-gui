//! One scroll bar axis of a [`ScrollView`](super::ScrollView): its offset and
//! content extent, thumb metrics, hover / drag / page interaction, the
//! hover-grow and fade tweens, and the painted track and thumb. `ScrollView`
//! owns one per axis and supplies the geometry; agg-sharp's `ScrollBar`.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::scroll_view::{ScrollBarColor, ScrollBarKind, ScrollBarStyle, ScrollBarVisibility};

pub const DEFAULT_GRAB_MARGIN: f64 = 6.0;

/// Seconds a floating bar takes to grow from its dormant width to its full
/// width on hover (and to thin back down): agg-sharp's `ScrollBar.GrowMs`.
pub const SCROLL_BAR_GROW_SECS: f64 = 0.12;
/// Seconds a floating bar takes to fade in or out: agg-sharp's
/// `ScrollBar.FadeMs`. Eased out (cubic), so half way it is at 0.875.
pub const SCROLL_BAR_FADE_SECS: f64 = 0.18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarOrientation {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy)]
pub struct ScrollbarGeometry {
    pub orientation: ScrollbarOrientation,
    pub track_start: f64,
    pub track_end: f64,
    /// Vertical: right edge of the bar. Horizontal: bottom edge of the bar.
    pub cross_end: f64,
    pub hit_margin: f64,
}

#[derive(Clone, Copy)]
pub struct PreparedScrollbar {
    pub track: Rect,
    pub thumb: Rect,
    /// The thumb's corner radius.
    pub radius: f64,
    /// The track's corner radius.
    pub track_radius: f64,
    pub alpha: f64,
    pub color: ScrollBarColor,
    pub hovered_thumb: bool,
    pub dragging: bool,
}

#[derive(Clone, Copy)]
pub struct ScrollbarAxis {
    pub enabled: bool,
    pub offset: f64,
    pub content: f64,
    pub hovered_bar: bool,
    pub hovered_thumb: bool,
    pub dragging: bool,
    pub drag_thumb_offset: f64,
    /// A finger is panning the view: a floating bar shows for it, since a
    /// finger has no hover to find the bar with.
    pub touch_panning: bool,
    pub hover_anim: crate::animation::Tween,
    pub visibility_anim: crate::animation::Tween,
}

impl Default for ScrollbarAxis {
    fn default() -> Self {
        Self {
            enabled: false,
            offset: 0.0,
            content: 0.0,
            hovered_bar: false,
            hovered_thumb: false,
            dragging: false,
            drag_thumb_offset: 0.0,
            touch_panning: false,
            hover_anim: crate::animation::Tween::new(0.0, SCROLL_BAR_GROW_SECS),
            visibility_anim: crate::animation::Tween::new(0.0, SCROLL_BAR_FADE_SECS),
        }
    }
}

/// How much more than the view the content has to be before it counts as
/// overflowing (agg-sharp `ScrollableWidget.OverflowTolerance`). Content sized
/// to exactly fit (a window fitted to it) lands a rounding error either side
/// of the view, which is not something to show a scroll bar for.
pub const OVERFLOW_TOLERANCE: f64 = 1e-6;

/// `content` overflows a `viewport` extent by more than
/// [`OVERFLOW_TOLERANCE`] (agg-sharp `HasVerticalOverflow` /
/// `HasHorizontalOverflow`).
#[inline]
pub fn overflows(content: f64, viewport: f64) -> bool {
    content > viewport + OVERFLOW_TOLERANCE
}

impl ScrollbarAxis {
    /// The content overflows `viewport` ([`overflows`]), whether or not the
    /// axis scrolls.
    pub fn overflows(&self, viewport: f64) -> bool {
        overflows(self.content, viewport)
    }

    pub fn max_scroll(&self, viewport: f64) -> f64 {
        (self.content - viewport).max(0.0)
    }

    pub fn interact(&self) -> bool {
        self.hovered_bar || self.hovered_thumb || self.dragging
    }

    pub fn animation_active(&self) -> bool {
        self.hover_anim.is_animating() || self.visibility_anim.is_animating()
    }

    pub fn can_scroll(&self, viewport: f64) -> bool {
        self.enabled && self.overflows(viewport)
    }

    pub fn clamp_offset(&mut self, viewport: f64) {
        self.offset = self.offset.clamp(0.0, self.max_scroll(viewport)).round();
    }

    pub fn scroll_by(&mut self, delta: f64, viewport: f64) -> bool {
        let old = self.offset;
        self.offset += delta;
        self.clamp_offset(viewport);
        (self.offset - old).abs() > 1e-6
    }

    pub fn thumb_metrics(
        &self,
        viewport: f64,
        style: ScrollBarStyle,
        geom: ScrollbarGeometry,
    ) -> Option<(f64, f64)> {
        if !self.overflows(viewport) {
            return None;
        }
        let track_len = geom.track_end - geom.track_start;
        let ratio = viewport / self.content;
        let thumb_len = (track_len * ratio).max(style.handle_min_length);
        let travel = (track_len - thumb_len).max(0.0);
        let max_s = self.max_scroll(viewport);
        let start = if max_s > 0.0 {
            match geom.orientation {
                ScrollbarOrientation::Vertical => {
                    geom.track_start + travel * (1.0 - self.offset / max_s)
                }
                ScrollbarOrientation::Horizontal => {
                    geom.track_start + travel * (self.offset / max_s)
                }
            }
        } else {
            match geom.orientation {
                ScrollbarOrientation::Vertical => geom.track_start + travel,
                ScrollbarOrientation::Horizontal => geom.track_start,
            }
        };
        Some((start, thumb_len))
    }

    pub fn pos_on_thumb(
        &self,
        pos: Point,
        viewport: f64,
        style: ScrollBarStyle,
        geom: ScrollbarGeometry,
    ) -> bool {
        match geom.orientation {
            ScrollbarOrientation::Vertical => {
                let hit_left = geom.cross_end - style.bar_width - geom.hit_margin;
                if pos.x < hit_left || pos.x >= geom.cross_end {
                    return false;
                }
                self.thumb_metrics(viewport, style, geom)
                    .is_some_and(|(start, len)| pos.y >= start && pos.y <= start + len)
            }
            ScrollbarOrientation::Horizontal => {
                let hit_top = geom.cross_end + style.bar_width + geom.hit_margin;
                if pos.y < geom.cross_end || pos.y >= hit_top {
                    return false;
                }
                self.thumb_metrics(viewport, style, geom)
                    .is_some_and(|(start, len)| pos.x >= start && pos.x <= start + len)
            }
        }
    }

    pub fn pos_in_hover(&self, pos: Point, style: ScrollBarStyle, geom: ScrollbarGeometry) -> bool {
        match geom.orientation {
            ScrollbarOrientation::Vertical => {
                let left = geom.cross_end - style.bar_width - geom.hit_margin;
                pos.x >= left && pos.x < geom.cross_end
            }
            ScrollbarOrientation::Horizontal => {
                let top = geom.cross_end + style.bar_width + geom.hit_margin;
                pos.y >= geom.cross_end && pos.y < top
            }
        }
    }

    pub fn update_hover(
        &mut self,
        pos: Point,
        viewport: f64,
        style: ScrollBarStyle,
        geom: ScrollbarGeometry,
    ) -> bool {
        let was_bar = self.hovered_bar;
        let was_thumb = self.hovered_thumb;
        let can_scroll = self.can_scroll(viewport);
        self.hovered_bar = can_scroll && self.pos_in_hover(pos, style, geom);
        self.hovered_thumb = can_scroll && self.pos_on_thumb(pos, viewport, style, geom);
        was_bar != self.hovered_bar || was_thumb != self.hovered_thumb
    }

    pub fn begin_drag(
        &mut self,
        pos: Point,
        viewport: f64,
        style: ScrollBarStyle,
        geom: ScrollbarGeometry,
    ) -> bool {
        if !self.pos_on_thumb(pos, viewport, style, geom) {
            return false;
        }
        let start = self
            .thumb_metrics(viewport, style, geom)
            .map(|(start, _)| start)
            .unwrap_or(0.0);
        self.dragging = true;
        self.drag_thumb_offset = match geom.orientation {
            ScrollbarOrientation::Vertical => pos.y - start,
            ScrollbarOrientation::Horizontal => pos.x - start,
        };
        true
    }

    pub fn drag_to(
        &mut self,
        pos: Point,
        viewport: f64,
        style: ScrollBarStyle,
        geom: ScrollbarGeometry,
    ) -> bool {
        let Some((_, thumb_len)) = self.thumb_metrics(viewport, style, geom) else {
            return false;
        };
        let travel = (geom.track_end - geom.track_start - thumb_len).max(1.0);
        let raw_start = match geom.orientation {
            ScrollbarOrientation::Vertical => pos.y - self.drag_thumb_offset,
            ScrollbarOrientation::Horizontal => pos.x - self.drag_thumb_offset,
        };
        let new_start = raw_start.clamp(geom.track_start, geom.track_start + travel);
        let frac = (new_start - geom.track_start) / travel;
        let old = self.offset;
        self.offset = match geom.orientation {
            ScrollbarOrientation::Vertical => (1.0 - frac) * self.max_scroll(viewport),
            ScrollbarOrientation::Horizontal => frac * self.max_scroll(viewport),
        };
        self.clamp_offset(viewport);
        (self.offset - old).abs() > 1e-6
    }

    pub fn page_at(
        &mut self,
        pos: Point,
        viewport: f64,
        style: ScrollBarStyle,
        geom: ScrollbarGeometry,
    ) -> bool {
        let Some((start, len)) = self.thumb_metrics(viewport, style, geom) else {
            return false;
        };
        let page = (viewport - 16.0).max(20.0);
        let old = self.offset;
        match geom.orientation {
            ScrollbarOrientation::Vertical => {
                if pos.y > start + len {
                    self.offset = (self.offset - page).max(0.0);
                } else if pos.y < start {
                    self.offset = (self.offset + page).min(self.max_scroll(viewport));
                }
            }
            ScrollbarOrientation::Horizontal => {
                if pos.x < start {
                    self.offset = (self.offset - page).max(0.0);
                } else if pos.x > start + len {
                    self.offset = (self.offset + page).min(self.max_scroll(viewport));
                }
            }
        }
        self.clamp_offset(viewport);
        (self.offset - old).abs() > 1e-6
    }

    pub fn should_paint(
        &self,
        viewport: f64,
        style: ScrollBarStyle,
        visibility: ScrollBarVisibility,
    ) -> bool {
        if !self.overflows(viewport) {
            return false;
        }
        let floating = style.kind == ScrollBarKind::Floating;
        match visibility {
            ScrollBarVisibility::AlwaysHidden => false,
            ScrollBarVisibility::AlwaysVisible => true,
            ScrollBarVisibility::VisibleWhenNeeded => {
                !floating || self.hovered_bar || self.dragging || self.touch_panning
            }
        }
    }

    /// The visibility the bar is heading for: `1.0` when it should paint,
    /// `0.0` when it should hide (agg-sharp's `ScrollBar.FadeTarget`).
    pub fn fade_target(
        &self,
        viewport: f64,
        style: ScrollBarStyle,
        visibility: ScrollBarVisibility,
    ) -> f64 {
        if self.should_paint(viewport, style, visibility) {
            1.0
        } else {
            0.0
        }
    }

    /// Track and thumb rects at hover-animation parameter `t` (see
    /// [`ScrollBarStyle::thumb_width_at`] and
    /// [`ScrollBarStyle::track_width_at`]), or `None` when the content fits.
    /// The track lies against the outer edge (`cross_end`); the thumb is
    /// centred across it, which for
    /// [`ScrollThumbRest::OuterEdge`](super::scroll_view::ScrollThumbRest)'s equal
    /// widths is the same edge.
    pub fn bar_rects(
        &self,
        viewport: f64,
        style: ScrollBarStyle,
        geom: ScrollbarGeometry,
        t: f64,
    ) -> Option<(Rect, Rect)> {
        let (thumb_start, thumb_len) = self.thumb_metrics(viewport, style, geom)?;
        let track_w = style.track_width_at(t);
        let thumb_w = style.thumb_width_at(t);
        let inset = (track_w - thumb_w) * 0.5;
        let track_len = geom.track_end - geom.track_start;
        Some(match geom.orientation {
            ScrollbarOrientation::Vertical => {
                let bar_x = geom.cross_end - track_w;
                (
                    Rect::new(bar_x, geom.track_start, track_w, track_len),
                    Rect::new(bar_x + inset, thumb_start, thumb_w, thumb_len),
                )
            }
            ScrollbarOrientation::Horizontal => (
                Rect::new(geom.track_start, geom.cross_end, track_len, track_w),
                Rect::new(thumb_start, geom.cross_end + inset, thumb_len, thumb_w),
            ),
        })
    }

    pub fn prepare_paint(
        &mut self,
        viewport: f64,
        style: ScrollBarStyle,
        visibility: ScrollBarVisibility,
        geom: ScrollbarGeometry,
    ) -> Option<PreparedScrollbar> {
        let target = self.fade_target(viewport, style, visibility);
        if style.kind == ScrollBarKind::Solid {
            // A Solid bar has no auto-hide: it is there whenever the content
            // overflows, at full opacity from its first frame (agg-sharp's
            // `ScrollBarAppearanceTests.DefaultsKeepTheLegacyGeometry`).
            self.visibility_anim = crate::animation::Tween::new(target, SCROLL_BAR_FADE_SECS);
        } else {
            self.visibility_anim.set_target(target);
        }
        let alpha = self.visibility_anim.tick();
        // The width eases on even while the bar is hidden, so a bar that
        // faded out mid-shrink is back at its dormant width when it next
        // shows rather than resuming a stale width.
        let hover_target = if self.interact() { 1.0 } else { 0.0 };
        if style.snap_on_hover {
            // agg-sharp's classic bar: the growth jumps, no ease.
            self.hover_anim = crate::animation::Tween::new(hover_target, SCROLL_BAR_GROW_SECS);
        } else {
            self.hover_anim.set_target(hover_target);
        }
        let t = self.hover_anim.tick();
        if !self.enabled || !self.overflows(viewport) || alpha <= 0.001 {
            return None;
        }
        let (track, thumb) = self.bar_rects(viewport, style, geom, t)?;
        Some(PreparedScrollbar {
            track,
            thumb,
            radius: style.corner_radius_at(t),
            track_radius: style.track_corner_radius_at(t),
            alpha,
            color: style.color,
            hovered_thumb: self.hovered_thumb,
            dragging: self.dragging,
        })
    }
}

impl PreparedScrollbar {
    pub fn translated(mut self, dx: f64, dy: f64) -> Self {
        self.track.x += dx;
        self.track.y += dy;
        self.thumb.x += dx;
        self.thumb.y += dy;
        self
    }
}

pub fn paint_prepared_scrollbar(ctx: &mut dyn DrawCtx, bar: PreparedScrollbar) {
    let v = ctx.visuals();
    let track_color = v.scroll_track;
    ctx.set_fill_color(scale_alpha(track_color, bar.alpha));
    ctx.begin_path();
    ctx.rounded_rect(
        bar.track.x,
        bar.track.y,
        bar.track.width,
        bar.track.height,
        bar.track_radius,
    );
    ctx.fill();

    let thumb_color = match bar.color {
        ScrollBarColor::Background if bar.dragging => v.scroll_thumb_dragging,
        ScrollBarColor::Background if bar.hovered_thumb => v.scroll_thumb_hovered,
        ScrollBarColor::Background => v.scroll_thumb,
        ScrollBarColor::Foreground if bar.dragging => v.accent_pressed,
        ScrollBarColor::Foreground if bar.hovered_thumb => v.accent_hovered,
        ScrollBarColor::Foreground => v.scroll_thumb,
    };
    ctx.set_fill_color(scale_alpha(thumb_color, bar.alpha));
    ctx.begin_path();
    ctx.rounded_rect(
        bar.thumb.x,
        bar.thumb.y,
        bar.thumb.width,
        bar.thumb.height,
        bar.radius,
    );
    ctx.fill();
}

fn scale_alpha(c: Color, a: f64) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a * (a as f32).clamp(0.0, 1.0))
}

/// C# `ScrollableWidget.ScrollIntoView`'s minimum scroll, for a vertical
/// scroller whose content rises as its offset grows: the offset that shows
/// `rect` (local, Y-up) within the visible band `[bottom, top]`. Content
/// clipped at the top is lowered until its top shows; otherwise content
/// clipped at the bottom is raised until its bottom shows; content already
/// fully in view keeps `offset`. The caller clamps to its scroll range.
pub(crate) fn minimum_scroll_to_show(offset: f64, bottom: f64, top: f64, rect: Rect) -> f64 {
    if rect.bottom() >= bottom && rect.top() <= top {
        offset
    } else if rect.top() >= top {
        offset - (rect.top() - top)
    } else {
        offset + (bottom - rect.bottom())
    }
}
