//! `RichTextView` — a **read-only** widget that paints a laid-out [`RichDoc`].
//!
//! Phase 1 scope: no editing, no cursor, no interaction.  The widget shares its
//! document via `Rc<RefCell<RichDoc>>` (phase 2 will wrap that in the existing
//! undo stack and add a keyboard editor on top).  It reports its content height
//! from `layout`, so it drops cleanly into a [`ScrollView`](crate::widgets::ScrollView).
//!
//! Painting walks the [`DocLayout`] top-down and flips
//! into the framework's Y-up space: per-run font/size/colour via `ctx.fill_text`,
//! highlight as a filled rect behind the run, underline / strikethrough as 1px
//! lines at metric-derived offsets, and list markers hung in the left gutter.
//!
//! # Caching note
//!
//! The layout is recomputed whenever the available width changes.  Because the
//! document lives behind a shared `RefCell`, external mutations do **not**
//! auto-invalidate; call [`RichTextView::invalidate`] after editing the doc.
//! Phase 2 should replace this with a document revision counter.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::layout::{layout_doc, DocLayout, FontResolver};
use super::model::{InlineStyle, RichDoc};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

/// Shared font resolver: maps a run's [`InlineStyle`] to a concrete font.
pub type SharedResolver = Rc<dyn Fn(&InlineStyle) -> Arc<Font>>;

/// A non-interactive rich-text display.
pub struct RichTextView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    doc: Rc<RefCell<RichDoc>>,
    resolver: SharedResolver,
    default_font_size: f64,
    /// Cached layout + the width it was computed for.
    layout: Option<DocLayout>,
    layout_width: f64,
}

impl RichTextView {
    /// Create a read-only view over `doc`, resolving run fonts via `resolver`.
    pub fn new(doc: Rc<RefCell<RichDoc>>, resolver: SharedResolver) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            doc,
            resolver,
            default_font_size: 16.0,
            layout: None,
            layout_width: -1.0,
        }
    }

    /// Set the default font size (points) runs inherit when their style leaves
    /// [`InlineStyle::font_size`](super::model::InlineStyle::font_size) unset.
    pub fn with_default_font_size(mut self, size: f64) -> Self {
        self.default_font_size = size;
        self
    }

    /// Set the widget's outer margin.
    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }

    /// Drop the cached layout so the next `layout` recomputes it — call after
    /// mutating the shared document.
    pub fn invalidate(&mut self) {
        self.layout = None;
        self.layout_width = -1.0;
    }

    /// Content height at the last laid-out width (0 before first layout).
    pub fn content_height(&self) -> f64 {
        self.layout.as_ref().map(|l| l.height).unwrap_or(0.0)
    }

    /// Measured content width from the last layout (0 before first layout).
    /// This is `max(available_width, widest_block)`, so a block wider than the
    /// slot is surfaced rather than silently clipped to the available width.
    pub fn content_width(&self) -> f64 {
        self.layout.as_ref().map(|l| l.width).unwrap_or(0.0)
    }

    fn ensure_layout(&mut self, width: f64) {
        if self.layout.is_some() && (self.layout_width - width).abs() < 0.5 {
            return;
        }
        let resolver: &FontResolver = &*self.resolver;
        let doc = self.doc.borrow();
        self.layout = Some(layout_doc(
            &doc,
            width,
            self.default_font_size,
            resolver,
        ));
        self.layout_width = width;
    }
}

impl Widget for RichTextView {
    fn type_name(&self) -> &'static str {
        "RichTextView"
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

    /// Read-only: never independently hittable, so pointer events reach an
    /// enclosing scroll container unimpeded.
    fn hit_test(&self, _: Point) -> bool {
        false
    }

    fn layout(&mut self, available: Size) -> Size {
        let width = available.width.max(1.0);
        self.ensure_layout(width);
        // Report the measured content width (which is at least `width`) so a
        // block wider than the slot is surfaced to the parent instead of being
        // reported as exactly the available width.
        Size::new(self.content_width().max(width), self.content_height())
    }

    fn measure_min_height(&self, available_w: f64) -> f64 {
        // Compute a throwaway layout at the requested width.
        let resolver: &FontResolver = &*self.resolver;
        let doc = self.doc.borrow();
        layout_doc(&doc, available_w.max(1.0), self.default_font_size, resolver).height
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.ensure_layout(self.bounds.width.max(1.0));
        let Some(layout) = &self.layout else {
            return;
        };
        let h = self.bounds.height;
        let default_color = ctx.visuals().text_color;

        // Walk blocks/lines top-down, tracking distance from the document top.
        let mut y_top = 0.0f64;
        for block in &layout.blocks {
            // List marker sits on the first line's baseline.
            if let (Some(marker), Some(font), Some(first)) =
                (&block.marker, &block.marker_font, block.lines.first())
            {
                let baseline_up = h - (y_top + first.baseline_from_top);
                let color = block
                    .lines
                    .first()
                    .and_then(|l| l.fragments.first())
                    .and_then(|f| f.style.text_color)
                    .unwrap_or(default_color);
                ctx.set_font(Arc::clone(font));
                ctx.set_font_size(block.marker_font_size);
                ctx.set_fill_color(color);
                ctx.fill_text(marker, block.marker_x, baseline_up);
            }

            for line in &block.lines {
                let line_top = y_top;
                let baseline_up = h - (line_top + line.baseline_from_top);
                let line_bottom_up = h - (line_top + line.height);
                let text_x0 = block.text_left + line.align_dx;

                for frag in &line.fragments {
                    let fx = text_x0 + frag.x;

                    // Highlight fill behind the fragment's line cell.
                    if let Some(hl) = frag.style.highlight {
                        ctx.set_fill_color(hl);
                        ctx.begin_path();
                        ctx.rect(fx, line_bottom_up, frag.width, line.height);
                        ctx.fill();
                    }

                    let color = frag.style.text_color.unwrap_or(default_color);
                    ctx.set_font(Arc::clone(&frag.font));
                    ctx.set_font_size(frag.font_size);
                    ctx.set_fill_color(color);
                    ctx.fill_text(&frag.text, fx, baseline_up);

                    // Decorations.
                    if frag.style.underline || frag.style.strikethrough {
                        ctx.set_stroke_color(color);
                        ctx.set_line_width(1.0_f64.max(frag.font_size * 0.06));
                    }
                    if frag.style.underline {
                        let uy = baseline_up - (frag.font_size * 0.12).max(1.5);
                        stroke_hline(ctx, fx, fx + frag.width, uy);
                    }
                    if frag.style.strikethrough {
                        let sy = baseline_up + frag.font_size * 0.28;
                        stroke_hline(ctx, fx, fx + frag.width, sy);
                    }
                }
                y_top += line.height;
            }
        }
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

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Stroke a 1px-ish horizontal line at Y-up `y` from `x0` to `x1`.
fn stroke_hline(ctx: &mut dyn DrawCtx, x0: f64, x1: f64, y: f64) {
    ctx.begin_path();
    ctx.move_to(x0, y);
    ctx.line_to(x1, y);
    ctx.stroke();
}

/// One-line resolver for the common single-font case: every run is shaped and
/// measured with `font`, regardless of its [`InlineStyle`].
///
/// This is the fastest way to get a *working* editor on screen — pass it one
/// [`Font`] and you have a functional [`RichTextView`] or
/// [`RichTextEdit`](super::RichTextEdit). Bold / italic / family requests all
/// fall back to `font`, so styled text still edits and lays out correctly; it
/// simply renders in the one typeface until you supply a resolver that returns
/// real variant faces (see the resolver contract below).
///
/// # Resolver contract
///
/// A resolver is any `Fn(&InlineStyle) -> Arc<Font>`. It maps a run's *style* to
/// the concrete [`Font`] used to **shape and measure** that run. It is called
/// once per run during layout. What each input field means:
///
/// * [`bold`](InlineStyle::bold) / [`italic`](InlineStyle::italic) — return the
///   matching real face if you have one (e.g. an Italic `.ttf`). The layout
///   engine applies **no** faux-bold / faux-slant, so bold/italic only *look*
///   different when the resolver hands back a genuinely different face.
/// * [`font_family`](InlineStyle::font_family) — `Some(name)` selects a family;
///   `None` means "the app default". Map unknown families to your base font.
/// * [`font_size`](InlineStyle::font_size) — **ignore this.** Size is applied by
///   the layout engine, not the resolver; a resolver picks the *typeface* only.
/// * `underline` / `strikethrough` / `text_color` / `highlight` — decorations
///   the painter draws; not the resolver's concern.
///
/// **Returning the base font for any style is always safe** — text still shapes,
/// measures, wraps, and edits; it just won't show that style's typeface. Build
/// up from there face by face.
///
/// For a full multi-font resolver backed by a system-font catalog with real
/// bold/italic faces, see the demo's `make_resolver` in
/// `demo-ui/src/windows/rich_text_demo`.
pub fn single_font_resolver(font: Arc<Font>) -> SharedResolver {
    Rc::new(move |_: &InlineStyle| Arc::clone(&font))
}

/// Deprecated alias for [`single_font_resolver`], kept for source compatibility.
///
/// Prefer [`single_font_resolver`], whose name states the single-typeface intent
/// and whose docs spell out the full resolver contract.
pub fn uniform_resolver(font: Arc<Font>) -> SharedResolver {
    single_font_resolver(font)
}
