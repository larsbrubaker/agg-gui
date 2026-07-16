//! `RichTextEdit` — the **interactive** rich-text editor widget.
//!
//! Builds on the phase-1 pieces: the [`RichDoc`] model, the command engine, and
//! the width-constrained [`layout`](super::layout) that the read-only
//! [`RichTextView`](super::view::RichTextView) also uses. It adds a caret +
//! selection, keyboard editing, mouse click/drag positioning, an internal
//! vertical scrollbar, and time-coalescing undo/redo.
//!
//! # Architecture
//!
//! The *logical* editor state lives in [`RichEditCore`] (document, caret,
//! anchor, pending caret style, undoer) behind an `Rc<RefCell<_>>`. This widget
//! owns only the *view* state — bounds, the cached [`DocLayout`], the scrollbar,
//! and focus/hover flags — and drives the core in response to input. A cheap
//! [`RichEditHandle`] shares the same core so a formatting toolbar can `exec`
//! commands, read [`common_style_of_selection`](RichEditHandle::common_style_of_selection),
//! and undo/redo the very editor being rendered.
//!
//! Submodules keep each concern (and this file) under the 800-line cap:
//! [`core`] (shared logic), `geometry` (caret ↔ pixel mapping), `input`
//! (key/mouse handling), `paint`, and `scroll` (internal vertical scrollbar).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use web_time::Instant;

use crate::event::{Event, EventResult};
use crate::focus::FocusId;
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::{BackbufferCache, BackbufferMode, Widget};
use crate::widgets::multi_click::{MultiClickTracker, SelectGranularity};
use crate::widgets::scrollbar::ScrollbarAxis;

use super::commands::{CommonStyle, RichCommand};
use super::model::{DocPos, DocRange};
use super::layout::{layout_doc, DocLayout, FontResolver};
use super::model::RichDoc;
use super::view::SharedResolver;

pub mod core;
mod geometry;
mod input;
mod paint;
mod scroll;

pub use core::{RichEditCore, RichEditHandle};

#[cfg(test)]
mod tests;

/// An interactive, styled rich-text editor.
pub struct RichTextEdit {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,

    /// Shared logical state — also reachable through a [`RichEditHandle`].
    core: Rc<RefCell<RichEditCore>>,
    resolver: SharedResolver,
    padding: f64,

    /// Cached layout + the width and doc revision it was computed for.
    layout: Option<DocLayout>,
    layout_width: f64,
    layout_doc_rev: u64,

    /// Internal vertical scroll (composes the shared [`ScrollbarAxis`], as
    /// `TextArea` does — see `scroll.rs`).
    vbar: ScrollbarAxis,

    /// Stable id for the programmatic focus channel; `None` opts out.
    focus_request_id: Option<FocusId>,

    /// Ephemeral input state.
    focused: bool,
    hovered: bool,
    selecting_drag: bool,
    focus_time: Option<Instant>,
    blink_last_phase: Cell<u64>,

    /// Multi-click (single / double / triple) detection and the granularity of
    /// the active selection drag. A double-click selects the word, a
    /// triple-click the block; dragging afterwards extends by whole words /
    /// blocks. `select_pivot` is the `(anchor, caret)` range (in document
    /// order) the initiating click selected.
    multi_click: MultiClickTracker,
    select_granularity: SelectGranularity,
    select_pivot: (DocPos, DocPos),
    /// Start instant for the undoer's monotonic clock.
    start: Instant,
    /// Caret revision observed at the previous layout, to scroll external
    /// (toolbar-driven) caret moves into view.
    last_layout_rev: Option<u64>,

    /// Per-widget CPU bitmap cache. Routes the whole editor (bg + selection +
    /// styled runs) through the same LCD-subpixel / grayscale pipeline as
    /// `Label` and `TextField` via `paint_subtree_backbuffered`, so styled text
    /// gets the app's best rendering by default and follows the System settings.
    cache: BackbufferCache,
    /// Last painted signature; a change invalidates [`Self::cache`] in `layout`.
    last_sig: Option<RichEditSig>,
    /// Monotonic counter bumped by [`Self::invalidate_layout`]. The cached
    /// layout is keyed on `(width, doc_rev)` and does NOT observe the demo's
    /// asynchronous font catalog, so an async face arrival reshapes the doc
    /// *without* advancing `core.rev()`. Folding this counter into
    /// [`RichEditSig`] makes such a reshape invalidate the backbuffer too,
    /// otherwise the cache would keep blitting the stale fallback-font bitmap.
    layout_gen: u64,
}

/// Snapshot of every input that affects the cached backbuffer bitmap
/// (background + selection band + styled runs + border). `layout` compares the
/// current sig against the last one and drops the cache on any difference.
///
/// Blink phase and the floating scrollbar paint in `paint_overlay` (after the
/// blit) and are excluded; typography / theme invalidation is handled by the
/// framework's epoch checks. `core_rev` folds in every document, caret and
/// selection change (see [`RichEditCore::rev`]).
#[derive(Clone, Debug, PartialEq)]
struct RichEditSig {
    core_rev: u64,
    focused: bool,
    hovered: bool,
    offset_bits: u64,
    w_bits: u64,
    h_bits: u64,
    layout_gen: u64,
}

impl RichTextEdit {
    /// Create an editor over `initial`, resolving run fonts via `resolver`.
    pub fn new(initial: RichDoc, resolver: SharedResolver) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            core: Rc::new(RefCell::new(RichEditCore::new(initial, 16.0))),
            resolver,
            padding: 8.0,
            layout: None,
            layout_width: -1.0,
            layout_doc_rev: u64::MAX,
            vbar: ScrollbarAxis {
                enabled: true,
                ..ScrollbarAxis::default()
            },
            focus_request_id: None,
            focused: false,
            hovered: false,
            selecting_drag: false,
            focus_time: None,
            blink_last_phase: Cell::new(0),
            multi_click: MultiClickTracker::default(),
            select_granularity: SelectGranularity::default(),
            select_pivot: (DocPos::new(0, 0), DocPos::new(0, 0)),
            start: Instant::now(),
            last_layout_rev: None,
            cache: BackbufferCache::default(),
            last_sig: None,
            layout_gen: 0,
        }
    }

    /// Build the backbuffer-cache invalidation signature from current state.
    fn cache_sig(&self) -> RichEditSig {
        RichEditSig {
            core_rev: self.core.borrow().rev(),
            focused: self.focused,
            hovered: self.hovered,
            offset_bits: self.vbar.offset.to_bits(),
            w_bits: self.bounds.width.to_bits(),
            h_bits: self.bounds.height.to_bits(),
            layout_gen: self.layout_gen,
        }
    }

    /// Set the default font size (points) runs inherit when their style leaves
    /// [`InlineStyle::font_size`](super::model::InlineStyle::font_size) unset.
    pub fn with_font_size(self, size: f64) -> Self {
        self.core.borrow_mut().set_default_font_size(size);
        self
    }
    /// Set the inner padding (logical px) between the border and the text.
    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p;
        self
    }
    /// Set the widget's outer margin.
    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    /// Assign a stable [`FocusId`] so the app can focus this editor
    /// programmatically.
    pub fn with_focus_id(mut self, id: FocusId) -> Self {
        self.focus_request_id = Some(id);
        self
    }
    /// Set the horizontal anchor used when the editor is placed in a layout.
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    /// Set the vertical anchor used when the editor is placed in a layout.
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }

    /// A cheap, cloneable handle onto this editor's shared core, for a toolbar.
    pub fn handle(&self) -> RichEditHandle {
        RichEditHandle::new(Rc::clone(&self.core))
    }

    // ── Public API (delegating to the shared core) ────────────────────────

    /// Apply a formatting command over the current selection/caret.
    pub fn exec(&mut self, cmd: &RichCommand) {
        self.core.borrow_mut().exec(cmd);
    }
    /// Summary of the styles under the current selection (drives toolbar state).
    pub fn common_style_of_selection(&self) -> CommonStyle {
        self.core.borrow().common_style_of_selection()
    }

    /// Select the whole document.
    pub fn select_all(&mut self) {
        self.core.borrow_mut().select_all();
    }
    /// Move the caret to `pos` (clamped), collapsing any selection.
    pub fn set_caret(&mut self, pos: DocPos) {
        self.core.borrow_mut().set_caret(pos, false);
    }
    /// Set the selection to `range` — `range.start` is the anchor, `range.end`
    /// the caret, each clamped onto a valid position.
    pub fn set_selection(&mut self, range: DocRange) {
        self.core.borrow_mut().set_selection(range.start, range.end);
    }
    /// The current selection (anchor → caret; collapsed when they coincide).
    pub fn selection(&self) -> DocRange {
        self.core.borrow().selection()
    }
    /// Replace the document with `doc`, resetting the caret to the start and
    /// **discarding the undo/redo history** (the loaded document is the new
    /// baseline).
    pub fn load(&mut self, doc: RichDoc) {
        self.core.borrow_mut().load(doc);
    }
    /// Undo the last coalesced edit; returns `true` if the document changed.
    pub fn undo(&mut self) -> bool {
        self.core.borrow_mut().undo()
    }
    /// Redo the last undone edit; returns `true` if the document changed.
    pub fn redo(&mut self) -> bool {
        self.core.borrow_mut().redo()
    }
    /// Whether an undo step is available.
    pub fn can_undo(&self) -> bool {
        self.core.borrow().can_undo()
    }
    /// Whether a redo step is available.
    pub fn can_redo(&self) -> bool {
        self.core.borrow().can_redo()
    }

    /// The document's plain text (blocks joined by `\n`).
    pub fn plain_text(&self) -> String {
        self.core.borrow().doc().plain_text()
    }

    /// Force the cached layout to be rebuilt on the next frame.  The layout is
    /// otherwise cached against `(width, doc_revision)` and does **not** observe
    /// the demo's asynchronous font catalog; call this when a newly-loaded font
    /// should be picked up (e.g. after the font-cache epoch advances).
    pub fn invalidate_layout(&mut self) {
        self.layout = None;
        self.layout_width = -1.0;
        self.layout_doc_rev = u64::MAX;
        // Advance the generation so the backbuffer cache re-rasters too: an
        // async font arrival reshapes without bumping `core.rev()`, which the
        // paint signature otherwise keys on.
        self.layout_gen = self.layout_gen.wrapping_add(1);
    }

    // ── Layout cache ──────────────────────────────────────────────────────

    /// Rebuild the cached layout when the width or document revision changed.
    fn ensure_layout(&mut self, width: f64) {
        let doc_rev = self.core.borrow().doc_rev();
        if self.layout.is_some()
            && (self.layout_width - width).abs() < 0.5
            && self.layout_doc_rev == doc_rev
        {
            return;
        }
        let resolver: &FontResolver = &*self.resolver;
        let core = self.core.borrow();
        self.layout = Some(layout_doc(
            core.doc(),
            width,
            core.default_font_size(),
            resolver,
        ));
        self.layout_width = width;
        self.layout_doc_rev = doc_rev;
    }

    /// Content height from the cached layout (0 before the first layout).
    pub(crate) fn content_height(&self) -> f64 {
        self.layout.as_ref().map(|l| l.height).unwrap_or(0.0)
    }
}

impl Widget for RichTextEdit {
    fn type_name(&self) -> &'static str {
        "RichTextEdit"
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

    fn is_focusable(&self) -> bool {
        true
    }
    fn focus_id(&self) -> Option<FocusId> {
        self.focus_request_id
    }
    fn accepts_text_input(&self) -> bool {
        true
    }
    fn text_input_value(&self) -> Option<String> {
        Some(self.plain_text())
    }

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }

    fn backbuffer_mode(&self) -> BackbufferMode {
        // Same decision as `Label` / `TextField` / `TextArea`: LCD coverage
        // buffer when the global toggle is on, grayscale RGBA otherwise. The
        // mode-flip detection in `paint_subtree_backbuffered` re-rasters when
        // the toggle changes.
        if crate::font_settings::lcd_enabled() {
            BackbufferMode::LcdCoverage
        } else {
            BackbufferMode::Rgba
        }
    }

    fn measure_min_height(&self, available_w: f64) -> f64 {
        let resolver: &FontResolver = &*self.resolver;
        let core = self.core.borrow();
        let inner_w = (available_w - self.padding * 2.0).max(1.0);
        layout_doc(core.doc(), inner_w, core.default_font_size(), resolver).height
            + self.padding * 2.0
    }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width.max(self.padding * 2.0 + 20.0);
        let h = available
            .height
            .max(self.padding * 2.0 + 20.0);
        self.bounds = Rect::new(0.0, 0.0, w, h);
        let inner_w = (w - self.padding * 2.0).max(1.0);
        self.ensure_layout(inner_w);
        self.sync_scroll();

        // Feed the undoer once per frame (time-coalescing snapshots); keep
        // frames coming while a change is still settling.
        let time = self.start.elapsed().as_secs_f64();
        let in_flux = self.core.borrow_mut().feed_undo(time);
        if in_flux {
            crate::animation::request_draw_after(std::time::Duration::from_millis(120));
        }

        // Scroll external (toolbar/undo-driven) caret moves into view.
        let rev = self.core.borrow().rev();
        if matches!(self.last_layout_rev, Some(prev) if prev != rev) {
            self.ensure_caret_visible();
        }
        self.last_layout_rev = Some(rev);

        // Drop the cached bitmap when any painted input changed (document,
        // caret/selection, scroll, focus/hover, size). Blink phase and the
        // floating scrollbar are excluded — they paint in `paint_overlay`.
        let sig = self.cache_sig();
        if self.last_sig.as_ref() != Some(&sig) {
            self.last_sig = Some(sig);
            self.cache.invalidate();
        }
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn crate::draw_ctx::DrawCtx) {
        self.paint_content(ctx);
    }

    fn paint_overlay(&mut self, ctx: &mut dyn crate::draw_ctx::DrawCtx) {
        // Floating scrollbar first (after the cache blit) so its hover/fade
        // animation stays live without re-rastering the text bitmap, then the
        // blinking caret.
        self.paint_scrollbar(ctx);
        self.paint_caret(ctx);
    }

    fn hit_test(&self, local: crate::geometry::Point) -> bool {
        local.x >= 0.0
            && local.x <= self.bounds.width
            && local.y >= 0.0
            && local.y <= self.bounds.height
    }

    fn needs_draw(&self) -> bool {
        self.focused || self.scrollbar_animating()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.handle_event(event)
    }
}
