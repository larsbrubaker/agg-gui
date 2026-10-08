//! The [`Widget`] trait: the per-element contract every node in the widget
//! tree implements (bounds, children, layout, paint, events, plus the
//! optional hooks for focus, inspector, backbuffers, tooltips, names and
//! typed downcasts).
//!
//! Lives in its own file so `widget.rs` stays the module root (traversal
//! re-exports and submodules); `widget.rs` re-exports it as
//! `crate::widget::Widget`. The paint-pipeline hooks (`widget_trait/paint_hooks.rs`)
//! and the layout-property hooks (`widget_trait/layout_hooks.rs`) are written
//! in their own files as macros expanded inside the trait, since a trait's
//! items cannot be split across files any other way.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};

#[macro_use]
mod paint_hooks;
#[macro_use]
mod layout_hooks;

use super::{
    BackbufferBand, BackbufferCache, BackbufferMode, BackbufferSpec, BackbufferState,
    CompositingLayer,
};

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

    /// When `true`, this widget disables *all* interaction — pointer and
    /// keyboard — within its subtree, reproducing egui's
    /// `UiBuilder::disabled()`.  Unlike [`claims_pointer_exclusively`], this is
    /// a position-independent state predicate: pointer hit-testing stops at
    /// this widget (so clicks are swallowed rather than passing through), and
    /// focus collection skips the whole subtree (so Tab cannot reach a child).
    /// Default: `false`.
    fn blocks_child_interaction(&self) -> bool {
        false
    }

    /// Return true when `local_pos` hits an app-level overlay owned by this
    /// widget. Unlike normal hit testing, ancestors may be missed because the
    /// overlay is painted outside their bounds.
    fn hit_test_global_overlay(&self, _local_pos: Point) -> bool {
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
    ///
    /// # Invalidation contract
    ///
    /// If your handler mutates state that affects the next paint (hover
    /// index, focus, button-pressed bool, animation phase, ...), call
    /// [`crate::animation::request_draw`] from inside the handler.  That
    /// bumps the invalidation epoch, which `dispatch_event` reads to mark
    /// retained ancestor backbuffers dirty — without it, the cached
    /// bitmap composites unchanged and your state mutation is invisible
    /// until something else dirties the cache.
    ///
    /// Returning `Consumed` *also* dirties the ancestor path automatically,
    /// so a consumed click handler that calls `request_draw` is belt-and-
    /// suspenders.  But `MouseMove` handlers that return `Ignored` (the
    /// usual case for hover-tracking) **only** invalidate via
    /// `request_draw`'s epoch bump.  Forgetting it produces "hover only
    /// works the first time after I drag the window resize edge" bugs.
    ///
    /// `request_draw_without_invalidation` is for the rare cases where
    /// the visual change is in an app-overlay or a position-only
    /// composite — see its rustdoc.
    fn on_event(&mut self, event: &Event) -> EventResult;

    /// Handle a key that was not consumed by the focused widget path.
    ///
    /// This is used for window/menu accelerators: focused controls get first
    /// chance at the key, then visible widgets in paint order may claim it.
    fn on_unconsumed_key(&mut self, _key: &Key, _modifiers: Modifiers) -> EventResult {
        EventResult::Ignored
    }

    /// Whether this widget can receive keyboard focus. Default: false.
    fn is_focusable(&self) -> bool {
        false
    }

    /// Stable identifier for the programmatic focus channel
    /// ([`crate::focus::request_focus`]).
    ///
    /// App code can't reach the [`App`](crate::widget::App)'s private focus
    /// path to focus a widget the moment it appears (e.g. a search field
    /// that should grab the keyboard when its overlay opens). A widget that
    /// returns `Some(id)` here can be focused by calling
    /// [`crate::focus::request_focus(id)`](crate::focus::request_focus); the
    /// `App` services the request on its next `layout`, moving focus to the
    /// matching focusable widget (which dispatches `FocusGained` and raises
    /// the on-screen keyboard for text inputs). Default: `None`.
    fn focus_id(&self) -> Option<crate::focus::FocusId> {
        None
    }

    /// A static name for this widget type, used by the inspector. Default: "Widget".
    fn type_name(&self) -> &'static str {
        "Widget"
    }

    /// Optional human-readable identifier for this widget instance.
    ///
    /// Distinct from [`type_name`] (which is per-type and constant):
    /// `id` lets external code look up a specific *instance* — the demo's
    /// z-order persistence matches a saved title against a live `Window`,
    /// and GUI automation finds widgets by it (C# `GuiWidget.Name`).  The
    /// default returns the embedded [`WidgetBase::name`] (set with
    /// [`with_name`](Self::with_name)); widgets with an identity of their
    /// own (e.g. `Window` returning its title) override.
    fn id(&self) -> Option<&str> {
        self.widget_base().and_then(|b| b.name.as_deref())
    }

    /// Builder sugar: name this widget (C# `GuiWidget.Name`), returning
    /// `self` for chaining.  Writes [`WidgetBase::name`] through
    /// [`widget_base_mut`](Self::widget_base_mut), so the default
    /// [`id`](Self::id) reports it; a widget without a `WidgetBase` ignores
    /// the call — wrap it in [`Named`](crate::widgets::Named) instead.
    fn with_name(mut self, name: impl Into<String>) -> Self
    where
        Self: Sized,
    {
        self.set_name(Some(name.into()));
        self
    }

    /// Object-safe setter counterpart of [`with_name`](Self::with_name).
    /// Pass `None` to clear.
    fn set_name(&mut self, name: Option<String>) {
        if let Some(base) = self.widget_base_mut() {
            base.name = name;
        }
    }

    /// Builder sugar: request a position in the parent (C#
    /// `OriginRelativeParent`), read by
    /// [`AbsoluteLayout`](crate::widgets::AbsoluteLayout).  Writes
    /// [`WidgetBase::origin`]; a no-op without a `WidgetBase`.
    fn with_origin(mut self, x: f64, y: f64) -> Self
    where
        Self: Sized,
    {
        if let Some(base) = self.widget_base_mut() {
            base.origin = Point::new(x, y);
        }
        self
    }

    /// The concrete widget as `Any`, so code holding a `&dyn Widget` (GUI
    /// automation, tests) can read its typed state — C#'s `widget is
    /// TextEditWidget edit`.  Default `None`; the core widgets return
    /// `Some(self)`.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Mutable counterpart of [`as_any`](Self::as_any).
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    /// Return `false` to suppress painting this widget **and all its children**.
    /// The widget's own `paint()` will not be called.  Default: `true`.
    fn is_visible(&self) -> bool {
        true
    }

    /// Whether this widget takes input right now (C# `GuiWidget.Enabled`,
    /// the widget's own flag).  Default `true`; widgets that can be
    /// disabled (`Button`, `SegmentedControl`, `ChevronWidget`) report it.
    /// Ancestors are not consulted here — GUI automation walks the chain.
    fn is_enabled(&self) -> bool {
        true
    }

    /// Return type-specific properties for the inspector properties pane.
    ///
    /// Each entry is `(name, display_value)`.  The default returns an empty
    /// list; widgets override this to expose their state to the inspector.
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![]
    }

    /// `true` when this widget accepts free-form character input (typing
    /// arbitrary letters, numbers, punctuation). Used by the on-screen
    /// software keyboard (`crate::widgets::on_screen_keyboard`) to decide
    /// whether to slide up when this widget gains focus.
    ///
    /// Default is `false`. `TextField` and `TextArea` override to `true`.
    /// `DragValue` and similar numeric editors should override to `true`
    /// only when they are in their full-text-edit mode; otherwise the
    /// keyboard would appear for transient drag interactions.
    ///
    /// This is independent of [`is_focusable`](Self::is_focusable) — a
    /// `Button` is focusable but doesn't accept typed text.
    fn accepts_text_input(&self) -> bool {
        false
    }

    /// Current text contents of this widget if it is text-bearing.
    /// Used by the on-screen software keyboard to apply the
    /// sentence-start auto-capitalize heuristic: an empty field (or one
    /// ending in `.`, `!`, `?`, newline) opens the keyboard with Shift
    /// active.
    ///
    /// Default is `None`. `TextField` and `TextArea` override to return
    /// their current text. Callers that only need to know *whether* the
    /// widget accepts text input should use
    /// [`accepts_text_input`](Self::accepts_text_input).
    fn text_input_value(&self) -> Option<String> {
        None
    }

    /// Preferred keyboard input mode for this widget — used by the
    /// on-screen software keyboard to pick the initial layer when this
    /// widget gains focus.  Default is
    /// [`KeyboardInputMode::Text`](crate::widgets::on_screen_keyboard::KeyboardInputMode::Text);
    /// numeric fields override to
    /// [`Numeric`](crate::widgets::on_screen_keyboard::KeyboardInputMode::Numeric)
    /// so the digit pad slides up instead of the letter row.
    ///
    /// Only consulted when [`accepts_text_input`](Self::accepts_text_input)
    /// also returns `true`.
    fn text_input_mode(&self) -> crate::widgets::on_screen_keyboard::KeyboardInputMode {
        crate::widgets::on_screen_keyboard::KeyboardInputMode::Text
    }

    /// Try to lift this widget's visible content upward by `amount`
    /// pixels.  Used by the on-screen-keyboard auto-scroll so a
    /// focused text field doesn't end up hidden behind the keyboard
    /// panel: the App walks UP the focus path and asks each ancestor
    /// to absorb some of the deficit.
    ///
    /// Scrolling containers (notably
    /// [`ScrollView`](crate::widgets::ScrollView)) override this and
    /// increase their vertical scroll offset by up to `amount`,
    /// returning how much they actually applied (clamped to their
    /// remaining slack).  Negative `amount` reverses the operation
    /// (used to restore scroll when focus leaves a text-input).
    /// Default returns `0.0` — non-scrolling widgets contribute
    /// nothing.
    fn try_scroll_to_lift(&mut self, _amount: f64) -> f64 {
        0.0
    }

    /// Scroll the least amount that brings `rect` — in this widget's local
    /// Y-up coordinates — into view (C# `ScrollableWidget.ScrollIntoView`
    /// with `ScrollAmount.Minimum`): content clipped at the top is lowered
    /// until its top shows, content clipped at the bottom is raised until its
    /// bottom shows, and content already fully in view stays put. Returns
    /// `true` when this widget scrolls (it took the request, whether or not
    /// it moved); the default, for widgets that do not scroll, returns
    /// `false` so a caller walking up from a descendant moves on.
    fn scroll_rect_into_view(&mut self, _rect: Rect) -> bool {
        false
    }

    /// If this widget is text-bearing (e.g. `Label`), update its foreground
    /// colour.  Default is a no-op.  Composite widgets call this on their
    /// children to retint labels without rebuilding them — used by `Button`
    /// when toggling between active (white text on accent) and inactive
    /// (theme text on subtle bg) appearances.
    fn set_label_color(&mut self, _color: crate::color::Color) {}

    /// If this widget is text-bearing (e.g. `Label`), update its
    /// displayed text.  Default is a no-op.  Composite widgets that
    /// own a `Label` child use this to push live values (e.g. an FPS
    /// counter) into the child without bypassing the standard
    /// backbuffered glyph cache — calling this on a `Label` only
    /// invalidates the cache when the text actually changed.
    fn set_label_text(&mut self, _text: &str) {}

    /// Opt-in reflection accessor for the inspector's typed property editors.
    ///
    /// Widgets that derive [`bevy_reflect::Reflect`] (via the `reflect`
    /// cargo feature) override this to return `Some(self)` so the inspector
    /// can walk their fields with type information — boolean toggles,
    /// numeric sliders, color pickers, enum dropdowns — instead of falling
    /// back to the read-only string [`properties`](Self::properties) list.
    ///
    /// Default returns `None`; the inspector then uses the string list.
    /// Available only with the `reflect` feature so consumers without it
    /// don't pay the dependency cost.
    #[cfg(feature = "reflect")]
    fn as_reflect(&self) -> Option<&dyn bevy_reflect::Reflect> {
        None
    }

    /// Mutable counterpart of [`as_reflect`](Self::as_reflect).  Used by the
    /// inspector to write edits back into the live widget.
    #[cfg(feature = "reflect")]
    fn as_reflect_mut(&mut self) -> Option<&mut dyn bevy_reflect::Reflect> {
        None
    }

    widget_paint_hooks!();

    widget_layout_hooks!();

    // -------------------------------------------------------------------------
    // Visibility-gated scheduled draw propagation
    // -------------------------------------------------------------------------
    //
    // The host render loop walks the widget tree from the root to decide
    // whether a visible subtree has a scheduled draw need such as cursor blink.
    // Ordinary visual invalidation should call `animation::request_draw`, which
    // also advances the retained-layer invalidation epoch.  `needs_draw` stays
    // for visibility-gated future/ongoing draw needs: invisible subtrees
    // (collapsed Window, non-selected TabView tab, off-viewport content)
    // must NOT keep the app in a continuous draw loop.

    /// Return `true` if this widget, or any visible descendant, has an ongoing
    /// draw need that should keep the host drawing.
    ///
    /// The default walks visible children.  Widgets with their own pending
    /// state OR that state with the default walk — see `WidgetBase` helpers.
    fn needs_draw(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.children().iter().any(|c| c.needs_draw())
    }

    /// Return the earliest instant, on the UI clock ([`crate::clock`]), at
    /// which this widget (or any visible descendant) wants the next draw.
    /// `None` = no scheduled wake.
    /// The host loop turns a `Some(t)` into `ControlFlow::WaitUntil(t)` so
    /// e.g. a cursor blink fires without continuous polling.
    ///
    /// Same visibility contract as [`needs_draw`]: hidden subtrees return
    /// `None` regardless of what the widget *would* ask for if shown.
    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() {
            return None;
        }
        super::tree::earliest_child_draw_deadline(self.children())
    }
}
