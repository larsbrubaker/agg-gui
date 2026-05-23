//! ScrollView widget tests — extracted from `tests/widgets.rs` to keep
//! that file under the 800-line guardrail.

use super::*;

/// ScrollView returns the available size from layout and positions its child
/// with a negative y when content is taller than the viewport.
#[test]
fn test_scroll_view_tall_content_child_y() {
    let content = SizedBox::new().with_height(500.0);
    let mut scroll = ScrollView::new(Box::new(content));

    let result = scroll.layout(Size::new(200.0, 200.0));

    assert_eq!(result.width, 200.0);
    assert_eq!(result.height, 200.0);

    // With scroll_offset=0 and content_height=500, viewport_height=200:
    // child_y = 200 - 500 + 0 = -300  (content sticks up beyond viewport top)
    let child_y = scroll.children()[0].bounds().y;
    assert!(
        child_y < 0.0,
        "tall content with offset=0 should have negative child_y; got {child_y}",
    );
}

#[test]
fn test_scroll_view_middle_drag_pans_both_axes() {
    use std::cell::Cell;
    use std::rc::Rc;

    let v_offset = Rc::new(Cell::new(80.0));
    let h_offset = Rc::new(Cell::new(80.0));
    let content = SizedBox::new().with_width(500.0).with_height(500.0);
    let mut scroll = ScrollView::new(Box::new(content))
        .horizontal(true)
        .with_offset_cell(Rc::clone(&v_offset))
        .with_h_offset_cell(Rc::clone(&h_offset));
    scroll.layout(Size::new(200.0, 200.0));

    let mods = Modifiers::default();
    scroll.on_event(&crate::Event::MouseDown {
        pos: crate::Point::new(100.0, 100.0),
        button: MouseButton::Middle,
        modifiers: mods,
    });
    scroll.on_event(&crate::Event::MouseMove {
        pos: crate::Point::new(80.0, 120.0),
    });
    scroll.on_event(&crate::Event::MouseUp {
        pos: crate::Point::new(80.0, 120.0),
        button: MouseButton::Middle,
        modifiers: mods,
    });

    assert_eq!(h_offset.get(), 100.0);
    assert_eq!(v_offset.get(), 100.0);
}

/// Default scroll details mirror egui's floating ScrollStyle defaults.
#[test]
fn test_scroll_bar_style_defaults_match_egui() {
    let style = ScrollBarStyle::default();

    assert_eq!(style.kind, ScrollBarKind::Floating);
    assert_eq!(style.color, ScrollBarColor::Foreground);
    assert_eq!(style.bar_width, 10.0);
    assert_eq!(style.floating_width, 2.0);
    assert_eq!(style.handle_min_length, 12.0);
    assert_eq!(style.outer_margin, 0.0);
    assert_eq!(style.inner_margin, 4.0);
    assert_eq!(style.content_margin, 0.0);
    assert_eq!(style.fade_strength, 0.5);
    assert_eq!(style.fade_size, 20.0);
}

#[test]
fn test_scroll_fade_does_not_overpaint_front_window() {
    use crate::widget::paint_subtree;
    use crate::widgets::{primitives::Stack, window::Window};
    use crate::{DrawCtx, Event, EventResult, Rect};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    struct SolidBox {
        bounds: Rect,
        color: Color,
    }

    impl SolidBox {
        fn new(color: Color) -> Self {
            Self {
                bounds: Rect::default(),
                color,
            }
        }
    }

    impl Widget for SolidBox {
        fn type_name(&self) -> &'static str {
            "SolidBox"
        }

        fn bounds(&self) -> Rect {
            self.bounds
        }

        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }

        fn children(&self) -> &[Box<dyn Widget>] {
            &[]
        }

        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            panic!("SolidBox has no children")
        }

        fn layout(&mut self, available: Size) -> Size {
            self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
            available
        }

        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            ctx.set_fill_color(self.color);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }

        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let offset = Rc::new(Cell::new(120.0));
    let mut scroll_style = ScrollBarStyle::default();
    scroll_style.fade_strength = 1.0;
    scroll_style.fade_size = 80.0;

    let back_content = Box::new(SizedBox::new().with_height(600.0));
    let back_scroll = ScrollView::new(back_content)
        .with_offset_cell(Rc::clone(&offset))
        .with_style(scroll_style);
    let back = Window::new("Back", Arc::clone(&font), Box::new(back_scroll))
        .with_bounds(Rect::new(20.0, 20.0, 260.0, 220.0));

    let front_color = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let front = Window::new(
        "Front",
        Arc::clone(&font),
        Box::new(SolidBox::new(front_color)),
    )
    .with_bounds(Rect::new(70.0, 70.0, 180.0, 140.0));

    let mut stack = Stack::new().add(Box::new(back)).add(Box::new(front));
    stack.set_bounds(Rect::new(0.0, 0.0, 320.0, 260.0));
    stack.layout(Size::new(320.0, 260.0));

    let mut fb = Framebuffer::new(320, 260);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(&mut stack, &mut ctx);
    }

    // This pixel is inside the front window's content area and also inside the
    // back ScrollView's top fade band. The front window paints later, so the
    // scroll fade from the back window must not affect it.
    let p = sample(&fb, 120, 150);
    assert!(
        p[0] > 230 && p[1] < 40 && p[2] < 40,
        "back window scroll fade overpainted the front window; sampled {p:?}"
    );
}

#[test]
fn test_scroll_fade_uses_window_background() {
    use crate::theme::{set_visuals, Visuals};
    use crate::widget::paint_subtree;
    use crate::Rect;
    use std::cell::Cell;
    use std::rc::Rc;

    struct VisualsGuard;

    impl Drop for VisualsGuard {
        fn drop(&mut self) {
            set_visuals(Visuals::dark());
        }
    }

    let _guard = VisualsGuard;
    let visuals = Visuals::light();
    let expected = visuals.window_fill;
    set_visuals(visuals);

    let offset = Rc::new(Cell::new(40.0));
    let mut style = ScrollBarStyle::default();
    style.fade_strength = 1.0;
    style.fade_size = 40.0;

    let content = Box::new(SizedBox::new().with_height(300.0));
    let mut scroll = ScrollView::new(content)
        .with_offset_cell(Rc::clone(&offset))
        .with_bar_visibility(crate::ScrollBarVisibility::AlwaysHidden)
        .with_style(style);
    scroll.layout(Size::new(200.0, 100.0));
    scroll.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));

    let mut fb = Framebuffer::new(200, 100);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(expected);
        paint_subtree(&mut scroll, &mut ctx);
    }

    let p = sample(&fb, 100, 98);
    assert!(
        p[0] > 244 && p[1] > 244 && p[2] > 244,
        "scroll fade should blend toward the window background, got {p:?}"
    );
}
