//! A search's lines between its results (`BrowserState::list_slots`): while any part of the
//! search runs, a "Searching for '{query}'" line with a spinner; then each place searched gets a
//! header ([`SectionHeaderRow`]): an expand toggle showing the arrow and the place's name, then
//! "(n)", "No results" or the place's error once it is done (a spinner while it still searches),
//! over a rule. A left click anywhere on a header rolls its section up or opens it again
//! (`BrowserState::toggle_search_section`); the toggle is not a target of its own. Both views
//! place the headers between their rows (`list/row.rs`, `list/icon_view.rs`), each across the
//! list's whole width. The headers' names come from the look (`BrowserNames::search_status`,
//! `search_section`), the toggle's being the header's and " Toggle".

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::{measure_advance, Font};
use agg_gui::{
    measure_text_metrics, Color, DrawCtx, Event, EventResult, Label, MouseButton, Point, Rect,
    Size, Spinner, Widget,
};
use agg_gui_files::tr;

use crate::look::glyph_face;
use crate::state::{BrowserState, ListSlot};

use super::build::ListCx;

/// A header's margin: left, bottom, right, top (more above, to set sections apart).
const MARGIN: (f64, f64, f64, f64) = (3.0, 3.0, 3.0, 8.0);
/// A header's padding above its rule.
const PADDING_BOTTOM: f64 = 3.0;
/// The gap before the status text.
const STATUS_GAP: f64 = 6.0;
/// The gap between the toggle's arrow and its name.
const ARROW_GAP: f64 = 4.0;
/// Font Awesome's caret-down (open) and caret-right (rolled up).
const OPEN_ARROW: char = '\u{F0D7}';
const CLOSED_ARROW: char = '\u{F0DA}';

/// A search's line between its results (see the module header).
pub struct SectionHeaderRow {
    id: String,
    /// The section a click rolls up or opens; `None` for the "Searching for" line.
    section: Option<String>,
    state: Rc<RefCell<BrowserState>>,
    rule: Color,
    pressed: bool,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl SectionHeaderRow {
    /// The line for `slot` in `cx`'s list; `None` for a row's slot.
    pub fn for_slot(slot: &ListSlot, cx: &ListCx) -> Option<Self> {
        let style = &cx.style;
        let names = cx.look.names();
        let label = |text: &str, color: Color| -> Box<dyn Widget> {
            Box::new(
                Label::new(text, Arc::clone(&cx.font))
                    .with_font_size(style.font_size)
                    .with_color(color),
            )
        };
        let spinner = || -> Box<dyn Widget> { Box::new(Spinner::small().with_color(style.accent)) };
        let (id, section, children) = match slot {
            ListSlot::Entry(_) => return None,
            ListSlot::Searching { query } => (
                names.search_status.clone(),
                None,
                vec![
                    label(
                        &tr("Searching for '{0}'").replace("{0}", query),
                        style.text_color,
                    ),
                    spinner(),
                ],
            ),
            ListSlot::Section(header) => {
                let id = names.search_section_for(&header.name);
                let toggle = SectionToggle {
                    id: format!("{id} Toggle"),
                    text: header.name.clone(),
                    open: !header.collapsed,
                    font: Arc::clone(&cx.font),
                    font_size: style.font_size,
                    color: style.text_color,
                    bounds: Rect::default(),
                    children: Vec::new(),
                };
                let mut children: Vec<Box<dyn Widget>> = vec![Box::new(toggle)];
                match &header.status {
                    Some(status) => children.push(label(status, style.light_text_color)),
                    // No status yet means still searching.
                    None => children.push(spinner()),
                }
                (id, Some(header.name.clone()), children)
            }
        };
        Some(Self {
            id,
            section,
            state: Rc::clone(&cx.state),
            rule: style.section_rule,
            pressed: false,
            bounds: Rect::default(),
            children,
        })
    }
}

impl Widget for SectionHeaderRow {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "SectionHeaderRow"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.id)
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
    /// The whole header is the click target, the toggle included.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        true
    }
    fn layout(&mut self, available: Size) -> Size {
        let (left, bottom, right, top) = MARGIN;
        let inner = (available.width - left - right).max(0.0);
        let sizes: Vec<Size> = self
            .children
            .iter_mut()
            .map(|c| c.layout(Size::new(inner, available.height)))
            .collect();
        let content_h = sizes.iter().map(|s| s.height).fold(0.0, f64::max);
        let base = bottom + 1.0 + PADDING_BOTTOM;
        let mut x = left;
        for (i, (child, size)) in self.children.iter_mut().zip(&sizes).enumerate() {
            if i > 0 {
                x += STATUS_GAP;
            }
            let width = size.width.min(inner);
            let y = base + (content_h - size.height) / 2.0;
            child.set_bounds(Rect::new(x, y, width, size.height));
            x += width;
        }
        Size::new(available.width, base + content_h + top)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (left, bottom, right, _) = MARGIN;
        ctx.set_fill_color(self.rule);
        ctx.begin_path();
        ctx.rect(left, bottom, self.bounds.width - left - right, 1.0);
        ctx.fill();
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        let Some(section) = &self.section else {
            return EventResult::Ignored;
        };
        match event {
            // Right and middle clicks belong to the list's own menu.
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                self.pressed = true;
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                if std::mem::take(&mut self.pressed) && self.hit_test(*pos) {
                    // The list builds again at its next layout, after this click is delivered.
                    self.state.borrow_mut().toggle_search_section(section);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("section", self.section.clone().unwrap_or_default())]
    }
}

/// A section's expand toggle: the arrow (down while open) and the section's name. Clicks are
/// its header's.
struct SectionToggle {
    id: String,
    text: String,
    open: bool,
    font: Arc<Font>,
    font_size: f64,
    color: Color,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl SectionToggle {
    fn arrow(&self) -> String {
        match self.open {
            true => OPEN_ARROW,
            false => CLOSED_ARROW,
        }
        .to_string()
    }
}

impl Widget for SectionToggle {
    fn type_name(&self) -> &'static str {
        "SectionToggle"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.id)
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
    fn layout(&mut self, _available: Size) -> Size {
        let arrow = measure_advance(&glyph_face(), &self.arrow(), self.font_size);
        let text = measure_text_metrics(&self.font, &self.text, self.font_size);
        Size::new(arrow + ARROW_GAP + text.width, text.line_height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        let glyphs = glyph_face();
        let arrow = self.arrow();
        let m = measure_text_metrics(&self.font, &self.text, self.font_size);
        let baseline = (h - (m.ascent - m.descent)) / 2.0;
        ctx.set_fill_color(self.color);
        ctx.set_font(Arc::clone(&glyphs));
        ctx.set_font_size(self.font_size);
        ctx.fill_text(&arrow, 0.0, baseline);
        let x = measure_advance(&glyphs, &arrow, self.font_size) + ARROW_GAP;
        ctx.set_font(Arc::clone(&self.font));
        ctx.fill_text(&self.text, x, baseline);
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("text", self.text.clone()),
            ("checked", self.open.to_string()),
        ]
    }
}
