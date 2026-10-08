//! Paint-pipeline hooks of the [`Widget`](super::Widget) trait: backbuffers
//! and compositing layers, overlay and global-overlay painting, child clips
//! and child transforms.
//!
//! A trait's items cannot be spread over several files, so this module holds
//! them as a macro that `widget_trait.rs` expands inside `pub trait Widget`;
//! the methods are ordinary trait methods (overridable, documented, part of
//! the vtable). The names they use resolve at the expansion site, whose
//! imports cover them. Sibling: `layout_hooks.rs`.

/// The paint-pipeline methods of `Widget`; expanded once, inside the trait.
macro_rules! widget_paint_hooks {
    () => {
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

        /// Request that this widget subtree be painted into a transient
        /// transparent compositing layer before being blended into its parent.
        ///
        /// Renderers that do not implement real layers ignore this hook. The
        /// method is mutable so widgets can advance visibility tweens at the
        /// point where the traversal knows the layer will be painted.
        fn compositing_layer(&mut self) -> Option<CompositingLayer> {
            None
        }

        /// Unified widget-owned backbuffer request.
        fn backbuffer_spec(&mut self) -> BackbufferSpec {
            let mode = self.backbuffer_mode();
            BackbufferSpec::default_for(mode, self.backbuffer_cache_mut().is_some())
        }

        /// Mutable retained backbuffer state for widgets that request a
        /// [`BackbufferSpec`] other than [`BackbufferKind::None`].
        fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
            None
        }

        /// Mark this widget's own retained surface dirty, if it owns one.
        ///
        /// Invalidates *both* the retained-layer [`BackbufferState`] and the
        /// per-widget bitmap [`BackbufferCache`].  Inspector edits that mutate
        /// a widget's reflected props bypass setters that normally invalidate
        /// the cache (e.g. `Label::set_text`); calling `mark_dirty` after an
        /// edit restores correct re-raster on the next frame.
        fn mark_dirty(&mut self) {
            if let Some(state) = self.backbuffer_state_mut() {
                state.invalidate();
            }
            if let Some(cache) = self.backbuffer_cache_mut() {
                cache.invalidate();
            }
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

        /// Opt into over-scan band caching for a *scrolling* backbuffered widget.
        ///
        /// When this returns `Some`, `paint_subtree_backbuffered` rasters a band
        /// taller than the widget bounds (viewport + over-scan) and composites it at
        /// a blit offset, so scrolling within the band re-blits instead of
        /// re-rasterising. See [`BackbufferBand`] for the full contract (units,
        /// physical-pixel quantization, bounds clipping, invalidation).
        ///
        /// Only consulted when [`backbuffer_cache_mut`](Self::backbuffer_cache_mut)
        /// returns `Some`. Default `None` keeps the byte-identical bounds-sized
        /// raster + 1:1 blit path for every other widget.
        fn backbuffer_band(&self) -> Option<BackbufferBand> {
            None
        }

        /// Logical pixels of ink this widget paints `(below, above)` its bounds:
        /// text on a one-em line box overhangs the box by however far the face's
        /// ascent and descent reach past an em. A backbuffered widget's bitmap
        /// grows by these margins and is blitted without clipping to the widget's
        /// own bounds (its parent's clip still applies), so the overhang shows.
        ///
        /// Only consulted when [`backbuffer_cache_mut`](Self::backbuffer_cache_mut)
        /// returns `Some` and [`backbuffer_band`](Self::backbuffer_band) returns
        /// `None`. Default `None`: the bitmap is exactly the bounds.
        fn backbuffer_ink_outset(&self) -> Option<(f64, f64)> {
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

        /// Called after `paint`, child painting, and optional overlay painting.
        ///
        /// Most widgets do not need this. It exists for widgets that intentionally
        /// open a backend compositing scope in `paint` and must close it after all
        /// descendants have rendered into that scope.
        fn finish_paint(&mut self, _ctx: &mut dyn DrawCtx) {}

        /// Paint app-level overlays after the entire widget tree has been painted.
        ///
        /// The traversal preserves this widget's local transform but skips ancestor
        /// clips and retained parent redraw requirements. Use this for portal-style
        /// UI that draws outside normal bounds while still participating in the
        /// widget tree's Z order.
        fn paint_global_overlay(&mut self, _ctx: &mut dyn DrawCtx) {}

        /// Opt this widget's *entire subtree* out of the normal inline paint pass so
        /// it renders in [`paint_global_overlay`](Self::paint_global_overlay)
        /// instead — the same clip-escaping global pass used by menus and tooltips.
        ///
        /// When this returns `true` and the widget is visible, [`paint_subtree`]
        /// skips it during the ordinary tree walk (nothing, including children, is
        /// painted inline), and the widget is expected to paint itself in its
        /// `paint_global_overlay` via [`paint_subtree_forced`]. Because the global
        /// overlay walk applies only per-widget translations (never ancestor
        /// clips), the subtree then floats above and outside any ancestor's clip —
        /// which is exactly what a modal dialog nested inside a scrolled/clipped
        /// container needs so it can't be truncated by its host.
        ///
        /// Default `false`: ordinary widgets paint inline as usual.
        ///
        /// # Z-order caveat for deferred hosts
        ///
        /// The global-overlay walk is post-order (a parent's
        /// `paint_global_overlay` runs *after* its descendants'), so a deferred
        /// widget's body — painted in its own `paint_global_overlay` — draws on top
        /// of anything a descendant painted *directly* in an earlier
        /// `paint_global_overlay` (e.g. a `menu`/`PopupMenu` surface, the markdown
        /// link layer, or a nested modal). Descendants that instead submit to a
        /// drained queue (`ComboBox` popups, `Tooltip`) are unaffected — those
        /// queues drain after the whole overlay walk. Today the colour dialog's
        /// content uses only queue-based descendants, so it's safe; a future modal
        /// host embedding a direct-overlay child must account for this occlusion.
        fn defer_paint_to_overlay(&self) -> bool {
            false
        }

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

        /// Affine transform applied between this widget and its children during
        /// inspector traversal.  Mirrors what `paint()` does — e.g. a widget
        /// that pushes pan/zoom in `paint()` and pops it in `finish_paint()`
        /// makes the framework recurse into its children with pan/zoom active,
        /// so `collect_inspector_nodes` must apply the same transform when
        /// accumulating descendant screen bounds.  Without this hook the
        /// inspector hover overlay lands at the un-transformed canvas position
        /// when the widget sits inside a panning/zooming container.
        ///
        /// Default: identity (most widgets translate their children only
        /// through `child.bounds()`, which `collect_inspector_nodes` already
        /// accumulates separately).
        ///
        /// Defaults to [`child_transform`](Self::child_transform) so a widget that
        /// injects a real pan/zoom into pointer + paint (a [`Scene`]) is seen by the
        /// inspector at the same on-screen position without having to implement two
        /// hooks.  Widgets that need an inspector-only transform (rare) override
        /// this directly.
        fn inspector_child_transform(&self) -> crate::TransAffine {
            self.child_transform()
                .unwrap_or_else(crate::TransAffine::new)
        }

        /// Affine transform (child-local → this widget's local space) that the
        /// framework applies to **all** of this widget's children for painting,
        /// hit-testing, and event dispatch alike.
        ///
        /// This is the "scale hook" that lets a container magnify/pan its whole
        /// child subtree (a [`Scene`]) while keeping those children fully
        /// first-class: because they live in [`children`](Self::children), keyboard
        /// focus (Tab + click-to-focus), the inspector, and pointer input all reach
        /// them through the framework's normal traversals, which map coordinates
        /// through this transform when descending.
        ///
        /// The transform is applied to the child group as a whole; per-child
        /// `bounds()` offsets are interpreted **inside** the transform (i.e. in the
        /// child/scene space), so a container that pins its content at the origin
        /// (the common case) needs no further care.  Pointer traversals invert this
        /// transform to map an incoming screen-local point into child space.
        ///
        /// Default: `None` — children are positioned by `bounds()` alone.
        ///
        /// [`Scene`]: crate::widgets::Scene
        fn child_transform(&self) -> Option<crate::TransAffine> {
            None
        }
    };
}
