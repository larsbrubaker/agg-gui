use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::scroll_view::{ScrollBarColor, ScrollBarKind, ScrollBarStyle, ScrollBarVisibility};

pub(crate) const DEFAULT_GRAB_MARGIN: f64 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollbarOrientation {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy)]
pub(crate) struct ScrollbarGeometry {
    pub orientation: ScrollbarOrientation,
    pub track_start: f64,
    pub track_end: f64,
    /// Vertical: right edge of the bar. Horizontal: bottom edge of the bar.
    pub cross_end: f64,
    pub hit_margin: f64,
}

#[derive(Clone, Copy)]
pub(crate) struct PreparedScrollbar {
    pub track: Rect,
    pub thumb: Rect,
    pub radius: f64,
    pub alpha: f64,
    pub color: ScrollBarColor,
    pub hovered_thumb: bool,
    pub dragging: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ScrollbarAxis {
    pub enabled: bool,
    pub offset: f64,
    pub content: f64,
    pub hovered_bar: bool,
    pub hovered_thumb: bool,
    pub dragging: bool,
    pub drag_thumb_offset: f64,
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
            hover_anim: crate::animation::Tween::new(0.0, 0.12),
            visibility_anim: crate::animation::Tween::new(0.0, 0.18),
        }
    }
}

impl ScrollbarAxis {
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
        self.enabled && self.content > viewport
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
        if self.content <= viewport {
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
        if self.content <= viewport {
            return false;
        }
        let floating = style.kind == ScrollBarKind::Floating;
        match visibility {
            ScrollBarVisibility::AlwaysHidden => false,
            ScrollBarVisibility::AlwaysVisible => true,
            ScrollBarVisibility::VisibleWhenNeeded => {
                !floating || self.hovered_bar || self.dragging
            }
        }
    }

    pub fn prepare_paint(
        &mut self,
        viewport: f64,
        style: ScrollBarStyle,
        visibility: ScrollBarVisibility,
        geom: ScrollbarGeometry,
    ) -> Option<PreparedScrollbar> {
        self.visibility_anim
            .set_target(if self.should_paint(viewport, style, visibility) {
                1.0
            } else {
                0.0
            });
        let alpha = self.visibility_anim.tick();
        if !self.enabled || self.content <= viewport || alpha <= 0.001 {
            return None;
        }
        let (thumb_start, thumb_len) = self.thumb_metrics(viewport, style, geom)?;
        self.hover_anim
            .set_target(if self.interact() { 1.0 } else { 0.0 });
        let t = self.hover_anim.tick();
        let bar_w = style.bar_width_at(t);
        let radius = bar_w * 0.5;
        let (track, thumb) = match geom.orientation {
            ScrollbarOrientation::Vertical => {
                let bar_x = geom.cross_end - bar_w;
                (
                    Rect::new(
                        bar_x,
                        geom.track_start,
                        bar_w,
                        geom.track_end - geom.track_start,
                    ),
                    Rect::new(bar_x, thumb_start, bar_w, thumb_len),
                )
            }
            ScrollbarOrientation::Horizontal => (
                Rect::new(
                    geom.track_start,
                    geom.cross_end,
                    geom.track_end - geom.track_start,
                    bar_w,
                ),
                Rect::new(thumb_start, geom.cross_end, thumb_len, bar_w),
            ),
        };
        Some(PreparedScrollbar {
            track,
            thumb,
            radius,
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

pub(crate) fn paint_prepared_scrollbar(ctx: &mut dyn DrawCtx, bar: PreparedScrollbar) {
    let v = ctx.visuals();
    let track_color = v.scroll_track;
    ctx.set_fill_color(scale_alpha(track_color, bar.alpha));
    ctx.begin_path();
    ctx.rounded_rect(
        bar.track.x,
        bar.track.y,
        bar.track.width,
        bar.track.height,
        bar.radius,
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
