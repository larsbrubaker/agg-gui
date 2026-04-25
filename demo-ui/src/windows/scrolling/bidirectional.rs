//! Bidirectional tab: 100 lorem-ipsum paragraphs as non-wrapped single lines,
//! scrolled in both axes.  Our `ScrollView` now supports horizontal scrolling,
//! so this tab uses `.horizontal(true)` and lets the paragraph widget report a
//! wide natural width.

use std::sync::Arc;

use agg_gui::{
    DrawCtx, Event, EventResult, FlexColumn, Font, Rect, ScrollBarVisibility, ScrollView,
    Separator, Size, Widget,
};

use super::helpers::{wrapped_label, LOREM_IPSUM_LONG};

const N_LINES: usize = 100;
const LINE_HEIGHT: f64 = 20.0;
const FONT_SIZE: f64 = 12.0;
const PADDING_X: f64 = 8.0;

struct LoremCanvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    /// Measured pixel width of one lorem-ipsum line at `FONT_SIZE`.  Cached
    /// so layout doesn't re-shape 90+ chars of text every frame.
    text_w: f64,
}

impl LoremCanvas {
    fn new(font: Arc<Font>) -> Self {
        let text_w = agg_gui::measure_text_metrics(&font, LOREM_IPSUM_LONG, FONT_SIZE).width;
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            text_w,
        }
    }
}

impl Widget for LoremCanvas {
    fn type_name(&self) -> &'static str {
        "LoremCanvas"
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
        // Content width is the actual rendered text width plus padding.
        // Note: `available.width` is `f64::MAX/2` when the parent `ScrollView`
        // has horizontal scroll enabled, so we must NOT use `.max(available.width)`
        // — that would explode content_width to infinity and let the user
        // scroll far past the end of the text.
        let _ = available; // intentionally unused
        let w = self.text_w + PADDING_X * 2.0;
        let h = (N_LINES as f64) * LINE_HEIGHT;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(FONT_SIZE);
        ctx.set_fill_color(v.text_color);

        let total_h = (N_LINES as f64) * LINE_HEIGHT;
        for i in 0..N_LINES {
            let y_bottom = total_h - (i as f64 + 1.0) * LINE_HEIGHT;
            let y_text = y_bottom + (LINE_HEIGHT - FONT_SIZE) * 0.5;
            ctx.fill_text(LOREM_IPSUM_LONG, PADDING_X, y_text);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(
        wrapped_label(
            Arc::clone(&font),
            "100 lorem-ipsum paragraphs, rendered as single non-wrapped lines.  \
         Use the scrollbars or shift+wheel for horizontal scroll.",
            11.0,
        ),
        0.0,
    );
    col.push(Box::new(Separator::horizontal()), 0.0);

    let scroll = ScrollView::new(Box::new(LoremCanvas::new(Arc::clone(&font))))
        .horizontal(true)
        .with_bar_visibility(ScrollBarVisibility::AlwaysVisible);
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use agg_gui::{find_widget_by_type, set_scroll_visibility, Font, ScrollBarVisibility, Size};

    struct VisibilityGuard;

    impl Drop for VisibilityGuard {
        fn drop(&mut self) {
            set_scroll_visibility(ScrollBarVisibility::VisibleWhenNeeded);
        }
    }

    #[test]
    fn bidirectional_scroll_area_keeps_both_scrollbars_visible() {
        const BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
        let font = Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"));

        let _guard = VisibilityGuard;
        set_scroll_visibility(ScrollBarVisibility::AlwaysHidden);
        let mut root = super::build(font);
        root.layout(Size::new(360.0, 240.0));

        let scroll = find_widget_by_type(root.as_ref(), "ScrollView")
            .expect("bidirectional tab scroll view");
        let props = scroll.properties();

        assert_property(&props, "v_enabled", "true");
        assert_property(&props, "h_enabled", "true");
        assert_property(&props, "bar_visibility", "AlwaysVisible");
        assert_positive_property(&props, "max_scroll");
        assert_positive_property(&props, "h_max_scroll");
    }

    fn assert_property(props: &[(&'static str, String)], name: &str, expected: &str) {
        let actual = props
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
            .unwrap_or_else(|| panic!("missing property {name}"));
        assert_eq!(actual, expected);
    }

    fn assert_positive_property(props: &[(&'static str, String)], name: &str) {
        let actual = props
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
            .unwrap_or_else(|| panic!("missing property {name}"));
        let value = actual
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("{name} should be a number, got {actual:?}"));
        assert!(value > 0.0, "{name} should be positive, got {value}");
    }
}
