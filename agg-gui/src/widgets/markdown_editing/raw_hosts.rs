//! The rich editor's Raw block hosts — the part of agg-sharp's
//! `RichDocumentView.cs` that shows each Raw block (front matter, HTML, a
//! nested quote, a rule) rendered by the markdown viewer as a child widget
//! (`rawHosts`, `RawBlockSize`, `CloseRawHosts` and the host placement in
//! `Restack`).
//!
//! A host is a [`MarkdownView`] of the block's source, drawn in the editor's
//! theme colours ([`host_colors`]) rather than the app's visuals, so a dark
//! editor in a light app (or the reverse) shows its raw blocks in its own
//! theme.  The layout style's
//! Raw block sizing callback creates and measures hosts while the view lays
//! out, so the hosts live in a shared [`RichRawHosts`] the style and the edit
//! widget (`edit_widget`) both reach; the widget places the live ones as its
//! children ([`RawHostWidget`]) and closes the rest.  A [`RichRawHost`] is a
//! handle to one host: where it is, whether it has been closed (C#'s
//! `HasBeenClosed`) and the link under a point.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::markdown::{MarkdownColors, MarkdownView};

use super::block::{RichBlock, RichBlockId};
use super::layout_style::{RichLayoutStyle, RichThemeColors};

struct HostInner {
    view: MarkdownView,
    closed: bool,
    /// The host's box in the editor's view coordinates (C#'s position in the
    /// document view).
    in_view: Rect,
}

/// A handle to one Raw block's rendered host.
#[derive(Clone)]
pub struct RichRawHost(Rc<RefCell<HostInner>>);

impl RichRawHost {
    /// Whether the editor has closed this host (its block left the document,
    /// or a theme or scale change rebuilt the hosts).
    pub fn is_closed(&self) -> bool {
        self.0.borrow().closed
    }

    /// The host's box in the editor's view coordinates.
    pub fn bounds_in_view(&self) -> Rect {
        self.0.borrow().in_view
    }

    /// Whether `other` is this same host.
    pub fn ptr_eq(&self, other: &RichRawHost) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// The markdown the host shows.
    pub fn markdown(&self) -> String {
        self.0.borrow().view.markdown().to_string()
    }

    /// The url of the link drawn under `point` (view coordinates), if any.
    pub fn link_at(&self, point: Point) -> Option<String> {
        let inner = self.0.borrow();
        let local = Point::new(point.x - inner.in_view.x, point.y - inner.in_view.y);
        inner.view.link_at_point(local).map(str::to_string)
    }

    fn close(&self) {
        self.0.borrow_mut().closed = true;
    }
}

/// The colours a host draws in for the editor's `theme`: the style's own text
/// and link colours, the code shading ([`RichLayoutStyle::SHADE_ALPHA`] of the
/// text, as the editor shades its code blocks) and the quote bar's alpha for
/// rules.  `None` (C#'s default theme) gives `None`: the host keeps the
/// viewer's default look.  No background is passed: the editor paints its
/// selection beneath a host, which a fill would cover.
pub(super) fn host_colors(
    style: &RichLayoutStyle,
    theme: Option<&RichThemeColors>,
) -> Option<MarkdownColors> {
    let theme = theme?;
    let alpha = |a: u8| f32::from(a) / 255.0;
    Some(MarkdownColors {
        text: style.text_color,
        link: style.link_color,
        code_background: style.code_background_color(),
        rule: theme.text.with_alpha(alpha(90)),
        background: None,
    })
}

/// The Raw block hosts of one editor, keyed by block.
#[derive(Default)]
pub struct RichRawHosts {
    hosts: Vec<(RichBlockId, RichRawHost)>,
    /// Block index to host, as last placed: the link lookup's map.
    placed: Vec<(usize, RichRawHost)>,
}

impl RichRawHosts {
    /// The host showing `block`, created on first use, laid out to
    /// `available` wide; returns its size (C#'s `RawBlockSize`).
    pub(super) fn size(
        &mut self,
        block: &RichBlock,
        available: f64,
        font: &Arc<Font>,
        bold: Option<&Arc<Font>>,
        font_size: f64,
        colors: Option<MarkdownColors>,
    ) -> Size {
        let id = block.id();
        let host = match self.hosts.iter().find(|(b, _)| *b == id) {
            Some((_, host)) => host.clone(),
            None => {
                let mut view = MarkdownView::new(block.original_source.clone(), Arc::clone(font))
                    .with_font_size(font_size)
                    .with_padding(0.0);
                if let Some(bold) = bold {
                    view = view.with_bold_font(Arc::clone(bold));
                }
                if let Some(colors) = colors {
                    view = view.with_colors(colors);
                }
                let host = RichRawHost(Rc::new(RefCell::new(HostInner {
                    view,
                    closed: false,
                    in_view: Rect::default(),
                })));
                self.hosts.push((id, host.clone()));
                host
            }
        };
        let mut inner = host.0.borrow_mut();
        let size = inner.view.layout(Size::new(available, f64::INFINITY));
        let height = f64::max(size.height, 1.0);
        inner.in_view.width = available;
        inner.in_view.height = height;
        Size::new(available, height)
    }

    /// The host of `block`, if it has one.
    pub fn host_of(&self, block: RichBlockId) -> Option<RichRawHost> {
        self.hosts
            .iter()
            .find(|(b, _)| *b == block)
            .map(|(_, h)| h.clone())
    }

    /// Closes every host whose block `keep` rejects (C#'s `CloseRawHosts`).
    pub(super) fn close_unless(&mut self, keep: &dyn Fn(RichBlockId) -> bool) {
        self.hosts.retain(|(block, host)| {
            let kept = keep(*block);
            if !kept {
                host.close();
            }
            kept
        });
        self.placed.retain(|(_, host)| !host.is_closed());
    }

    /// Places block `index`'s host with its bottom-left at `origin` (view
    /// coordinates).
    pub(super) fn place(&mut self, index: usize, block: RichBlockId, origin: Point) {
        if let Some(host) = self.host_of(block) {
            {
                let mut inner = host.0.borrow_mut();
                inner.in_view.x = origin.x;
                inner.in_view.y = origin.y;
            }
            self.placed.retain(|(i, _)| *i != index);
            self.placed.push((index, host));
        }
    }

    /// Forgets the index map before the hosts are placed again.
    pub(super) fn clear_placed(&mut self) {
        self.placed.clear();
    }

    /// The host placed at block `index`.
    pub fn placed_at(&self, index: usize) -> Option<RichRawHost> {
        self.placed
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, h)| h.clone())
    }

    /// The placed hosts, in block order.
    pub(super) fn placed(&self) -> Vec<(usize, RichRawHost)> {
        let mut placed = self.placed.clone();
        placed.sort_by_key(|(i, _)| *i);
        placed
    }
}

/// A host as a child widget of the editor.  Not hit-testable, so every press
/// over a Raw block lands in the editor (C#'s `Selectable = false`).
pub(super) struct RawHostWidget {
    host: RichRawHost,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl RawHostWidget {
    pub(super) fn new(host: RichRawHost, bounds: Rect) -> Self {
        Self {
            host,
            bounds,
            children: Vec::new(),
        }
    }
}

impl Widget for RawHostWidget {
    fn type_name(&self) -> &'static str {
        "RichRawHost"
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
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.host.0.borrow_mut().view.paint(ctx);
    }
    fn hit_test(&self, _local_pos: Point) -> bool {
        false
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn needs_draw(&self) -> bool {
        self.host.0.borrow().view.needs_draw()
    }
}
