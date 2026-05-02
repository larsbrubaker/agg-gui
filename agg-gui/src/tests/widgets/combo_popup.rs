use super::*;

#[test]
fn test_combo_popup_opens_up_when_space_below_is_limited() {
    use crate::text::Font;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let selected = Rc::new(Cell::new(0_usize));
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven",
        ],
        0,
        font,
    )
    .with_selected_cell(Rc::clone(&selected));

    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 120.0);
    app.layout(viewport);

    // Open from a root-level combo near the bottom of the viewport. There is
    // no room below in Y-up space, so the popup should choose the space above.
    let button_screen_y = viewport.height - 12.0;
    app.on_mouse_down(
        12.0,
        button_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        12.0,
        button_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    assert_eq!(
        app.root()
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "open")
            .map(|(_, v)| v),
        Some("true".to_string())
    );

    // Paint once so the global popup pass computes up/down geometry.
    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    assert_eq!(
        app.root()
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "popup_opens_up")
            .map(|(_, v)| v),
        Some("true".to_string())
    );

    // If the popup opened upward, row 3 is above the closed button and
    // selectable. If it incorrectly opened downward, this click misses it.
    let row_three_y_up = 35.0;
    assert!(
        app.root().hit_test(crate::Point::new(12.0, row_three_y_up)),
        "open ComboBox popup should extend hit testing above the root-level button"
    );
    app.on_mouse_down(
        12.0,
        viewport.height - row_three_y_up,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        12.0,
        viewport.height - row_three_y_up,
        MouseButton::Left,
        Modifiers::default(),
    );

    assert_eq!(selected.get(), 3);
}

#[test]
fn test_combo_popup_wheel_uses_system_scroll_direction() {
    use crate::text::Font;
    use crate::widgets::ScrollBarStyle;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    crate::set_scroll_style(ScrollBarStyle::default());
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let selected = Rc::new(Cell::new(0_usize));
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven",
        ],
        0,
        font,
    )
    .with_selected_cell(Rc::clone(&selected));

    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 120.0);
    app.layout(viewport);
    let button_screen_y = viewport.height - 12.0;
    app.on_mouse_down(
        12.0,
        button_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        12.0,
        button_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    assert_eq!(
        app.root()
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "open")
            .map(|(_, v)| v),
        Some("true".to_string())
    );

    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    assert_eq!(
        app.root()
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "popup_opens_up")
            .map(|(_, v)| v),
        Some("true".to_string())
    );

    // App convention: positive delta_y means content moves up. The popup now
    // routes wheel input through the same scrollbar axis as ScrollView, so one
    // wheel tick moves by the shared 40 px scroll step.
    let top_popup_row_y_up = 101.0;
    assert!(
        app.root()
            .hit_test(crate::Point::new(12.0, top_popup_row_y_up)),
        "open ComboBox popup should be hittable in the space above the button"
    );
    app.on_mouse_wheel(12.0, viewport.height - top_popup_row_y_up, 44.0);
    app.on_mouse_down(
        12.0,
        viewport.height - top_popup_row_y_up,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        12.0,
        viewport.height - top_popup_row_y_up,
        MouseButton::Left,
        Modifiers::default(),
    );

    assert_eq!(selected.get(), 4);
}

#[test]
fn test_combo_popup_scrollbar_hover_and_track_do_not_select_rows() {
    use crate::text::Font;
    use crate::widgets::ScrollBarStyle;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    crate::set_scroll_style(ScrollBarStyle::default());
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let selected = Rc::new(Cell::new(0_usize));
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
            "Eleven",
        ],
        0,
        font,
    )
    .with_selected_cell(Rc::clone(&selected));

    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 220.0);
    app.layout(viewport);

    // Open near the bottom so the popup opens upward and has a visible scrollbar.
    app.on_mouse_down(
        12.0,
        viewport.height - 12.0,
        MouseButton::Left,
        Modifiers::default(),
    );

    let track_x = 175.0;
    let thumb_y = 120.0;
    app.on_mouse_move(track_x, viewport.height - thumb_y);

    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    let thumb_pixel = sample(&fb, track_x as u32, thumb_y as u32);
    assert!(
        thumb_pixel[2] > thumb_pixel[0] && thumb_pixel[2] > thumb_pixel[1],
        "hovered popup scrollbar thumb should use the accent color; sampled {thumb_pixel:?}"
    );

    // Clicking the scrollbar track should page the popup list, not choose a row.
    app.on_mouse_down(
        track_x,
        viewport.height - 40.0,
        MouseButton::Left,
        Modifiers::default(),
    );
    assert_eq!(
        selected.get(),
        0,
        "scrollbar track click must not select the row underneath"
    );
    assert_eq!(
        app.root()
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "scroll_offset")
            .map(|(_, v)| v),
        Some("4".to_string()),
        "track click should page the dropdown using the same list direction as wheel scroll"
    );
}

#[test]
fn test_combo_popup_scrollbar_drag_updates_scroll_offset() {
    use crate::text::Font;
    use crate::widgets::ScrollBarStyle;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    crate::set_scroll_style(ScrollBarStyle::default());
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let selected = Rc::new(Cell::new(0_usize));
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
            "Eleven",
        ],
        0,
        font,
    )
    .with_selected_cell(Rc::clone(&selected));

    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 220.0);
    app.layout(viewport);

    app.on_mouse_down(
        12.0,
        viewport.height - 12.0,
        MouseButton::Left,
        Modifiers::default(),
    );

    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);

    let track_x = 175.0;
    let thumb_y = 120.0;
    app.on_mouse_move(track_x, viewport.height - thumb_y);
    app.on_mouse_down(
        track_x,
        viewport.height - thumb_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_move(track_x, viewport.height - 40.0);
    app.on_mouse_up(
        track_x,
        viewport.height - 40.0,
        MouseButton::Left,
        Modifiers::default(),
    );

    assert_eq!(
        app.root()
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "scroll_offset")
            .map(|(_, v)| v),
        Some("4".to_string()),
        "dragging the popup thumb should update the same scrollbar axis offset"
    );
    assert_eq!(
        selected.get(),
        0,
        "dragging the scrollbar must not select the row underneath"
    );
}

#[test]
fn test_combo_popup_middle_drag_scrolls_popup_not_parent() {
    use crate::text::Font;
    use crate::widgets::{ScrollBarStyle, ScrollBarVisibility};
    use crate::{DrawCtx, Event, EventResult, Point, Rect};
    use std::sync::Arc;

    struct MiddleDragParent {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        middle_drags: usize,
    }

    impl Widget for MiddleDragParent {
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
            self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
            if let Some(child) = self.children.first_mut() {
                child.layout(Size::new(180.0, 24.0));
                child.set_bounds(Rect::new(0.0, 0.0, 180.0, 24.0));
            }
            available
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn hit_test(&self, local_pos: Point) -> bool {
            local_pos.x >= 0.0
                && local_pos.x <= self.bounds.width
                && local_pos.y >= 0.0
                && local_pos.y <= self.bounds.height
        }
        fn on_event(&mut self, event: &Event) -> EventResult {
            match event {
                Event::MouseDown {
                    button: MouseButton::Middle,
                    ..
                }
                | Event::MouseMove { .. } => {
                    self.middle_drags += 1;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        }
        fn properties(&self) -> Vec<(&'static str, String)> {
            vec![("middle_drags", self.middle_drags.to_string())]
        }
    }

    crate::set_scroll_style(ScrollBarStyle::default());
    crate::set_scroll_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
            "Eleven",
        ],
        0,
        font,
    );
    let root = MiddleDragParent {
        bounds: Rect::default(),
        children: vec![Box::new(combo)],
        middle_drags: 0,
    };
    let mut app = App::new(Box::new(root));
    let viewport = Size::new(180.0, 220.0);
    app.layout(viewport);

    app.on_mouse_down(
        12.0,
        viewport.height - 12.0,
        MouseButton::Left,
        Modifiers::default(),
    );
    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);

    app.on_mouse_down(
        40.0,
        viewport.height - 80.0,
        MouseButton::Middle,
        Modifiers::default(),
    );
    app.on_mouse_move(40.0, viewport.height - 130.0);
    app.on_mouse_up(
        40.0,
        viewport.height - 130.0,
        MouseButton::Middle,
        Modifiers::default(),
    );

    assert_eq!(
        app.root()
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "middle_drags")
            .map(|(_, v)| v),
        Some("0".to_string()),
        "middle-drag inside a popup should be consumed by the popup owner, not its parent"
    );
    assert_eq!(
        app.root().children()[0]
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "scroll_offset")
            .map(|(_, v)| v),
        Some("2".to_string()),
        "middle-drag inside the popup should scroll popup content"
    );
}

#[test]
fn test_combo_popup_open_does_not_keep_requesting_redraw() {
    use crate::text::Font;
    use crate::widgets::{ScrollBarStyle, ScrollBarVisibility};
    use std::sync::Arc;

    crate::set_scroll_style(ScrollBarStyle::default());
    crate::set_scroll_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven",
        ],
        0,
        font,
    );

    let mut app = App::new(Box::new(combo));
    let viewport = Size::new(180.0, 220.0);
    app.layout(viewport);
    app.on_mouse_down(
        12.0,
        viewport.height - 12.0,
        MouseButton::Left,
        Modifiers::default(),
    );

    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);

    assert!(
        !app.wants_draw(),
        "an idle open popup should not keep the host in a continuous redraw loop"
    );
}

#[test]
fn test_combo_popup_uses_root_transform_inside_layer() {
    use crate::text::Font;
    use crate::widget::CompositingLayer;
    use crate::{DrawCtx, Event, EventResult, Rect};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    struct Root {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }

    impl Widget for Root {
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
            self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
            let child = &mut self.children[0];
            child.layout(Size::new(140.0, 80.0));
            child.set_bounds(Rect::new(50.0, 120.0, 140.0, 80.0));
            available
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    struct LayerHost {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }

    impl Widget for LayerHost {
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
        fn layout(&mut self, _available: Size) -> Size {
            let child = &mut self.children[0];
            child.layout(Size::new(100.0, 24.0));
            child.set_bounds(Rect::new(10.0, 20.0, 100.0, 24.0));
            Size::new(140.0, 80.0)
        }
        fn compositing_layer(&mut self) -> Option<CompositingLayer> {
            Some(CompositingLayer::new(0.0, 0.0, 0.0, 0.0, 1.0))
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let selected = Rc::new(Cell::new(0_usize));
    let combo = ComboBox::new(vec!["Zero", "One", "Two", "Three"], 0, font)
        .with_selected_cell(Rc::clone(&selected));
    let host = LayerHost {
        bounds: Rect::default(),
        children: vec![Box::new(combo)],
    };
    let root = Root {
        bounds: Rect::default(),
        children: vec![Box::new(host)],
    };
    let mut app = App::new(Box::new(root));
    let viewport = Size::new(240.0, 220.0);
    app.layout(viewport);

    // Combo origin is root (50,120) + host-local (10,20).
    let combo_x = 60.0;
    let combo_y = 140.0;
    app.on_mouse_down(
        combo_x + 8.0,
        viewport.height - (combo_y + 12.0),
        MouseButton::Left,
        Modifiers::default(),
    );
    assert!(
        !app.root().needs_draw(),
        "an open global popup should submit through the late overlay pass without continuously repainting retained parents"
    );

    let mut fb = Framebuffer::new(viewport.width as u32, viewport.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);

    let selected_row_pixel = sample(&fb, 150, 129);
    assert!(
        selected_row_pixel[2] > 150 && selected_row_pixel[0] < 120,
        "popup selected row should paint at the combo's root-space position; sampled {selected_row_pixel:?}"
    );

    // Row 2 is below the LayerHost's normal bounds, so regular parent-bounded
    // hit testing cannot reach the combo. Global overlay hit testing must.
    let row_two_y_up = 85.0;
    app.on_mouse_down(
        combo_x + 8.0,
        viewport.height - row_two_y_up,
        MouseButton::Left,
        Modifiers::default(),
    );
    assert_eq!(selected.get(), 2);
}

/// HiDPI guard: under `device_scale > 1`, the popup must paint at the same
/// logical position the hit-test reports. The submitted request coords come
/// from `ctx.root_transform()` — which already includes the outer device-scale
/// multiplier — and the global drain pass runs while that scale is still on
/// the ctx, so the request must be in LOGICAL units to avoid double-scaling
/// the popup off the visible button. Mobile bug regression: dropdown rendered
/// near the top of the screen while the click area stayed adjacent to the
/// closed combo.
#[test]
fn test_combo_popup_paints_at_logical_root_coords_under_hidpi() {
    use crate::text::Font;
    use crate::{DrawCtx, Event, EventResult, Rect};
    use std::sync::Arc;

    struct ScaleGuard;
    impl Drop for ScaleGuard {
        fn drop(&mut self) {
            crate::set_device_scale(1.0);
        }
    }
    let _g = ScaleGuard;
    crate::set_device_scale(2.0);

    struct PositioningRoot {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for PositioningRoot {
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
            let child = &mut self.children[0];
            child.layout(Size::new(180.0, 24.0));
            child.set_bounds(Rect::new(0.0, 20.0, 180.0, 24.0));
            available
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let combo = ComboBox::new(
        vec![
            "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven",
        ],
        0,
        font,
    );
    let root = PositioningRoot {
        bounds: Rect::default(),
        children: vec![Box::new(combo)],
    };
    let mut app = App::new(Box::new(root));

    // App.layout takes physical input and divides by scale internally.
    let viewport_phys = Size::new(360.0, 440.0);
    app.layout(viewport_phys);

    // Combo at logical y=20 → Y-up logical [20, 44] → physical Y-up [40, 88].
    // Screen y (Y-down) range: [440-88, 440-40] = [352, 400].
    // Screen y=380 → logical y=(440-380)/2=30, inside the closed button.
    app.on_mouse_down(20.0, 380.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(20.0, 380.0, MouseButton::Left, Modifiers::default());
    assert_eq!(
        app.root().children()[0]
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "open")
            .map(|(_, v)| v),
        Some("true".to_string())
    );

    let mut fb = Framebuffer::new(viewport_phys.width as u32, viewport_phys.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
    app.paint(&mut ctx);

    // With the bug, request.y from `ctx.root_transform()` is 40 (physical) and
    // gets multiplied by the outer scale at drain → popup paints at physical
    // Y-up [128, 392], leaving rows 88..128 transparent / clear-colored.
    // With the fix, request.y is 20 logical → popup paints at physical Y-up
    // [88, 396], covering this sample.
    let p = sample(&fb, 180, 100);
    assert!(
        !(p[0] > 200 && p[1] < 50 && p[2] < 50),
        "popup must overpaint the clear color at logical position above the \
         button under HiDPI; sample at physical (180, 100) was {p:?} (red \
         clear color = popup painted somewhere else)"
    );
}
