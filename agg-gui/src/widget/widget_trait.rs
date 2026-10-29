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
// Text selection
// ---------------------------------------------------------------------------

/// A text widget's selection as [`Widget::text_selection`] reports it: the
/// fixed end (`anchor`) and the caret, equal when nothing is selected.
///
/// The offsets number positions in the widget's committed text; an
/// input-method composition is not part of it (none is open while a keyboard
/// caret gesture runs, as the gesture ends a dictation first). They are only
/// compared with each other and handed back to the same widget's
/// [`Widget::set_text_selection`], so each widget numbers its own text:
/// - `TextField`, `TextArea`: byte offsets into the text;
/// - `RichTextEdit`: byte offsets into its plain text, the blocks joined
///   with one `\n` each;
/// - `RichMarkdownEditWidget`: character offsets over its text positions in
///   document order (each block, or each table cell row by row), one
///   between each; a whole-block selection reports `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    /// Where the selection started.
    pub anchor: usize,
    /// Where the caret is.
    pub caret: usize,
    /// At a soft line wrap, where one offset is both the end of a line and
    /// the start of the next, whether the caret shows at the end of the
    /// earlier line (its affinity). Only `RichMarkdownEditWidget` tracks
    /// it; the others report `false` and ignore it.
    pub caret_at_line_end: bool,
}

impl TextSelection {
    /// A selection from `anchor` to `caret`, the caret on the later line at
    /// a soft wrap.
    pub const fn new(anchor: usize, caret: usize) -> Self {
        Self {
            anchor,
            caret,
            caret_at_line_end: false,
        }
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
    ///
    /// The widget arranges its own children against `available`.  A parent
    /// that then gives it bounds of a different size (a fitted child that
    /// reported less than it was offered, a cross-axis anchor) calls
    /// `layout` again with that final size, so the children land inside the
    /// box the widget actually occupies (`FlexColumn`, `FlexRow` and
    /// `AbsoluteLayout` do).
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

    /// Whether, while this widget is the active modal, pointer moves outside
    /// its bounds hover the widgets under the pointer as usual: their hover
    /// state and cursor (a splitter's resize pointer) show, as beside any
    /// panel. Presses, releases, the wheel and keys still go to the modal, so
    /// a press outside still reaches it first (to close it). For a
    /// light-dismiss panel with no backdrop; a dialog over a backdrop keeps
    /// the default `false`, hiding what is beneath.
    fn modal_hover_passes_outside(&self) -> bool {
        false
    }

    /// A [`ScrollView`](crate::widgets::ScrollView) above this widget has
    /// scrolled. Delivered to the whole scrolled subtree, only while a
    /// caret suggestion list is open anywhere; a widget showing an unclipped
    /// overlay tied to its position (the `TextField` suggestion list) closes
    /// it here. Default: nothing.
    fn on_ancestor_scrolled(&mut self) {}

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

    /// Preview an event routed to one of this widget's descendants, before
    /// the descendant sees it.
    ///
    /// Routed dispatch ([`dispatch_event`](crate::widget::dispatch_event))
    /// calls this on every ancestor of the target, root first, with the
    /// event in the ancestor's local coordinates, and only then delivers
    /// the event to the target and bubbles it back up through `on_event`.
    /// It is agg-sharp's `MouseDown` / `MouseDownCaptured` split seen from
    /// the parent: a GuiWidget's `MouseDown` fires even when a child took
    /// the press, so a container (a node card, a window) can react to a
    /// press anywhere inside it — select or raise itself — while the slider
    /// or text field under the pointer still gets the press.
    ///
    /// Return `Ignored` (the default) to let the event continue. Returning
    /// a consuming result intercepts it: no deeper preview runs and no
    /// widget's `on_event` sees the event. A preview that only observes
    /// should return `Ignored` and call
    /// [`request_draw`](crate::animation::request_draw) if it changed
    /// paint state.
    fn preview_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

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

    /// Whether a pointer press that lands on this widget leaves keyboard
    /// focus where it is. Default: false.
    ///
    /// A press on a widget that cannot take focus normally clears it
    /// (clicking empty space defocuses a text field). A menu host — a menu
    /// bar, a button that opens a [`PopupMenu`](crate::widgets::PopupMenu) —
    /// returns true: opening the menu and choosing a row must leave the
    /// focused field focused, or Edit > Paste would have nowhere to paste
    /// (agg-sharp's `PopupMenu` hands focus back to whatever had it when it
    /// closes). A focusable widget takes focus on a press whatever this
    /// returns; only the widget the press lands on is asked.
    fn keeps_focus_on_press(&self) -> bool {
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

    /// Report the named things this widget draws that are not widgets of
    /// their own — drop-down rows, menu rows, 3D controls — whose name is
    /// `name`, by pushing one [`NamedTarget`](super::NamedTarget) per match
    /// onto `out` (C#'s `FindDescendants` overrides that add a
    /// `WidgetAndPosition` with a `NamedObject`). GUI automation looks them
    /// up by name next to the widgets themselves and clicks a target at the
    /// centre of its rectangle. Only targets that are on show right now
    /// should be reported. Default: none. Children are walked by the caller,
    /// so a widget reports only its own targets.
    fn find_named_targets(&self, _name: &str, _out: &mut Vec<super::NamedTarget>) {}

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

    /// Show or hide this widget from a generic caller (the inspector's
    /// `visible` row, C# `GuiWidget.Visible = …`).  Returns `false`, changing
    /// nothing, when the widget has no visibility of its own to set — the
    /// default; widgets whose [`is_visible`](Self::is_visible) reads a flag
    /// they own override this to set it.
    fn try_set_visible(&mut self, _visible: bool) -> bool {
        false
    }

    /// Enable or disable this widget from a generic caller (the inspector's
    /// `enabled` row, C# `GuiWidget.Enabled = …`).  Returns `false`, changing
    /// nothing, when the widget cannot be disabled — the default; widgets
    /// whose [`is_enabled`](Self::is_enabled) reads a flag override this.
    fn try_set_enabled(&mut self, _enabled: bool) -> bool {
        false
    }

    /// Replace this widget's inner padding from a generic caller (the
    /// inspector's padding edits, C# `GuiWidget.Padding = …`).  Padding
    /// belongs to each container, stored its own way, so the default returns
    /// `false` and changes nothing; containers whose
    /// [`padding`](Self::padding) reads a stored `Insets` override this.
    fn try_set_padding(&mut self, _padding: Insets) -> bool {
        false
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

    /// Where an input-method (IME) candidate window should appear: the
    /// caret's rectangle (or the composition's, while one is shown) in this
    /// widget's local Y-up space. Only asked of the focused widget that
    /// [`accepts_text_input`](Self::accepts_text_input). `None` (default)
    /// places the window under the whole widget.
    fn ime_cursor_area(&self) -> Option<Rect> {
        None
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

    /// The committed text before the caret (before the selection, and
    /// before any input-method composition, which is not part of the text),
    /// if this widget is text-bearing: what the on-screen keyboard's
    /// dictation reads to decide whether the dictated words start a sentence
    /// and need a space before them (`on_screen_keyboard/voice_session.rs`).
    /// Default is `None`, and the keyboard then takes the whole
    /// [`text_input_value`](Self::text_input_value) (the caret at the end).
    /// `TextField`, `TextArea` and `RichTextEdit` override it.
    fn text_before_caret(&self) -> Option<String> {
        None
    }

    /// The selection, if this widget is text-bearing and can say. The
    /// on-screen keyboard's caret gestures (`on_screen_keyboard/cursor_slide.rs`)
    /// read it to tell whether a step moved the caret, and put it back with
    /// [`set_text_selection`](Self::set_text_selection) when a gesture is
    /// taken back or cancelled. See [`TextSelection`] for what the offsets
    /// count. Default is `None`: every step then counts as a move, and the
    /// gestures fall back to giving words back with the inverse keys.
    /// `TextField`, `TextArea`, `RichTextEdit` and `RichMarkdownEditWidget`
    /// override it.
    fn text_selection(&self) -> Option<TextSelection> {
        None
    }

    /// Put the selection back to `selection`, as
    /// [`text_selection`](Self::text_selection) reported it (each end clamped
    /// to the text). Returns whether it did; the default can't, and a widget
    /// that can't must return `false` so the gestures fall back.
    ///
    /// Putting back the selection the widget already has must change
    /// nothing (no undo-run break, no dropped pending style, no wrap side
    /// lost): the keyboard does exactly that to learn whether a widget can
    /// put its selection back, as a gesture starts.
    ///
    /// Implementing this pair is what makes a Backspace slide's cancel (a
    /// second finger, a cancelled touch) lossless. Without it the gestures
    /// give words back with the inverse key, Shift + word + Right, which is
    /// no exact inverse: under Ctrl-style bindings it also skips the spaces
    /// after a word, so a space can be left selected for the next key to
    /// replace.
    fn set_text_selection(&mut self, _selection: TextSelection) -> bool {
        false
    }

    /// Whether the text holds more than one line, so Up and Down move the
    /// caret between lines: the iPhone keyboard's trackpad sends its line
    /// steps only to such a widget (a one-line field may take Up and Down
    /// for something else, a number's step say). Default `false`: a
    /// multi-line text widget must opt in. `TextArea`, `RichTextEdit` and
    /// `RichMarkdownEditWidget` do.
    fn is_multi_line_text(&self) -> bool {
        false
    }

    /// Take over a dictation from the field this widget replaced: the app
    /// rebuilt the focused field in place, with the same text, and the
    /// on-screen keyboard's dictation follows it
    /// (`widget/app/voice.rs`). Put the caret at byte `caret` of the
    /// committed text and show `composition` there as input-method
    /// composition text (empty: none). With `composition_in_text`, the
    /// widget's text already holds `composition` at `caret` (a text area
    /// sharing its edit state with the one it replaced, which kept its
    /// composition in the buffer): adopt it as the composition rather than
    /// insert it again. Returns whether it did; the default can't, and the
    /// dictation then ends instead of following.
    fn resume_text_input(
        &mut self,
        _caret: usize,
        _composition: &str,
        _composition_in_text: bool,
    ) -> bool {
        false
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

    /// What this widget's Enter does (the on-screen keyboard's Enter icon)
    /// and whether its sentences start with a capital. Default is
    /// [`TextInputHints::MULTI_LINE`](crate::widgets::on_screen_keyboard::TextInputHints::MULTI_LINE);
    /// `TextField` returns its own (one-line: Done, lower case).
    fn text_input_hints(&self) -> crate::widgets::on_screen_keyboard::TextInputHints {
        crate::widgets::on_screen_keyboard::TextInputHints::MULTI_LINE
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

    /// The widget's reflected values, for the inspector: `None` (the
    /// default) for a widget that has none, `Some(&self.props)` for one that
    /// opts in. The inspector lists each field of the returned struct after
    /// the widget's [`properties`](Self::properties) rows, with no
    /// per-widget code, and edits them live through
    /// [`as_reflect_mut`](Self::as_reflect_mut) (`tree_inspector/edit.rs`).
    /// Only with the `reflect` feature (the crate header's "Reflection").
    ///
    /// # The companion-props pattern
    ///
    /// A widget opts in by holding its inspectable values in a companion
    /// `pub props: FooProps` struct that derives `Reflect`, and returning that
    /// struct here, never the widget itself. Deriving `Reflect` on a widget
    /// does not work: `bevy_reflect::Reflect` requires `Send + Sync`, and
    /// widgets hold `Rc<Cell<…>>` state and `Box<dyn FnMut(…)>` callbacks that
    /// are neither. Their sub-widgets (`Box<dyn Widget>`, which cannot be
    /// reflected at all) and `Arc<Font>` would force `Reflect` onto every type
    /// they reference, and skipping those fields with `#[reflect(ignore)]`
    /// needs a `Default` that a widget such as `Label` (which needs a font)
    /// has no sensible value for. The companion holds only plain values
    /// (`f64`, `bool`, `Option<usize>`, [`Color`](crate::Color), …), and the
    /// widget reads and writes them through `self.props`, so an edit the
    /// inspector makes shows on the next paint. Where a shared cell is the
    /// authoritative value (`Checkbox::with_state_cell`), the cell wins, and
    /// an inspector edit of the companion shows only on an unbound widget.
    /// `SliderProps`, `CheckboxProps`, `ProgressBarProps`,
    /// `ToggleSwitchProps` and `ContainerProps` follow it.
    ///
    /// # Converting a widget `Foo`
    ///
    /// 1. Pick the values a user might tune at run time: numbers, bools,
    ///    colours, enums, strings, small value structs. Leave out callbacks,
    ///    sub-widgets, shared `Rc<RefCell<…>>` cells, fonts and internal
    ///    state (drag tracking, animation, hover and focus flags).
    /// 2. Define the companion:
    ///    ```ignore
    ///    #[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
    ///    #[derive(Clone, Debug, Default)]
    ///    pub struct FooProps {
    ///        pub field_a: f64,
    ///        pub field_b: bool,
    ///    }
    ///    ```
    /// 3. Replace those fields on `Foo` with `pub props: FooProps`, and route
    ///    every read and write through it (`self.field_a` becomes
    ///    `self.props.field_a`), the constructor and builders included.
    /// 4. Keep every setter's cache invalidation: reflection writes the
    ///    field directly and then calls [`mark_dirty`](Self::mark_dirty),
    ///    but code that calls the setter still relies on it.
    /// 5. Override this method and its `_mut` counterpart, each under
    ///    `#[cfg(feature = "reflect")]`, to return `Some(&self.props)` and
    ///    `Some(&mut self.props)`.
    /// 6. Add a test beside the edit-pipeline tests in
    ///    `src/tests/reflect_roundtrip.rs` that applies an
    ///    `InspectorEdit` to one of the new fields and reads it back through
    ///    the widget's own getter.
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
