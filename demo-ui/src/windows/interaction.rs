//! Interaction demo windows: drag-and-drop and panels layout.
//!
//! These demos show stateful interaction patterns — custom painting and event
//! handling — without animation. The Popups demo lives in `popups_demo.rs`;
//! the Scene demo lives in `scene_demo.rs`.

use std::sync::Arc;

use agg_gui::{
    set_cursor_icon, Color, CursorIcon, DrawCtx, Event, EventResult, FlexColumn, Font, HAnchor,
    Hyperlink, Insets, Label, MouseButton, Point, Rect, ScrollView, Separator, Size, Widget,
};

mod drag_and_drop;
pub use drag_and_drop::drag_and_drop;
// ---------------------------------------------------------------------------
// Panels demo
// ---------------------------------------------------------------------------

const PANEL_SOURCE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows/interaction.rs";
const PANEL_GAP: f64 = 4.0;
const TOP_MIN_H: f64 = 32.0;
const SIDE_MIN_W: f64 = 80.0;
const SIDE_MAX_W: f64 = 200.0;
const BOTTOM_H: f64 = 52.0;
const LOREM_IPSUM_LONG: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
     Curabitur et mauris auctor, cursus leo ut, viverra erat. \
     Nulla facilisi. Vivamus tempus ligula a lectus condimentum aliquam. \
     Sed sit amet magna et arcu efficitur porttitor. Suspendisse potenti. \
     Praesent consequat, lacus in sollicitudin tempor, ex purus commodo urna.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelDrag {
    Top,
    Left,
    Right,
}

/// Egui-style panels layout: top, left, right, bottom, then central panel.
struct PanelsLayout {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    top_h: f64,
    left_w: f64,
    right_w: f64,
    hover: Option<PanelDrag>,
    drag: Option<PanelDrag>,
}

impl Widget for PanelsLayout {
    fn type_name(&self) -> &'static str {
        "PanelsLayout"
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

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        self.clamp_sizes();
        self.layout_children();
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let top_h = self.top_h;
        let bottom_h = BOTTOM_H.min((h - top_h - PANEL_GAP * 2.0).max(0.0));
        let left_w = self.left_w;
        let right_w = self.right_w;
        let mid_w = (w - left_w - right_w - PANEL_GAP * 2.0).max(0.0);
        let middle_h = (h - top_h - PANEL_GAP).max(0.0);
        let center_h = (middle_h - bottom_h - PANEL_GAP).max(0.0);

        let top = Rect::new(0.0, h - top_h, w, top_h);
        let left = Rect::new(0.0, 0.0, left_w, middle_h);
        let right = Rect::new(w - right_w, 0.0, right_w, middle_h);
        let bottom = Rect::new(left_w + PANEL_GAP, 0.0, mid_w, bottom_h);
        let center = Rect::new(left_w + PANEL_GAP, bottom_h + PANEL_GAP, mid_w, center_h);

        let panel_bg = v.panel_fill;

        for rect in [top, left, right, bottom, center] {
            ctx.set_fill_color(panel_bg);
            ctx.begin_path();
            ctx.rect(rect.x, rect.y, rect.width, rect.height);
            ctx.fill();
        }
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let top_h = self.top_h;
        let bottom_h = BOTTOM_H.min((h - top_h - PANEL_GAP * 2.0).max(0.0));
        let left_w = self.left_w;
        let right_w = self.right_w;
        let mid_w = (w - left_w - right_w - PANEL_GAP * 2.0).max(0.0);
        let middle_h = (h - top_h - PANEL_GAP).max(0.0);
        let center_h = (middle_h - bottom_h - PANEL_GAP).max(0.0);

        let top = Rect::new(0.0, h - top_h, w, top_h);
        let left = Rect::new(0.0, 0.0, left_w, middle_h);
        let right = Rect::new(w - right_w, 0.0, right_w, middle_h);
        let bottom = Rect::new(left_w + PANEL_GAP, 0.0, mid_w, bottom_h);
        let center = Rect::new(left_w + PANEL_GAP, bottom_h + PANEL_GAP, mid_w, center_h);

        let mut draw_separator = |rect: Rect, active: bool| {
            let color = if active {
                Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.55)
            } else {
                v.separator
            };
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.rect(rect.x, rect.y, rect.width, rect.height);
            ctx.fill();
        };

        draw_separator(
            Rect::new(0.0, h - top_h - PANEL_GAP, w, PANEL_GAP),
            self.hover == Some(PanelDrag::Top) || self.drag == Some(PanelDrag::Top),
        );
        draw_separator(
            Rect::new(left_w, 0.0, PANEL_GAP, middle_h),
            self.hover == Some(PanelDrag::Left) || self.drag == Some(PanelDrag::Left),
        );
        draw_separator(
            Rect::new(w - right_w - PANEL_GAP, 0.0, PANEL_GAP, middle_h),
            self.hover == Some(PanelDrag::Right) || self.drag == Some(PanelDrag::Right),
        );

        for rect in [top, left, right, bottom, center] {
            ctx.set_stroke_color(v.separator);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rect(rect.x, rect.y, rect.width, rect.height);
            ctx.stroke();
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        self.drag.is_some()
            || (local_pos.x >= 0.0
                && local_pos.x <= self.bounds.width
                && local_pos.y >= 0.0
                && local_pos.y <= self.bounds.height)
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                if let Some(drag) = self.drag {
                    self.resize_panel(drag, *pos);
                    self.layout_children();
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }

                let next = self.drag_target_at(*pos);
                if let Some(target) = next {
                    set_cursor_icon(match target {
                        PanelDrag::Top => CursorIcon::ResizeVertical,
                        PanelDrag::Left | PanelDrag::Right => CursorIcon::ResizeHorizontal,
                    });
                }
                if self.hover != next {
                    self.hover = next;
                    agg_gui::animation::request_draw();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(target) = self.drag_target_at(*pos) {
                    self.drag = Some(target);
                    self.hover = Some(target);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.drag.take().is_some() {
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

/// Build the Panels demo — resizable egui-style panels with scrollable content.
pub fn panels_demo(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(PanelsLayout {
        bounds: Rect::default(),
        children: vec![
            panel_scroll("Expandable Upper Panel", Arc::clone(&font)),
            panel_scroll("Left Panel", Arc::clone(&font)),
            panel_scroll("Right Panel", Arc::clone(&font)),
            bottom_panel(Arc::clone(&font)),
            panel_scroll("Central Panel", font),
        ],
        top_h: 112.0,
        left_w: 150.0,
        right_w: 150.0,
        hover: None,
        drag: None,
    })
}

impl PanelsLayout {
    fn clamp_sizes(&mut self) {
        let w = self.bounds.width.max(0.0);
        let h = self.bounds.height.max(0.0);
        let side_max = SIDE_MAX_W
            .min((w - PANEL_GAP * 2.0).max(0.0) * 0.45)
            .max(SIDE_MIN_W);
        self.top_h = self
            .top_h
            .clamp(TOP_MIN_H, (h - BOTTOM_H - PANEL_GAP * 2.0).max(TOP_MIN_H));
        self.left_w = self.left_w.clamp(SIDE_MIN_W, side_max);
        self.right_w = self.right_w.clamp(SIDE_MIN_W, side_max);
    }

    fn layout_children(&mut self) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        if self.children.len() < 5 || w <= 0.0 || h <= 0.0 {
            return;
        }

        let top_h = self.top_h;
        let bottom_h = BOTTOM_H.min((h - top_h - PANEL_GAP * 2.0).max(0.0));
        let left_w = self.left_w;
        let right_w = self.right_w;
        let mid_w = (w - left_w - right_w - PANEL_GAP * 2.0).max(0.0);
        let middle_h = (h - top_h - PANEL_GAP).max(0.0);
        let center_h = (middle_h - bottom_h - PANEL_GAP).max(0.0);

        let rects = [
            Rect::new(0.0, h - top_h, w, top_h),
            Rect::new(0.0, 0.0, left_w, middle_h),
            Rect::new(w - right_w, 0.0, right_w, middle_h),
            Rect::new(left_w + PANEL_GAP, 0.0, mid_w, bottom_h),
            Rect::new(left_w + PANEL_GAP, bottom_h + PANEL_GAP, mid_w, center_h),
        ];

        for (child, rect) in self.children.iter_mut().zip(rects) {
            child.layout(Size::new(rect.width, rect.height));
            child.set_bounds(rect);
        }
    }

    fn drag_target_at(&self, pos: Point) -> Option<PanelDrag> {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let middle_h = (h - self.top_h - PANEL_GAP).max(0.0);
        let top_y = h - self.top_h - PANEL_GAP;

        if pos.y >= top_y - 2.0 && pos.y <= top_y + PANEL_GAP + 2.0 {
            return Some(PanelDrag::Top);
        }
        if pos.y <= middle_h {
            if pos.x >= self.left_w - 2.0 && pos.x <= self.left_w + PANEL_GAP + 2.0 {
                return Some(PanelDrag::Left);
            }
            let right_x = w - self.right_w - PANEL_GAP;
            if pos.x >= right_x - 2.0 && pos.x <= right_x + PANEL_GAP + 2.0 {
                return Some(PanelDrag::Right);
            }
        }
        None
    }

    fn resize_panel(&mut self, target: PanelDrag, pos: Point) {
        let w = self.bounds.width.max(0.0);
        let h = self.bounds.height.max(0.0);
        match target {
            PanelDrag::Top => {
                self.top_h = (h - pos.y - PANEL_GAP)
                    .clamp(TOP_MIN_H, (h - BOTTOM_H - PANEL_GAP * 2.0).max(TOP_MIN_H));
            }
            PanelDrag::Left => {
                self.left_w = pos.x.clamp(SIDE_MIN_W, SIDE_MAX_W);
            }
            PanelDrag::Right => {
                self.right_w = (w - pos.x - PANEL_GAP).clamp(SIDE_MIN_W, SIDE_MAX_W);
            }
        }
    }
}

fn panel_scroll(title: &'static str, font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_inner_padding(Insets::from_sides(10.0, 10.0, 10.0, 8.0));

    col.push(
        Box::new(
            Label::new(title, Arc::clone(&font))
                .with_font_size(17.0)
                .with_h_anchor(HAnchor::CENTER),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(LOREM_IPSUM_LONG, Arc::clone(&font))
                .with_font_size(12.0)
                .with_wrap(true),
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(LOREM_IPSUM_LONG, font)
                .with_font_size(12.0)
                .with_wrap(true),
        ),
        0.0,
    );

    Box::new(ScrollView::new(Box::new(col)))
}

fn bottom_panel(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(4.0)
        .with_inner_padding(Insets::symmetric(8.0, 5.0));
    col.push(
        Box::new(
            Label::new("Bottom Panel", Arc::clone(&font))
                .with_font_size(17.0)
                .with_h_anchor(HAnchor::CENTER),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Hyperlink::new("(source code)", font)
                .with_font_size(11.0)
                .with_h_anchor(HAnchor::CENTER)
                .on_click(|| crate::url::open_url(PANEL_SOURCE_URL)),
        ),
        0.0,
    );
    Box::new(col)
}

#[cfg(test)]
mod panel_tests {
    use super::*;

    struct Probe {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }

    impl Probe {
        fn boxed() -> Box<dyn Widget> {
            Box::new(Self {
                bounds: Rect::default(),
                children: Vec::new(),
            })
        }
    }

    impl Widget for Probe {
        fn type_name(&self) -> &'static str {
            "Probe"
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
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    fn layout() -> PanelsLayout {
        PanelsLayout {
            bounds: Rect::default(),
            children: vec![
                Probe::boxed(),
                Probe::boxed(),
                Probe::boxed(),
                Probe::boxed(),
                Probe::boxed(),
            ],
            top_h: 112.0,
            left_w: 150.0,
            right_w: 150.0,
            hover: None,
            drag: None,
        }
    }

    #[test]
    fn panels_match_egui_order_with_y_up_geometry() {
        let mut p = layout();
        p.layout(Size::new(600.0, 400.0));

        assert_eq!(p.children[0].bounds(), Rect::new(0.0, 288.0, 600.0, 112.0));
        assert_eq!(p.children[1].bounds(), Rect::new(0.0, 0.0, 150.0, 284.0));
        assert_eq!(p.children[2].bounds(), Rect::new(450.0, 0.0, 150.0, 284.0));
        assert_eq!(p.children[3].bounds(), Rect::new(154.0, 0.0, 292.0, 52.0));
        assert_eq!(p.children[4].bounds(), Rect::new(154.0, 56.0, 292.0, 228.0));
    }
}
