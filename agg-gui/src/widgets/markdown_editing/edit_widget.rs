//! `RichMarkdownEditWidget` — the rich (WYSIWYG) Markdown editor as an agg-gui
//! widget, a port of agg-sharp's `RichMarkdownEditWidget.cs` (with the parts
//! of `RichDocumentView.cs` that were widget work: the Raw hosts, the image
//! cache, the relayout before a paint on a stale style).
//!
//! The widget owns a [`RichEditorState`] (the document, selection, history,
//! view and every input handler) and adds what needs a widget: focus and the
//! caret blink, the vertical scroll bar (which keeps the presses over it, C#'s
//! `OverText`), the 50 ms auto-scroll timer, painting, the Raw blocks' hosts
//! as children (`raw_hosts`), the image cache (`image_cache`) and the theme.
//! Its events and painting are in `edit_widget_events`.
//!
//! Differences from C#:
//! - agg-gui has no `ThemeConfig`: the host gives the fonts and a
//!   [`RichThemeColors`] (`None` for C#'s default theme), and the body size
//!   rich copies are styled at ([`RichMarkdownEditWidget::set_default_font_size`],
//!   C#'s `Theme.DefaultFontSize`).
//! - The image loader is the viewer's provider contract
//!   ([`RichMarkdownEditWidget::set_image_provider`]) plus its remote image
//!   loader for http(s) and `file://` urls
//!   ([`RichMarkdownEditWidget::set_remote_image_loader`]), rather than C#'s
//!   static `MarkdownWidget.RetrieveImageSquenceAsync`.
//! - The toolbar's [`super::RichEditCommands`] is implemented by the state;
//!   reach it through [`RichMarkdownEditWidget::state_mut`].

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use web_time::Instant;

use crate::color::Color;
use crate::geometry::{Point, Rect};
use crate::layout_props::{HAnchor, VAnchor, WidgetBase};
use crate::widget::Widget;
use crate::widgets::scrollbar::ScrollbarAxis;

use super::block::{RichBlockId, RichBlockKind};
use super::block_painter::{RichEditColors, RichLayout};
use super::caret_blink::RichCaretBlink;
use super::document::RichDocument;
use super::document_view::{image_display_size, placeholder_line};
use super::edit_error::RichEditError;
use super::editor_state::{RichEditorState, SCROLL_BAR_ROOM};
use super::image_cache::{RichImageCache, RichImageProvider, RichResolveUrl};
use super::layout_style::{RichLayoutFonts, RichLayoutStyle, RichThemeColors};
use super::raw_hosts::{RawHostWidget, RichRawHost, RichRawHosts};
use super::selection::RichSelection;

/// What the widget, its layout style's callbacks and its link lookup share.
pub(super) struct Shared {
    pub(super) fonts: RichLayoutFonts,
    pub(super) theme: RefCell<Option<RichThemeColors>>,
    pub(super) hosts: RefCell<RichRawHosts>,
    pub(super) images: RefCell<RichImageCache>,
    /// The width blocks lay out to, for an image's display size.
    pub(super) layout_width: Cell<f64>,
}

/// The rich Markdown editor widget; see the module header.
pub struct RichMarkdownEditWidget {
    pub(super) bounds: Rect,
    pub(super) base: WidgetBase,
    pub(super) children: Vec<Box<dyn crate::widget::Widget>>,
    pub(super) state: RichEditorState,
    pub(super) shared: Rc<Shared>,
    pub(super) colors: RichEditColors,
    pub(super) blink: RichCaretBlink,
    pub(super) focused: bool,
    pub(super) empty_hint: Option<String>,
    pub(super) vbar: ScrollbarAxis,
    pub(super) seen_placements: u64,
    pub(super) auto_scroll_due: Option<Instant>,
    /// Set by the state's focus requests (a press, the toolbar's `FocusText`).
    pub(super) focus_wanted: Rc<Cell<bool>>,
    /// The widget's id on agg-gui's focus channel: a focus request asks the
    /// `App` to focus it at its next layout (C#'s `Focus()`).
    pub(super) focus_id: crate::focus::FocusId,
}

impl RichMarkdownEditWidget {
    /// An editor over `fonts` in `theme`'s colours (C#'s default theme when
    /// `None`), showing one empty paragraph.  It stretches both ways.
    pub fn new(fonts: RichLayoutFonts, theme: Option<RichThemeColors>) -> Self {
        let colors = colors_for(theme.as_ref());
        let shared = Rc::new(Shared {
            fonts,
            theme: RefCell::new(theme),
            hosts: RefCell::new(RichRawHosts::default()),
            images: RefCell::new(RichImageCache::new()),
            layout_width: Cell::new(0.0),
        });
        let mut state = RichEditorState::new(style_builder(Rc::clone(&shared)));
        let lookup = Rc::clone(&shared);
        state.set_raw_link_lookup(Box::new(move |index, point| {
            lookup
                .hosts
                .borrow()
                .placed_at(index)
                .and_then(|host| host.link_at(point))
        }));
        let focus_wanted = Rc::new(Cell::new(false));
        let wanted = Rc::clone(&focus_wanted);
        let focus_id = super::format_toolbar::next_focus_id();
        state.on_focus_requested(Box::new(move || {
            wanted.set(true);
            crate::focus::request_focus(focus_id);
        }));
        let mut base = WidgetBase::new();
        base.h_anchor = HAnchor::STRETCH;
        base.v_anchor = VAnchor::STRETCH;
        let seen_placements = state.placements();
        Self {
            bounds: Rect::default(),
            base,
            children: Vec::new(),
            state,
            shared,
            colors,
            blink: RichCaretBlink::new(),
            focused: false,
            empty_hint: None,
            vbar: ScrollbarAxis::default(),
            seen_placements,
            auto_scroll_due: None,
            focus_wanted,
            focus_id,
        }
    }

    /// The editor's state: document, selection, history, view, handlers.
    pub fn state(&self) -> &RichEditorState {
        &self.state
    }

    /// The editor's state for edits, handlers and the toolbar's
    /// [`super::RichEditCommands`].  Raw hosts follow at the next layout or
    /// paint.
    pub fn state_mut(&mut self) -> &mut RichEditorState {
        &mut self.state
    }

    /// The document as markdown; an untouched document reads back
    /// byte-identical.
    pub fn markdown(&self) -> String {
        self.state.markdown()
    }

    /// Parses and shows `markdown` with the caret at its start, scrolled to
    /// the top, with an empty history.
    pub fn set_markdown(&mut self, markdown: &str) -> Result<(), RichEditError> {
        let result = self.state.set_markdown(markdown);
        self.sync_raw_hosts();
        result
    }

    /// The document shown.
    pub fn document(&self) -> &RichDocument {
        self.state.document()
    }

    /// The document for an edit made outside the pipeline; call
    /// [`Self::relayout`] after it.
    pub fn document_mut(&mut self) -> &mut RichDocument {
        self.state.document_mut()
    }

    /// Shows `document` (C#'s `Document` setter and `SetDocument`, an undo or
    /// redo result) with `selection` clamped into it.
    pub fn set_document(
        &mut self,
        document: RichDocument,
        selection: RichSelection,
    ) -> Result<(), RichEditError> {
        let result = self.state.set_document(document, selection);
        self.sync_raw_hosts();
        result
    }

    /// The selection.
    pub fn selection(&self) -> RichSelection {
        self.state.selection()
    }

    /// Sets the selection (C#'s `Selection` setter).
    pub fn select(&mut self, selection: RichSelection) {
        self.state.select(selection);
    }

    /// Re-lays out blocks `first..=last` after an edit of [`Self::document_mut`].
    pub fn relayout(&mut self, first: usize, last: usize) -> Result<(), RichEditError> {
        let result = self.state.relayout(first, last);
        self.sync_raw_hosts();
        result
    }

    /// Lays every block out again after an edit that touched many.
    pub fn relayout_all(&mut self) -> Result<(), RichEditError> {
        let result = self.state.relayout_all();
        self.sync_raw_hosts();
        result
    }

    /// True when the document is one empty paragraph.
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// The hint an empty document shows, focused or not (e.g. "No notes yet").
    pub fn empty_hint(&self) -> Option<&str> {
        self.empty_hint.as_deref()
    }

    /// Sets the empty hint.
    pub fn set_empty_hint(&mut self, hint: Option<String>) {
        self.empty_hint = hint;
        crate::animation::request_draw();
    }

    /// The theme colours (`None`: C#'s default theme).
    pub fn theme(&self) -> Option<RichThemeColors> {
        *self.shared.theme.borrow()
    }

    /// Sets the theme: new colours, a new style, every block (and Raw host)
    /// laid out again.
    pub fn set_theme(&mut self, theme: Option<RichThemeColors>) -> Result<(), RichEditError> {
        *self.shared.theme.borrow_mut() = theme;
        self.colors = colors_for(theme.as_ref());
        let result = self.state.invalidate_style();
        self.sync_raw_hosts();
        result
    }

    /// The body text size, in points, a copy's styled HTML uses (C#'s
    /// `Theme.DefaultFontSize`).
    pub fn set_default_font_size(&mut self, points: f64) {
        self.state.copy_font_size = points;
    }

    /// The colours besides the style's (C#'s `Colors`).
    pub fn colors(&self) -> &RichEditColors {
        &self.colors
    }

    /// Replaces the colours besides the style's.
    pub fn set_colors(&mut self, colors: RichEditColors) {
        self.colors = colors;
    }

    /// Sets where images come from: RGBA8 pixels, top row first, and size,
    /// as the markdown viewer's `with_image_provider` takes them.  An http(s) or
    /// `file://` url the provider does not supply goes to the remote image
    /// loader ([`Self::set_remote_image_loader`]), off the UI frame.
    pub fn set_image_provider(&mut self, provider: Option<RichImageProvider>) {
        self.shared.images.borrow_mut().set_provider(provider);
    }

    /// Sets the loader that reads http(s) and `file://` images (the viewer's
    /// `with_remote_image_loader`); `None` uses the process default
    /// ([`crate::set_default_remote_image_loader`]), and with neither such an
    /// image does not load.
    pub fn set_remote_image_loader(
        &mut self,
        loader: Option<std::sync::Arc<dyn crate::RemoteImageLoader>>,
    ) {
        self.shared
            .images
            .borrow_mut()
            .set_remote_image_loader(loader);
    }

    /// Maps an image's url as written in the markdown to what the loader
    /// fetches (C#'s `ResolveImageUrl`).
    pub fn set_resolve_image_url(&mut self, resolve: Option<RichResolveUrl>) {
        self.shared.images.borrow_mut().set_resolve_url(resolve);
    }

    /// Mac or Windows/Linux key bindings.
    pub fn set_use_mac_key_bindings(&mut self, mac: bool) {
        self.state.use_mac_key_bindings = mac;
    }

    /// Adds a handler raised with a link's url when it is Cmd/Ctrl+clicked.
    pub fn on_link_clicked(&mut self, handler: Box<dyn FnMut(&str)>) {
        self.state.on_link_clicked(handler);
    }

    /// Adds a handler asked first on paste; true means the host pasted (an
    /// image it saved) and the editor reads no clipboard text (C#'s
    /// `PasteRequested` with `Handled`).
    pub fn on_paste_requested(&mut self, handler: Box<dyn FnMut() -> bool>) {
        self.state.on_paste_requested(handler);
    }

    /// Adds a handler raised after every edit, undo and redo.
    pub fn on_document_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.state.on_document_changed(handler);
    }

    /// Adds a handler raised on every change of the selection.
    pub fn on_selection_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.state.on_selection_changed(handler);
    }

    /// Adds a handler raised when a Raw block is double-clicked, with where
    /// its source starts in [`Self::markdown`].
    pub fn on_raw_block_activated(&mut self, handler: Box<dyn FnMut(usize)>) {
        self.state.on_raw_block_activated(handler);
    }

    /// Adds a handler raised by Cmd/Ctrl+K.
    pub fn on_link_requested(&mut self, handler: Box<dyn FnMut()>) {
        self.state.on_link_requested(handler);
    }

    /// Whether the editor has keyboard focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether a press or the toolbar has asked for focus since the last
    /// call (C#'s `Focus()`).  Each request also goes to agg-gui's focus
    /// channel ([`crate::focus::request_focus`] with [`Widget::focus_id`]),
    /// so the `App` focuses the editor at its next layout.
    pub fn take_focus_request(&self) -> bool {
        self.focus_wanted.replace(false)
    }

    /// Whether the caret is in the showing half of its blink.
    pub fn caret_showing(&self) -> bool {
        self.blink.showing()
    }

    /// How many blocks have been laid out since the editor was made.
    pub fn layout_count(&self) -> usize {
        self.state.view.layout_count()
    }

    /// How many blocks have a layout.
    pub fn laid_out_block_count(&self) -> usize {
        self.state.view.laid_out_count()
    }

    /// The blocks painted by the last paint.
    pub fn drawn_blocks(&self) -> Range<usize> {
        self.state.view.drawn_blocks()
    }

    /// The style the current layouts were built with.
    pub fn style(&self) -> Option<&RichLayoutStyle> {
        self.state.style()
    }

    /// A block's box in view coordinates (see [`Self::view_to_editor`]).
    pub fn block_bounds(&self, index: usize) -> Option<Rect> {
        self.state.view.block_bounds(index)
    }

    /// The layout block `index` was last laid out to.
    pub fn block_layout(&self, index: usize) -> Option<&RichLayout> {
        self.state.view.layout_of(index)
    }

    /// The block block `index`'s layout was built from.
    pub fn block_layout_id(&self, index: usize) -> Option<RichBlockId> {
        self.state.view.block_id_of(index)
    }

    /// Where block `index`'s layout origin sits in view coordinates.
    pub fn block_origin(&self, index: usize) -> Option<f64> {
        self.state.view.origin_of(index)
    }

    /// A view-coordinates point in the widget's own coordinates (C#'s
    /// `DocumentView.TransformToParentSpace(editor, ...)`).
    pub fn view_to_editor(&self, point: Point) -> Point {
        self.state.from_view(point)
    }

    /// The host showing Raw block `index` rendered, or `None` for other
    /// blocks (C#'s `RawBlockWidget`).
    pub fn raw_block_widget(&self, index: usize) -> Option<RichRawHost> {
        let block = self.state.document().blocks.get(index)?;
        if block.kind != RichBlockKind::Raw {
            return None;
        }
        self.shared.hosts.borrow().host_of(block.id())
    }

    /// How far the view is scrolled down from its top.
    pub fn scroll_offset_from_top(&self) -> f64 {
        self.state.scroll_offset_from_top()
    }

    /// Scrolls to `offset` from the top, clamped to the document.
    pub fn set_scroll_offset_from_top(&mut self, offset: f64) {
        self.state.set_scroll_offset_from_top(offset);
        self.place_raw_hosts();
    }

    /// The furthest the view scrolls.
    pub fn max_scroll_from_top(&self) -> f64 {
        f64::max(self.state.view.height() - self.state.viewport_height, 0.0)
    }

    /// Closes the hosts of blocks no longer shown and places the live ones
    /// as children (C#'s `CloseRawHosts` and the host placement in
    /// `Restack`).  Before the view has a width, hosts of blocks the document
    /// still has are kept (C#'s `RelayoutAll`).
    pub(super) fn sync_raw_hosts(&mut self) {
        let document = self.state.document();
        let view = &self.state.view;
        let laid_out = view.laid_out_count() == document.blocks.len() && view.width() > 0.0;
        let live: Vec<RichBlockId> = document
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                b.kind == RichBlockKind::Raw && (!laid_out || view.raw_host_origin(*i).is_some())
            })
            .map(|(_, b)| b.id())
            .collect();
        self.shared
            .hosts
            .borrow_mut()
            .close_unless(&|block| live.contains(&block));
        self.place_raw_hosts();
    }

    /// Moves the live hosts to their blocks and rebuilds the child list.
    pub(super) fn place_raw_hosts(&mut self) {
        let mut hosts = self.shared.hosts.borrow_mut();
        hosts.clear_placed();
        for (i, block) in self.state.document().blocks.iter().enumerate() {
            if block.kind != RichBlockKind::Raw {
                continue;
            }
            if let Some(origin) = self.state.view.raw_host_origin(i) {
                hosts.place(i, block.id(), origin);
            }
        }
        self.children.clear();
        for (_, host) in hosts.placed() {
            let in_view = host.bounds_in_view();
            let at = self.state.from_view(Point::new(in_view.x, in_view.y));
            let bounds = Rect::new(at.x, at.y, in_view.width, in_view.height);
            self.children
                .push(Box::new(RawHostWidget::new(host, bounds)) as Box<dyn Widget>);
        }
    }

    /// Re-lays out the blocks showing images that have arrived (C#'s
    /// `ImageLoaded`).
    pub(super) fn relayout_arrived_images(&mut self) -> Result<(), RichEditError> {
        let arrived = self.shared.images.borrow().take_arrived();
        if arrived.is_empty() {
            return Ok(());
        }
        let images = Rc::clone(&self.shared);
        let showing = self
            .state
            .view
            .blocks_showing(self.state.document(), &|atom| {
                arrived.contains(&images.images.borrow().url_of(atom))
            });
        let indices: Vec<usize> = self
            .state
            .document()
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| showing.contains(&b.id()))
            .map(|(i, _)| i)
            .collect();
        if let (Some(first), Some(last)) = (indices.first(), indices.last()) {
            self.relayout(*first, *last)?;
        }
        Ok(())
    }

    /// Sizes the editor's state to the widget and records the layout width
    /// the image sizing reads.
    pub(super) fn size_state(&mut self) -> Result<(), RichEditError> {
        let (width, height) = (self.bounds.width, self.bounds.height);
        self.shared
            .layout_width
            .set(f64::max(width - SCROLL_BAR_ROOM, 0.0));
        self.state.set_viewport(width, height)?;
        self.state.set_text_scale(
            crate::device_scale::device_scale(),
            crate::font_settings::current_typography_epoch(),
        )
    }
}

/// The style builder the state calls when the style is stale: closes the Raw
/// hosts (built with the old theme and scale; rebuilt as their blocks lay
/// out), then builds the theme's style with the image and Raw block sizing
/// callbacks (C#'s `BuildStyle`).
fn style_builder(shared: Rc<Shared>) -> Box<dyn FnMut() -> RichLayoutStyle> {
    Box::new(move || {
        shared.hosts.borrow_mut().close_unless(&|_| false);
        let theme = *shared.theme.borrow();
        let mut style = RichLayoutStyle::with_theme(shared.fonts.clone(), theme.as_ref(), None);
        let placeholder = placeholder_line(&style);
        let body_px = style.block_face(&Default::default()).em_px();
        let images = Rc::clone(&shared);
        style.image_size = Some(Box::new(move |atom| {
            let loaded = images.images.borrow().loaded(atom);
            image_display_size(
                loaded.map(|image| (image.width, image.height)),
                images.layout_width.get(),
                placeholder,
            )
        }));
        let hosts = Rc::clone(&shared);
        style.raw_block_size = Some(Box::new(move |block, available| {
            hosts.hosts.borrow_mut().size(
                block,
                available,
                &hosts.fonts.regular,
                hosts.fonts.bold.as_ref(),
                body_px,
            )
        }));
        style
    })
}

/// The editor's colours for a theme (C#'s `ColorsFor`); C#'s default
/// `ThemeConfig` (black text, the #7AD7F0 accent) when `None`.
pub fn colors_for(theme: Option<&RichThemeColors>) -> RichEditColors {
    let text = theme.map_or(Color::black(), |t| t.text);
    let accent = theme.map_or(Color::from_rgb8(0x7A, 0xD7, 0xF0), |t| t.accent);
    let alpha = |a: u8| f32::from(a) / 255.0;
    RichEditColors {
        // MinimalShade is a black tint, invisible on a dark background; the text colour shows on both.
        code_background: text.with_alpha(alpha(RichLayoutStyle::SHADE_ALPHA)),
        // The viewer's quote bar (QuoteBlockX) is the text colour at this alpha.
        quote_bar: text.with_alpha(alpha(90)),
        selection: accent.with_alpha(alpha(100)),
        caret: text,
        hint: text.with_alpha(alpha(110)),
    }
}
