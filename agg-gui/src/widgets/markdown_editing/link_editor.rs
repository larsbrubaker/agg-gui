//! `MarkdownLinkEditor` — the formatting toolbar's built-in link row,
//! "Link:" [url] OK Remove Cancel, shown under the strip.  Ports agg-sharp's
//! `MarkdownLinkEditor.cs`.
//!
//! A row rather than a popup keeps it in the toolbar's own layout, so it needs
//! no window or popup positioning, and a host with its own dialog can skip it
//! (the toolbar's `on_link_requested`).  Remove shows only when editing an
//! existing link.  Escape cancels.
//!
//! The row's state ([`MarkdownLinkEditor`]: the url, whether it shows, whether
//! Remove shows) is shared with its widget ([`LinkRowWidget`]), whose buttons,
//! Enter and Escape queue a [`LinkRowRequest`] for the toolbar
//! (`format_toolbar`) to run against the document's owner: C#'s `Accepted`,
//! `Removed` and `Cancelled` events.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::{Button, FlexRow, Label, TextField};

/// What the link row asks the toolbar to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkRowRequest {
    /// OK or Enter: link the text to the typed url.
    Accept,
    /// Remove: drop the link, keep its text.
    Remove,
    /// Cancel or Escape: close with no change.
    Cancel,
}

/// The link row's state, shared by the toolbar and the row's widget.
#[derive(Clone)]
pub struct MarkdownLinkEditor {
    url: Rc<RefCell<String>>,
    visible: Rc<Cell<bool>>,
    remove_visible: Rc<Cell<bool>>,
    requests: Rc<RefCell<Vec<LinkRowRequest>>>,
}

impl Default for MarkdownLinkEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownLinkEditor {
    /// A hidden row with an empty url.
    pub fn new() -> Self {
        Self {
            url: Rc::new(RefCell::new(String::new())),
            visible: Rc::new(Cell::new(false)),
            remove_visible: Rc::new(Cell::new(false)),
            requests: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// The url in the box.
    pub fn url(&self) -> String {
        self.url.borrow().clone()
    }

    /// Sets the url in the box.
    pub fn set_url(&self, url: &str) {
        *self.url.borrow_mut() = url.to_string();
        crate::animation::request_draw();
    }

    /// Whether the row shows.
    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }

    /// Whether the Remove button shows (only for an existing link).
    pub fn remove_visible(&self) -> bool {
        self.remove_visible.get()
    }

    /// Shows the row holding `current_url` (`None` for a new link).  The
    /// toolbar moves the focus to the box.
    pub fn open(&self, current_url: Option<&str>) {
        self.set_url(current_url.unwrap_or(""));
        self.remove_visible.set(current_url.is_some());
        self.visible.set(true);
        crate::animation::request_draw();
    }

    /// Hides the row with no request: the caret moved elsewhere, or the
    /// toolbar now edits another document.
    pub fn close(&self) {
        self.visible.set(false);
        crate::animation::request_draw();
    }

    /// Queues `request` (the row's buttons, Enter and Escape) and hides the
    /// row.
    pub(super) fn request(&self, request: LinkRowRequest) {
        self.visible.set(false);
        self.requests.borrow_mut().push(request);
        crate::animation::request_draw();
    }

    /// The requests queued since the last call.
    pub(super) fn take_requests(&self) -> Vec<LinkRowRequest> {
        std::mem::take(&mut *self.requests.borrow_mut())
    }

    /// The row's widget over this state.
    pub(super) fn widget(&self, font: Arc<Font>, focus_id: crate::focus::FocusId) -> LinkRowWidget {
        LinkRowWidget::new(self.clone(), font, focus_id)
    }
}

/// `url` trimmed, with "https://" added when it has no scheme and reads as a
/// web address: a novice types "example.com" and expects the link to open that
/// site, not a file of that name.  It reads as one when the part before any
/// '/' holds a dot and no spaces, and does not end in a file type a document
/// links to (page.md, image.png stay relative).  Paths, anchors and anything
/// with a scheme (mailto:, http://) are left alone.
pub fn with_scheme(url: &str) -> String {
    let url = url.trim();
    if url.is_empty()
        || url.chars().any(char::is_whitespace)
        || url.starts_with('/')
        || url.starts_with('.')
        || url.starts_with('#')
    {
        return url.to_string();
    }

    let host = url.split(['/', '?', '#']).next().unwrap_or("");
    if host.contains(':') || !host.contains('.') || host.ends_with('.') {
        return url.to_string();
    }

    let ending = host[host.rfind('.').map_or(0, |dot| dot + 1)..].to_lowercase();
    if FILE_ENDINGS.contains(&ending.as_str()) {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

const FILE_ENDINGS: [&str; 14] = [
    "md", "markdown", "html", "htm", "txt", "pdf", "png", "jpg", "jpeg", "gif", "svg", "webp",
    "mcx", "stl",
];

/// The row as a widget: "Link:", the url box, OK, Remove (for an existing
/// link) and Cancel.  Escape anywhere in the row cancels.
pub struct LinkRowWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    state: MarkdownLinkEditor,
    font: Arc<Font>,
    focus_id: crate::focus::FocusId,
    built_with_remove: Option<bool>,
}

impl LinkRowWidget {
    fn new(state: MarkdownLinkEditor, font: Arc<Font>, focus_id: crate::focus::FocusId) -> Self {
        let mut base = WidgetBase::new();
        base.h_anchor = HAnchor::STRETCH;
        base.v_anchor = VAnchor::FIT;
        Self {
            bounds: Rect::default(),
            base,
            children: Vec::new(),
            state,
            font,
            focus_id,
            built_with_remove: None,
        }
    }

    // Rebuilds the row when Remove's visibility changes.
    fn build(&mut self) {
        let remove = self.state.remove_visible();
        if self.built_with_remove == Some(remove) {
            return;
        }
        self.built_with_remove = Some(remove);
        let font = &self.font;
        let button =
            |label: &str, tip: &str, request: LinkRowRequest, state: &MarkdownLinkEditor| {
                let state = state.clone();
                Box::new(
                    Button::new(label, Arc::clone(font))
                        .on_click(move || state.request(request))
                        .with_tooltip(tip),
                ) as Box<dyn Widget>
            };
        let accept = self.state.clone();
        let mut row = FlexRow::new()
            .with_gap(4.0)
            .with_inner_padding(Insets::symmetric(3.0, 2.0))
            .add(Box::new(Label::new("Link:", Arc::clone(font))))
            .add_flex(
                Box::new(
                    TextField::new(Arc::clone(font))
                        .with_text_cell(Rc::clone(&self.state.url))
                        .with_placeholder("https://")
                        .with_focus_id(self.focus_id)
                        .on_enter(move |_| accept.request(LinkRowRequest::Accept))
                        .with_tooltip("The web address the link opens"),
                ),
                1.0,
            )
            .add(button(
                "OK",
                "Link the selected text to this address",
                LinkRowRequest::Accept,
                &self.state,
            ));
        if remove {
            row = row.add(button(
                "Remove",
                "Remove the link and keep its text",
                LinkRowRequest::Remove,
                &self.state,
            ));
        }
        row = row.add(button(
            "Cancel",
            "Close without changing the link",
            LinkRowRequest::Cancel,
            &self.state,
        ));
        self.children = vec![Box::new(row)];
    }
}

impl Widget for LinkRowWidget {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "MarkdownLinkEditor"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn is_visible(&self) -> bool {
        self.state.is_visible()
    }

    fn layout(&mut self, available: Size) -> Size {
        self.build();
        let Some(row) = self.children.first_mut() else {
            return Size::new(available.width, 0.0);
        };
        let size = row.layout(Size::new(available.width, available.height));
        row.set_bounds(Rect::new(0.0, 0.0, available.width, size.height));
        Size::new(available.width, size.height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    // Escape cancels before the url box sees it (C#'s OnKeyDown on the row).
    fn preview_event(&mut self, event: &Event) -> EventResult {
        self.on_event(event)
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::KeyDown {
            key: Key::Escape, ..
        } = event
        {
            if self.state.is_visible() {
                self.state.request(LinkRowRequest::Cancel);
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }
}
