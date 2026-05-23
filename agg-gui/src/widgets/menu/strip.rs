//! `MenuBarStrip` — full-width top-of-window strip that auto-sizes its
//! height to the wrapped child's natural height.
//!
//! Most apps put a menu bar at the top of the window inside a small
//! container that paints a chrome background, draws a 1-px separator
//! between the bar and the rest of the UI, and lets the bar scroll
//! horizontally if its content overflows a narrow viewport.  Before this
//! widget existed every app reinvented that container with a hard-coded
//! height, which left a visible chrome stripe below the menu bar
//! whenever the constants drifted (or the bar shrank from a redesign).
//!
//! `MenuBarStrip` removes the foot-gun: it queries the child for its
//! natural height during `layout` and reports exactly that height back
//! up the tree.  Drop it inside a `FlexColumn` with a wrapped `MenuBar`
//! (or any other top-bar widget) and the strip claims the right number
//! of pixels with no per-app tuning.
//!
//! Provides:
//! - Full-width `top_bar_bg` fill behind the wrapped child.
//! - 1-px bottom separator line matching the `Separator` widget tone.
//! - Horizontal overflow scroll (wheel + middle-button drag) when the
//!   child's natural width exceeds the available width.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::widget::{current_mouse_world, Widget};

pub struct MenuBarStrip {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    h_offset: f64,
    content_width: f64,
    content_height: f64,
    middle_dragging: bool,
    middle_start_world_x: f64,
    middle_start_h_offset: f64,
}

impl MenuBarStrip {
    pub fn new(inner: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![inner],
            h_offset: 0.0,
            content_width: 0.0,
            content_height: 0.0,
            middle_dragging: false,
            middle_start_world_x: 0.0,
            middle_start_h_offset: 0.0,
        }
    }

    fn max_scroll(&self) -> f64 {
        (self.content_width - self.bounds.width).max(0.0)
    }

    fn clamp_offset(&mut self) {
        self.h_offset = self.h_offset.clamp(0.0, self.max_scroll());
    }

    fn update_child_bounds(&mut self) {
        if let Some(child) = self.children.first_mut() {
            child.set_bounds(Rect::new(
                -self.h_offset.round(),
                0.0,
                self.content_width,
                self.content_height,
            ));
        }
    }
}

impl Widget for MenuBarStrip {
    fn type_name(&self) -> &'static str {
        "MenuBarStrip"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        // Ask the child what it wants.  Its reported height becomes the
        // strip's height — no app-side constant needed.  The strip
        // remains full-width across `available` so the bg fill runs
        // edge-to-edge even when the child is narrower (e.g. a
        // `fit_width` MenuBar).
        let used = if let Some(child) = self.children.first_mut() {
            child.layout(available)
        } else {
            Size::new(0.0, 0.0)
        };
        self.content_height = used.height;
        self.content_width = used.width.max(available.width);
        self.bounds = Rect::new(0.0, 0.0, available.width, self.content_height);
        self.clamp_offset();
        self.update_child_bounds();
        Size::new(available.width, self.content_height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(v.top_bar_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Y-up: local y=0 is the BOTTOM edge of the strip, which is
        // exactly where the separator between the strip and the rest of
        // the UI lives.  Painted in `paint_overlay` so it sits on top of
        // child widgets — `MenuBar` paints an opaque `top_bar_bg` fill
        // across its full bounds (to satisfy LCD coverage), which would
        // otherwise erase a separator drawn in `paint`.
        let v = ctx.visuals();
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, 1.0);
        ctx.fill();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseWheel {
                delta_x,
                delta_y,
                modifiers,
                ..
            } => {
                // Horizontal wheel is the canonical scroll direction.
                // Shift + vertical wheel falls back to horizontal so
                // mice without a horizontal axis can still pan the bar.
                let delta = if delta_x.abs() > f64::EPSILON {
                    *delta_x
                } else if modifiers.shift {
                    *delta_y
                } else {
                    0.0
                };
                if delta.abs() <= f64::EPSILON || self.max_scroll() <= 0.0 {
                    return EventResult::Ignored;
                }
                let before = self.h_offset;
                self.h_offset += delta * 40.0;
                self.clamp_offset();
                if (self.h_offset - before).abs() > f64::EPSILON {
                    self.update_child_bounds();
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Middle,
                ..
            } if self.max_scroll() > 0.0 => {
                self.middle_dragging = true;
                self.middle_start_world_x = current_mouse_world().map(|p| p.x).unwrap_or(0.0);
                self.middle_start_h_offset = self.h_offset;
                EventResult::Consumed
            }
            Event::MouseMove { pos } if self.middle_dragging => {
                let world_x = current_mouse_world().map(|p| p.x).unwrap_or(pos.x);
                self.h_offset =
                    self.middle_start_h_offset - (world_x - self.middle_start_world_x);
                self.clamp_offset();
                self.update_child_bounds();
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Middle,
                ..
            } if self.middle_dragging => {
                self.middle_dragging = false;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::widget::Widget;

    /// Tiny stand-in for a menu bar — reports a fixed natural height
    /// so the strip's "size-to-content" contract is testable without
    /// pulling in the whole `MenuBar` widget.
    struct FixedHeightChild {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        natural_height: f64,
        natural_width: f64,
    }

    impl Widget for FixedHeightChild {
        fn type_name(&self) -> &'static str {
            "FixedHeightChild"
        }
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
        fn layout(&mut self, _available: Size) -> Size {
            Size::new(self.natural_width, self.natural_height)
        }
        fn paint(&mut self, _ctx: &mut dyn crate::DrawCtx) {}
        fn on_event(&mut self, _: &crate::Event) -> crate::EventResult {
            crate::EventResult::Ignored
        }
    }

    #[test]
    fn strip_height_matches_child_natural_height() {
        let child = FixedHeightChild {
            bounds: Rect::default(),
            children: Vec::new(),
            natural_height: 26.0,
            natural_width: 120.0,
        };
        let mut strip = MenuBarStrip::new(Box::new(child));
        let used = strip.layout(Size::new(800.0, 200.0));
        assert_eq!(used.height, 26.0, "strip must size to child's height");
        assert_eq!(used.width, 800.0, "strip must span full available width");
    }

    #[test]
    fn strip_overflow_scroll_kicks_in_when_child_wider_than_available() {
        let child = FixedHeightChild {
            bounds: Rect::default(),
            children: Vec::new(),
            natural_height: 26.0,
            natural_width: 1000.0,
        };
        let mut strip = MenuBarStrip::new(Box::new(child));
        strip.layout(Size::new(400.0, 200.0));
        assert!(
            strip.max_scroll() > 0.0,
            "child wider than available width must produce scrollable overflow"
        );
    }
}
