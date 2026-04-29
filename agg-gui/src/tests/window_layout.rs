#[test]
fn test_window_content_defaults_to_top_pin() {
    use crate::text::Font;
    use crate::widgets::window::Window;
    use crate::{Label, Widget};
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let content: Box<dyn Widget> = Box::new(Label::new("content", Arc::clone(&font)));
    let mut win = Window::new("Top Pin", Arc::clone(&font), content)
        .with_bounds(crate::geometry::Rect::new(20.0, 20.0, 240.0, 180.0));

    <Window as Widget>::layout(&mut win, crate::geometry::Size::new(640.0, 480.0));

    let content_h = win.bounds().height - 28.0;
    let child = &win.children()[0];
    assert!(
        child.bounds().y > 0.0,
        "non-stretch window content should leave extra whitespace below"
    );
    assert!(
        (child.bounds().y + child.bounds().height - content_h).abs() < 0.001,
        "non-stretch window content should be pinned to the top of the content area"
    );
}

#[test]
fn test_top_pinned_window_content_relayouts_to_final_height() {
    use crate::text::Font;
    use crate::widgets::window::Window;
    use crate::{DrawCtx, Event, EventResult, Rect, Size, Widget};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    struct FixedHeightProbe {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        last_layout_h: Rc<Cell<f64>>,
    }

    impl Widget for FixedHeightProbe {
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
            self.last_layout_h.set(available.height);
            Size::new(available.width, 72.0)
        }

        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let last_layout_h = Rc::new(Cell::new(0.0));
    let content: Box<dyn Widget> = Box::new(FixedHeightProbe {
        bounds: Rect::default(),
        children: Vec::new(),
        last_layout_h: Rc::clone(&last_layout_h),
    });
    let mut win = Window::new("Top Relayout", Arc::clone(&font), content)
        .with_bounds(crate::geometry::Rect::new(20.0, 20.0, 240.0, 220.0));

    <Window as Widget>::layout(&mut win, crate::geometry::Size::new(640.0, 480.0));

    assert_eq!(win.children()[0].bounds().height, 72.0);
    assert_eq!(
        last_layout_h.get(),
        72.0,
        "top-pinned content must receive a final layout pass at its actual height"
    );
}
