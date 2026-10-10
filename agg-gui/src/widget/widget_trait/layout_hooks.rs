//! Layout-property and tooltip hooks of the [`Widget`](super::Widget)
//! trait: margin, padding, anchors, size constraints, the embedded
//! `WidgetBase`, tooltips, integer-bounds snapping, height measuring and
//! raise requests.
//!
//! A trait's items cannot be spread over several files, so this module holds
//! them as a macro that `widget_trait.rs` expands inside `pub trait Widget`;
//! the methods are ordinary trait methods (overridable, documented, part of
//! the vtable). The names they use resolve at the expansion site, whose
//! imports cover them. Sibling: `paint_hooks.rs`.

/// The layout-property methods of `Widget`; expanded once, inside the trait.
macro_rules! widget_layout_hooks {
    () => {
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

        /// Inner padding — space the widget reserves between its own bounds and
        /// its child layout area.  Only container widgets carry padding; leaf
        /// widgets default to [`Insets::ZERO`].
        ///
        /// The inspector reads this to draw a Chrome F12-style padding band on
        /// the highlighted widget.  Containers that already store padding
        /// internally (e.g. `FlexColumn::inner_padding`) override this to expose
        /// it to the inspector.
        fn padding(&self) -> Insets {
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

        /// How narrow a crowded row may squeeze this widget, or `None` when it
        /// can't be squeezed below the width it measured.
        ///
        /// A [`FlexRow`](crate::widgets::FlexRow) whose fixed children together
        /// are wider than the row takes the difference out of the children that
        /// answer `Some(min)` here (never below `min`), then lays them out at the
        /// narrower width — so siblings never overlap and nothing runs past the
        /// row. A [`Label`](crate::widgets::Label) with
        /// [`with_ellipsis_if_clipped`](crate::widgets::Label::with_ellipsis_if_clipped)
        /// answers `Some(min_size().width)`: it ends its line in "..." instead.
        /// Default: `None`.
        fn shrink_min_width(&self) -> Option<f64> {
            None
        }

        /// Direct read access to the widget's embedded [`WidgetBase`].
        ///
        /// Returns `Some` for every widget that embeds `WidgetBase` — effectively
        /// all concrete widgets.  The inspector uses this to read margin, anchors,
        /// and size constraints without going through the individual trait methods.
        /// Default returns `None`; widgets that embed `WidgetBase` override.
        fn widget_base(&self) -> Option<&WidgetBase> {
            None
        }

        /// Mutable counterpart of [`widget_base`](Self::widget_base).
        ///
        /// The inspector calls this to apply live edits to margin, h_anchor,
        /// v_anchor, min_size, and max_size from the properties pane.
        fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
            None
        }

        /// Hover-help text for the central tooltip controller, or `None`.
        ///
        /// This is the universal accessor the App's per-frame tooltip pass reads to
        /// find the deepest hovered tipped widget. The default reads the embedded
        /// [`WidgetBase::tooltip`], so **every widget that embeds a `WidgetBase`
        /// participates for free** — set the text with
        /// [`with_tooltip`](Self::with_tooltip) (chaining) or
        /// [`set_tooltip_text`](Self::set_tooltip_text) (on a `&mut dyn Widget`).
        /// A widget with no `WidgetBase`, or one that stores its tip elsewhere,
        /// overrides this directly.
        fn tooltip_text(&self) -> Option<&str> {
            self.widget_base().and_then(|b| b.tooltip.as_deref())
        }

        /// Which item this widget's tip currently describes, for widgets that
        /// show many items under one bounds (a treemap cell, a chart bar, a
        /// list row painted without child widgets). Default: `None` — the
        /// widget is one item.
        ///
        /// The central tooltip controller treats a change of key while the
        /// pointer stays over this widget exactly as entering a different
        /// widget: the visible tip hides and the hover delay re-arms — the
        /// quick reshow delay when a tip was recently visible, otherwise the
        /// full initial delay. Without a key, only the
        /// [`tooltip_text`](Self::tooltip_text) changes and a visible tip just
        /// updates in place. Read alongside `tooltip_text` once per frame.
        fn tooltip_key(&self) -> Option<u64> {
            None
        }

        /// Builder sugar: attach hover-help text, returning `self` for chaining.
        ///
        /// Available on every widget that embeds a [`WidgetBase`] with **zero
        /// per-widget code** — it writes through [`widget_base_mut`](Self::widget_base_mut).
        /// A widget without a `WidgetBase` silently ignores the call (there is
        /// nowhere to store the text); such widgets should expose their own tip API.
        /// Excluded from the trait object vtable via `where Self: Sized`, so the
        /// trait stays object-safe.
        fn with_tooltip(mut self, text: impl Into<String>) -> Self
        where
            Self: Sized,
        {
            if let Some(base) = self.widget_base_mut() {
                base.tooltip = Some(text.into());
            }
            self
        }

        /// Object-safe setter counterpart of [`with_tooltip`](Self::with_tooltip),
        /// callable on a `&mut dyn Widget` / `Box<dyn Widget>` (used by builders
        /// that assemble a control tree from boxed widgets, e.g. toolbars). Writes
        /// through [`widget_base_mut`](Self::widget_base_mut); a no-op for widgets
        /// without a `WidgetBase`. Pass `None` to clear.
        fn set_tooltip_text(&mut self, text: Option<String>) {
            if let Some(base) = self.widget_base_mut() {
                base.tooltip = text;
            }
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
    };
}
