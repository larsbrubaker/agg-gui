//! Widget trait, tree traversal, and the top-level [`App`] struct.
//!
//! # Coordinate system
//!
//! Widget bounds are expressed in **parent-local** first-quadrant (Y-up)
//! coordinates. A widget at `bounds.x = 10, bounds.y = 20` is drawn 10 units
//! right and 20 units up from its parent's bottom-left corner.
//!
//! OS/browser mouse events arrive in Y-down screen coordinates. The single
//! conversion `y_up = viewport_height - y_down` happens inside
//! [`App::on_mouse_move`] / [`App::on_mouse_down`] / [`App::on_mouse_up`].
//! All widget code sees Y-up coordinates only.
//!
//! # Tree traversal
//!
//! Paint: root → leaves (children painted on top of parents).
//! Hit test: root → leaves (deepest child under cursor wins).
//! Event dispatch: leaf → root (events bubble up; any widget can consume).

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor};
use crate::lcd_coverage::LcdBuffer;

// ---------------------------------------------------------------------------
// Widget backbuffer — CPU bitmap cache per widget, invalidated via a dirty flag.
// ---------------------------------------------------------------------------
//
// Any widget can opt into a cached CPU backbuffer by returning `Some(&mut ...)`
// from [`Widget::backbuffer_cache_mut`].  The framework's `paint_subtree`
// handles caching transparently: when the widget is dirty (or has no bitmap
// yet) it allocates a fresh `Framebuffer`, runs `widget.paint` + all children
// into it via a software `GfxCtx`, and caches the resulting RGBA8 pixels as a
// shared `Arc<Vec<u8>>`.  Every subsequent frame that finds the widget clean
// just blits the cached pixels through `ctx.draw_image_rgba_arc` — zero AGG
// cost in steady state.  On the GL backend the `Arc`'s pointer identity keys
// the GPU texture cache (see `arc_texture_cache`), so the hardware texture
// is also reused across frames and dropped when the bitmap drops.
//
// The pattern is the one MatterCAD / AggSharp use: every widget CAN be
// backbuffered, each owns its bitmap, and a single `dirty` flag drives
// re-rasterisation.
//
// LCD subpixel rendering works naturally inside a backbuffer: the widget
// paints its own background first (so text has a solid dst) and then any
// `fill_text` call composites the per-channel coverage mask onto that
// destination.  No walk / sample / bg-declaration needed.

/// How a widget's backbuffer stores pixels.
///
/// The choice controls what the framework allocates as the render
/// target during `paint_subtree_backbuffered` and how the cached
/// bitmap is composited back onto the parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackbufferMode {
    /// 8-bit straight-alpha RGBA.  Standard Porter-Duff `SRC_ALPHA,
    /// ONE_MINUS_SRC_ALPHA` composite on blit.  Works for any widget,
    /// including ones with transparent areas.  Text inside is grayscale
    /// AA (no LCD subpixel).
    Rgba,
    /// 3 bytes-per-pixel **composited opaque RGB** — no alpha channel.
    /// Every fill (rects, strokes, text, etc.) inside the buffer goes
    /// through the 3× horizontal supersample + 5-tap filter + per-channel
    /// src-over pipeline described in `lcd-subpixel-compositing.md`.
    /// The buffer is blitted as an opaque RGB texture.
    ///
    /// **Contract:** the widget is responsible for painting content
    /// that covers its full bounds with opaque fills (starting with a
    /// bg rect).  Uncovered pixels land as black on the parent because
    /// there is no alpha channel to carry "no paint here."
    LcdCoverage,
}

/// A CPU bitmap owned by a widget that opts into backbuffer caching.
///
/// Set `dirty = true` from the widget's setter methods whenever the widget's
/// visual output could change (text, colour, bounds, hover/press state, …).
/// The framework re-rasterises on the next paint and clears the flag.
pub struct BackbufferCache {
    /// In **Rgba** mode: top-row-first RGBA8 pixels, straight alpha.
    /// Blitted via [`DrawCtx::draw_image_rgba_arc`].
    ///
    /// In **LcdCoverage** mode: top-row-first **colour plane** — 3
    /// bytes/pixel (R_premult, G_premult, B_premult) matching the
    /// convention of [`crate::lcd_coverage::LcdBuffer::color_plane`]
    /// flipped to top-down.  The companion alpha plane lives in
    /// [`Self::lcd_alpha`].
    pub pixels: Option<Arc<Vec<u8>>>,
    /// `LcdCoverage`-mode companion to `pixels`: top-row-first per-channel
    /// **alpha plane** (3 bytes/pixel, `(R_alpha, G_alpha, B_alpha)`).
    /// `None` means this is a plain Rgba cache.  When `Some`, the blit
    /// step uses [`DrawCtx::draw_lcd_backbuffer_arc`] to preserve the
    /// per-channel subpixel information through to the destination —
    /// required for LCD chroma to survive the cache round-trip.
    pub lcd_alpha: Option<Arc<Vec<u8>>>,
    pub width: u32,
    pub height: u32,
    /// When true, the next paint will re-rasterise rather than reusing
    /// `pixels`.  Widgets set this from their mutation paths
    /// (`set_text`, `set_color`, focus/hover changes, etc.) and the
    /// framework clears it after a successful re-raster.
    pub dirty: bool,
    /// Visuals epoch (see [`crate::theme::current_visuals_epoch`]) recorded
    /// the last time this cache was populated.  `paint_subtree_backbuffered`
    /// compares it against the live epoch and forces a re-raster on mismatch,
    /// so widgets whose text/fill colours come from `ctx.visuals()` refresh
    /// automatically on a dark/light theme flip without needing every widget
    /// to subscribe to theme-change events.
    pub theme_epoch: u64,
    /// Typography epoch (see
    /// [`crate::font_settings::current_typography_epoch`]) — same
    /// pattern as `theme_epoch` but for font / size scale / LCD /
    /// hinting / gamma / width / interval / faux-* globals.  Lets a
    /// slider drag in the LCD Subpixel demo invalidate every cached
    /// `Label` bitmap without bespoke hooks per widget.
    pub typography_epoch: u64,
}

impl BackbufferCache {
    pub fn new() -> Self {
        Self {
            pixels: None,
            lcd_alpha: None,
            width: 0,
            height: 0,
            dirty: true,
            theme_epoch: 0,
            typography_epoch: 0,
        }
    }

    /// Mark the cache dirty so the next paint re-rasterises.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
}

impl Default for BackbufferCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Widget trait
// ---------------------------------------------------------------------------

/// Every visible element in the UI is a widget.
///
/// Implementors handle their own painting and event handling. The framework
/// takes care of tree traversal, coordinate translation, and focus management.
pub trait Widget {
    /// Bounding rectangle in **parent-local** Y-up coordinates.
    fn bounds(&self) -> Rect;

    /// Set the bounding rectangle. Called by the parent during layout.
    fn set_bounds(&mut self, bounds: Rect);

    /// Immutable access to child widgets.
    fn children(&self) -> &[Box<dyn Widget>];

    /// Mutable access to child widgets (required for event dispatch + layout).
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>>;

    /// Compute desired size given available space, and update internal layout.
    ///
    /// The parent passes the space it can offer; the widget returns the size it
    /// actually wants to occupy. The parent uses the returned size to set this
    /// widget's bounds before calling `layout` on the next sibling.
    fn layout(&mut self, available: Size) -> Size;

    /// Paint this widget's own content into `ctx`.
    ///
    /// The framework has already translated `ctx` so that `(0, 0)` is this
    /// widget's bottom-left corner. **Do not paint children here** — the
    /// framework recurses into them automatically after `paint` returns.
    ///
    /// `ctx` is a `&mut dyn DrawCtx`; the concrete type is either a software
    /// `GfxCtx` (back-buffer path) or a `GlGfxCtx` (hardware GL path).
    fn paint(&mut self, ctx: &mut dyn DrawCtx);

    /// Return `true` if `local_pos` (in this widget's local coordinates) falls
    /// inside this widget's interactive area. Default: axis-aligned rect test.
    fn hit_test(&self, local_pos: Point) -> bool {
        let b = self.bounds();
        local_pos.x >= 0.0
            && local_pos.x <= b.width
            && local_pos.y >= 0.0
            && local_pos.y <= b.height
    }

    /// When `true`, `hit_test_subtree` stops recursing into this widget's
    /// children and returns this widget as the hit target.  Used for floating
    /// overlays (e.g. a scrollbar painted above its content) that must claim
    /// the pointer before children that happen to share the same pixels.
    /// Default: `false`.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        false
    }

    /// Whether this widget currently owns an app-modal interaction layer.
    ///
    /// When true anywhere in the tree, [`App`](crate::App) routes pointer and
    /// key events to that modal subtree before normal hit testing so content
    /// underneath the modal backdrop cannot be interacted with.
    fn has_active_modal(&self) -> bool {
        false
    }

    /// Handle an event. The event's positions are already in **local** Y-up
    /// coordinates. Return [`EventResult::Consumed`] to stop bubbling.
    fn on_event(&mut self, event: &Event) -> EventResult;

    /// Whether this widget can receive keyboard focus. Default: false.
    fn is_focusable(&self) -> bool {
        false
    }

    /// A static name for this widget type, used by the inspector. Default: "Widget".
    fn type_name(&self) -> &'static str {
        "Widget"
    }

    /// Optional human-readable identifier for this widget instance.
    ///
    /// Distinct from [`type_name`] (which is per-type and constant):
    /// `id` lets external code look up a specific *instance* — used
    /// today by the demo's z-order persistence to match a saved title
    /// against a live `Window` in the canvas `Stack`.  Default
    /// implementation returns `None`; widgets that want to be
    /// identifiable (e.g. `Window` returning its title) override.
    fn id(&self) -> Option<&str> {
        None
    }

    /// Return `false` to suppress painting this widget **and all its children**.
    /// The widget's own `paint()` will not be called.  Default: `true`.
    fn is_visible(&self) -> bool {
        true
    }

    /// Return type-specific properties for the inspector properties pane.
    ///
    /// Each entry is `(name, display_value)`.  The default returns an empty
    /// list; widgets override this to expose their state to the inspector.
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![]
    }

    /// Whether this widget renders into its own offscreen buffer before
    /// compositing into the parent.
    ///
    /// When `true`, `paint_subtree` wraps the widget (and all its descendants)
    /// in `ctx.push_layer` / `ctx.pop_layer`.  The widget and its children draw
    /// into a fresh transparent framebuffer; when complete, the buffer is
    /// SrcOver-composited back into the parent render target.  This enables
    /// per-widget alpha compositing, caching, and isolation.
    ///
    /// Default: `false` (pass-through rendering).
    fn has_backbuffer(&self) -> bool {
        false
    }

    /// Opt into per-widget CPU bitmap caching with a dirty flag.
    ///
    /// Widgets that return `Some(&mut cache)` get their paint +
    /// children cached as a `Vec<u8>` of RGBA8 pixels.  `paint_subtree`
    /// re-rasterises via AGG only when `cache.dirty` is true; otherwise
    /// it blits the existing bitmap.  GL backends key their texture
    /// cache on the `Arc`'s pointer identity so the uploaded GPU
    /// texture is also reused across frames.
    ///
    /// The widget is responsible for calling `cache.invalidate()` (or
    /// setting `cache.dirty = true`) from any mutation that could
    /// change the rendered output — text/color setters, focus/hover
    /// state changes, layout size changes, etc.  The framework clears
    /// the flag after a successful re-raster.
    ///
    /// Default: `None` (no caching — paint every frame directly).
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        None
    }

    /// Storage format for this widget's backbuffer.  Ignored unless
    /// [`backbuffer_cache_mut`] returns `Some`.  Default
    /// [`BackbufferMode::Rgba`] — correct for any widget.
    /// Opt into [`BackbufferMode::LcdCoverage`] only when the widget
    /// paints opaque content covering its full bounds.
    fn backbuffer_mode(&self) -> BackbufferMode {
        BackbufferMode::Rgba
    }

    /// Whether the inspector should recurse into this widget's children.
    ///
    /// Returns `false` for widgets that are part of the inspector infrastructure
    /// (e.g. the inspector's own `TreeView`) to prevent the inspector from
    /// showing itself recursively, which would grow the node list every frame.
    ///
    /// The widget itself is still included in the inspector snapshot — only
    /// its subtree is suppressed.
    fn contributes_children_to_inspector(&self) -> bool {
        true
    }

    /// Return `false` to hide this widget (and its subtree) from the inspector
    /// node snapshot entirely.  Intended for zero-size utility widgets such
    /// as layout-time watchers / tickers / invisible composers — they bloat
    /// the inspector tree without providing user-relevant information and,
    /// at scale, can make the inspector's per-frame tree rebuild expensive.
    fn show_in_inspector(&self) -> bool {
        true
    }

    /// Per-widget LCD subpixel preference for backbuffered text rendering.
    ///
    /// - `Some(true)`  — always raster text with LCD subpixel.
    /// - `Some(false)` — always use grayscale AA.
    /// - `None`        — defer to the global `font_settings::lcd_enabled()`.
    ///
    /// Only widgets that raster text into an offscreen backbuffer act on
    /// this flag (today: `Label`).  Defaulting to `None` means every such
    /// widget follows the global toggle unless the instance explicitly
    /// opts in or out.
    fn lcd_preference(&self) -> Option<bool> {
        None
    }

    /// Paint decorations that must appear **on top of all children**.
    ///
    /// Called by [`paint_subtree`] after all children have been painted.
    /// The default implementation is a no-op; override in widgets that need
    /// to draw overlays (e.g. resize handles, drag previews) that must not
    /// be occluded by child content.
    fn paint_overlay(&mut self, _ctx: &mut dyn DrawCtx) {}

    /// Paint app-level overlays after the entire widget tree has been painted.
    ///
    /// This is for overlays whose visual stacking is global, not parent-local
    /// (e.g. modal backdrops). Normal widgets should use [`paint_overlay`].
    fn paint_global_overlay(&mut self, _ctx: &mut dyn DrawCtx) {}

    /// Return a clip rectangle (in local coordinates) that constrains all child
    /// painting.  `paint_subtree` applies this clip before recursing into
    /// children, then restores the previous clip state afterward.  The clip does
    /// **not** affect `paint_overlay`, which runs after the clip is removed.
    ///
    /// The default clips children to this widget's own bounds, preventing
    /// overflow.  Override to return a narrower rect (e.g. Window clips to the
    /// content area below the title bar, or an empty rect when collapsed).
    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let b = self.bounds();
        Some((0.0, 0.0, b.width, b.height))
    }

    // -------------------------------------------------------------------------
    // Layout properties (universal — every widget carries these)
    // -------------------------------------------------------------------------

    /// Outer margin around this widget in logical units.
    ///
    /// The parent layout reads this to compute spacing and position.
    /// Default: [`Insets::ZERO`].
    fn margin(&self) -> Insets {
        Insets::ZERO
    }

    /// Horizontal anchor: how this widget sizes/positions itself horizontally
    /// within the slot the parent assigns.
    /// Default: [`HAnchor::FIT`] (take natural content width).
    fn h_anchor(&self) -> HAnchor {
        HAnchor::FIT
    }

    /// Vertical anchor: how this widget sizes/positions itself vertically
    /// within the slot the parent assigns.
    /// Default: [`VAnchor::FIT`] (take natural content height).
    fn v_anchor(&self) -> VAnchor {
        VAnchor::FIT
    }

    /// Minimum size constraint (logical units).
    ///
    /// The parent will never assign a slot smaller than this.
    /// Default: [`Size::ZERO`] (no minimum).
    fn min_size(&self) -> Size {
        Size::ZERO
    }

    /// Maximum size constraint (logical units).
    ///
    /// The parent will never assign a slot larger than this.
    /// Default: [`Size::MAX`] (no maximum).
    fn max_size(&self) -> Size {
        Size::MAX
    }

    /// Whether [`paint_subtree`] should snap this widget's incoming
    /// translation to the physical pixel grid.
    ///
    /// Defaults to the process-wide
    /// [`pixel_bounds::default_enforce_integer_bounds`](crate::pixel_bounds::default_enforce_integer_bounds)
    /// flag so the common case — crisp UI text + strokes — works without
    /// ceremony.  Widgets with a [`WidgetBase`] should delegate to
    /// `self.base().enforce_integer_bounds` so per-instance overrides take
    /// effect; widgets that genuinely want sub-pixel positioning (smooth
    /// scroll markers, zoomed canvases) override to return `false`.
    ///
    /// Mirrors MatterCAD's `GuiWidget.EnforceIntegerBounds` accessor.
    fn enforce_integer_bounds(&self) -> bool {
        crate::pixel_bounds::default_enforce_integer_bounds()
    }

    /// Report the minimum height this widget needs to fully render
    /// its content when given the supplied `available_w` for width.
    ///
    /// Used by parents whose layout strategy depends on a true
    /// content-required height that's independent of the slot they
    /// might hand the widget — most importantly by
    /// `Window::with_tight_content_fit(true)` to enforce "no
    /// clipping, no whitespace" on the height axis even when the
    /// content tree contains a flex-fill widget that would
    /// otherwise return `available.height` from `layout`.
    ///
    /// Default returns `min_size().height` — accurate for widgets
    /// whose minimum doesn't depend on width.  Width-sensitive
    /// widgets (wrapped text containers like `TextArea`, recursive
    /// containers like `FlexColumn`) override and compute properly.
    fn measure_min_height(&self, _available_w: f64) -> f64 {
        self.min_size().height
    }

    /// Container widgets (notably [`crate::widgets::Stack`]) call this on each
    /// child at the start of `layout()`.  A widget that returns `true` is
    /// moved to the END of its parent's child list — painted last, i.e.
    /// raised to the top of the z-order.  `take_` semantics: the call is
    /// also expected to **clear** the request so the child doesn't keep
    /// getting raised every frame.
    ///
    /// Default: no raise ever requested.  `Window` overrides to fire on the
    /// false→true visibility transition (see its `with_visible_cell`), so
    /// toggling a demo checkbox on in the sidebar automatically pops that
    /// window to the front.
    fn take_raise_request(&mut self) -> bool {
        false
    }

    // -------------------------------------------------------------------------
    // Visibility-gated repaint propagation
    // -------------------------------------------------------------------------
    //
    // The host render loop walks the widget tree from the root to decide
    // whether a new frame is needed.  A widget with in-flight animation,
    // pending hover transition, or scheduled cursor-blink reports `true` via
    // [`needs_paint`] or a deadline via [`next_paint_deadline`].  Parents
    // aggregate these over their **visible** children — invisible subtrees
    // (collapsed Window, non-selected TabView tab, off-viewport content)
    // must NOT contribute, so an animation inside a hidden part of the UI
    // cannot cause the screen to redraw.  This is the tree-walk equivalent
    // of a global "dirty" flag; going through the tree lets the framework
    // honour the visibility contract without trusting every widget author
    // to check it manually.

    /// Return `true` if this widget, or any visible descendant, has state
    /// that requires a repaint (hover change, tween in flight, etc.).
    ///
    /// The default walks visible children.  Widgets with their own pending
    /// state OR that state with the default walk — see `WidgetBase` helpers.
    fn needs_paint(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.children().iter().any(|c| c.needs_paint())
    }

    /// Return the earliest wall-clock instant at which this widget (or any
    /// visible descendant) wants the next paint.  `None` = no scheduled wake.
    /// The host loop turns a `Some(t)` into `ControlFlow::WaitUntil(t)` so
    /// e.g. a cursor blink fires without continuous polling.
    ///
    /// Same visibility contract as [`needs_paint`]: hidden subtrees return
    /// `None` regardless of what the widget *would* ask for if shown.
    fn next_paint_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() {
            return None;
        }
        let mut best: Option<web_time::Instant> = None;
        for c in self.children() {
            if let Some(t) = c.next_paint_deadline() {
                best = Some(match best {
                    Some(b) if b <= t => b,
                    _ => t,
                });
            }
        }
        best
    }
}

mod app;
mod paint;
mod tree;

pub use app::App;
pub use paint::{paint_global_overlays, paint_subtree};
pub use tree::{
    active_modal_path, collect_inspector_nodes, current_mouse_world, current_viewport,
    dispatch_event, find_widget_by_id, find_widget_by_id_mut, find_widget_by_type,
    hit_test_subtree, set_current_mouse_world, set_current_viewport, InspectorNode,
};
