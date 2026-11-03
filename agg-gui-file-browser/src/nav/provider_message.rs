//! [`ProviderMessageBar`]: the line under a list-only browser's list (a popup's) that says what
//! the showing folder has to say about itself (its header text): one line of 8 point text in the
//! look's light text colour, right-aligned with a 6 unit margin all round; no bar when the folder
//! has nothing to say.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::Label;
use agg_gui::{Color, DrawCtx, Event, EventResult, Rect, Size, Widget};

/// The message's size: 8 pt at 96 pixels to the inch, small so it reads as a note.
pub const PROVIDER_MESSAGE_SIZE: f64 = 8.0 / 72.0 * 96.0;
/// The margin round the message.
const MARGIN: f64 = 6.0;

/// The bar holding the message.
pub struct ProviderMessageBar {
    name: String,
    /// The line's colour, reported as the `color` property.
    color: Color,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl ProviderMessageBar {
    /// The bar named `name` for `message` in `color`, or `None` when the message is empty.
    pub fn for_message(message: &str, name: &str, color: Color, font: Arc<Font>) -> Option<Self> {
        if message.is_empty() {
            return None;
        }
        let label = Label::new(message, font)
            .with_font_size(PROVIDER_MESSAGE_SIZE)
            .with_color(color);
        Some(Self {
            name: name.to_string(),
            color,
            bounds: Rect::default(),
            children: vec![Box::new(label)],
        })
    }
}

impl Widget for ProviderMessageBar {
    fn type_name(&self) -> &'static str {
        "ProviderMessageBar"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
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
    /// As wide as it is given, as tall as its line and the margins, the line at the right.
    fn layout(&mut self, available: Size) -> Size {
        let inner = Size::new((available.width - 2.0 * MARGIN).max(0.0), available.height);
        let mut height = 2.0 * MARGIN;
        if let Some(label) = self.children.first_mut() {
            let size = label.layout(inner);
            let width = size.width.min(inner.width);
            label.set_bounds(Rect::new(
                available.width - MARGIN - width,
                MARGIN,
                width,
                size.height,
            ));
            height += size.height;
        }
        Size::new(available.width, height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    /// What tests read: the line's colour.
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("color", format!("{:?}", self.color))]
    }
}
