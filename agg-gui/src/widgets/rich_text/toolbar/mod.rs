//! `RichTextToolbar` — a configurable, batteries-included formatting toolbar
//! driven by a [`RichEditHandle`].
//!
//! This is the library counterpart of the demo's toolbar
//! (`demo-ui/src/windows/rich_text_demo/toolbar.rs`): a self-contained widget an
//! embedding app can drop above a [`RichTextEdit`](super::RichTextEdit) to get
//! bold/italic/underline/strike, alignment, lists, indent/outdent, undo/redo, a
//! font-size dropdown, and text/highlight colours — all wired to the same
//! shared editor core the widget renders.  Every control group can be toggled
//! off through the builder; all are on by default.  Every control also carries a
//! hover tooltip out of the box (gate them with
//! [`with_tooltips`](RichTextToolbar::with_tooltips)).
//!
//! # Layout
//!
//! Controls flow across **two rows** (a [`FlexColumn`] of two
//! [`FlexRow`](crate::widgets::flex_row::FlexRow)s),
//! matching the demo: row 1 is inline character formatting + font family/size +
//! colours; row 2 is block formatting (alignment, lists, indent) + history.  An
//! empty row (its whole group disabled) is omitted.
//!
//! # Font family dropdown
//!
//! The library cannot depend on any font catalog, so the family dropdown is
//! **opt-in**: supply names (and optional per-item preview fonts) via
//! [`with_families`](RichTextToolbar::with_families).  With no families the
//! dropdown is omitted entirely.
//!
//! # Colours (self-contained)
//!
//! The text/highlight swatches open a floating
//! [`color_wheel_picker_dialog_with_on_close`](crate::widgets::color_wheel_picker::color_wheel_picker_dialog_with_on_close).
//! That dialog is a **modal** window, so it paints through the framework's
//! clip-free global-overlay pass and clamps itself into the viewport — meaning
//! the picker can live *inside* the (thin) toolbar widget and still float
//! un-truncated over the editor.  The toolbar is therefore **fully
//! self-contained**: no companion overlay to place, no top-level `Stack`
//! required.  Colours drive a live preview through the handle's preview session
//! ([`begin_preview`](RichEditHandle::begin_preview) /
//! [`commit_preview`](RichEditHandle::commit_preview) /
//! [`cancel_preview`](RichEditHandle::cancel_preview)) — the selection recolours
//! as the wheel is dragged, commits on *Select*, and restores on
//! *Cancel* / × / *Escape*.
//!
//! # Example
//!
//! Extends the module-level embedding example: just drop the toolbar above the
//! editor in a [`FlexColumn`] — the colour picker is handled internally.
//!
//! ```
//! use std::sync::Arc;
//! use agg_gui::Font;
//! use agg_gui::widgets::rich_text::{single_font_resolver, RichDoc, RichTextEdit};
//! use agg_gui::widgets::rich_text::toolbar::{RichTextToolbar, Variant};
//! use agg_gui::FlexColumn;
//!
//! let bytes = std::fs::read(concat!(
//!     env!("CARGO_MANIFEST_DIR"),
//!     "/assets/fonts/NotoSans-Regular.ttf",
//! )).expect("bundled font readable");
//! let font = Arc::new(Font::from_slice(&bytes).expect("valid font"));
//!
//! let editor = RichTextEdit::new(RichDoc::new(), single_font_resolver(Arc::clone(&font)));
//! let handle = editor.handle();
//!
//! // Configure a toolbar. Bold/Italic gate on a variant check; colours are on.
//! let toolbar = RichTextToolbar::new(handle, Arc::clone(&font))
//!     .with_families(vec!["Sans".to_string(), "Serif".to_string()], None)
//!     .with_variant_check(|_family, v| matches!(v, Variant::Italic));
//!
//! // The toolbar strip sits above the editor; the colour dialog floats itself.
//! let _body = FlexColumn::new()
//!     .add(Box::new(toolbar))
//!     .add_flex(Box::new(editor), 1.0);
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::platform::primary_modifier_label;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::flex::FlexColumn;
use crate::widgets::tooltip::Tooltip;

use super::commands::CommonStyle;
use super::editor::RichEditHandle;
use super::model::ListKind;

mod color;
mod controls;

use crate::widgets::text_area::TextHAlign;

/// Which colour the toolbar's floating picker is currently editing, shared
/// between the swatch buttons and the internal colour overlay.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerKind {
    None,
    TextColor,
    Highlight,
}

/// A synthetic face variant a family may or may not ship, passed to a
/// [`variant check`](RichTextToolbar::with_variant_check) that gates the
/// Bold / Italic toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Bold,
    Italic,
}

/// Injected predicate: does `family` ship the given [`Variant`]?  Used to grey
/// out the Bold / Italic toggles for families without a real bold/italic face.
pub(crate) type VariantCheck = Rc<dyn Fn(&str, Variant) -> bool>;

/// The injected font-family list (and optional per-item preview fonts) that
/// backs the family dropdown.
struct Families {
    names: Vec<String>,
    item_fonts: Option<Vec<Arc<Font>>>,
}

/// Which control groups are shown.  Every field defaults to `true`.
#[derive(Clone, Copy)]
struct ToolbarConfig {
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    alignment: bool,
    lists: bool,
    indent: bool,
    history: bool,
    font_size: bool,
    text_color: bool,
    highlight: bool,
    /// Attach a hover tooltip to every control. On by default.
    tooltips: bool,
}

impl Default for ToolbarConfig {
    fn default() -> Self {
        Self {
            bold: true,
            italic: true,
            underline: true,
            strikethrough: true,
            alignment: true,
            lists: true,
            indent: true,
            history: true,
            font_size: true,
            text_color: true,
            highlight: true,
            tooltips: true,
        }
    }
}

/// Default font-size steps (points) offered by the size dropdown, 8–32.
const DEFAULT_FONT_SIZES: &[f64] = &[
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0,
];

/// Generous slot used to lay out the colour-overlay's modal dialog so it can
/// size itself regardless of how thin the toolbar strip is. The dialog's final
/// on-screen position comes from the paint-time viewport clamp, not this size.
const OVERLAY_LAYOUT_ROOM: Size = Size::new(4096.0, 4096.0);

/// A configurable formatting toolbar bound to a [`RichEditHandle`].
///
/// Build it with [`new`](Self::new), tweak the control roster and font
/// family/size options through the `with_*` builders, then place it in a layout
/// like any other widget.  The colour picker is hosted internally (a modal
/// dialog that floats via the global-overlay pass), so the toolbar is fully
/// self-contained — nothing else to place (see the [module example](self)).
pub struct RichTextToolbar {
    bounds: Rect,
    base: WidgetBase,
    /// Exactly one built child (the two-row [`FlexColumn`]); rebuilt whenever a
    /// builder changes the config so `children()` / `measure_min_height` are
    /// always valid without a prior layout pass.
    root: Vec<Box<dyn Widget>>,

    handle: RichEditHandle,
    font: Arc<Font>,
    cfg: ToolbarConfig,
    families: Option<Families>,
    variant_check: Option<VariantCheck>,
    font_sizes: Vec<f64>,
    picker: Rc<Cell<PickerKind>>,
}

impl RichTextToolbar {
    /// Create a toolbar driving `handle`, labelling controls with `font` (the
    /// same base font whose Font Awesome fallback renders the icon glyphs).  All
    /// control groups are enabled; the font-family dropdown is omitted until you
    /// call [`with_families`](Self::with_families).
    pub fn new(handle: RichEditHandle, font: Arc<Font>) -> Self {
        let mut this = Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            root: Vec::new(),
            handle,
            font,
            cfg: ToolbarConfig::default(),
            families: None,
            variant_check: None,
            font_sizes: DEFAULT_FONT_SIZES.to_vec(),
            picker: Rc::new(Cell::new(PickerKind::None)),
        };
        this.rebuild();
        this
    }

    // ── Control-roster builders (all groups default on) ────────────────────

    /// Show/hide the Bold toggle.
    pub fn with_bold(mut self, on: bool) -> Self {
        self.cfg.bold = on;
        self.rebuild();
        self
    }
    /// Show/hide the Italic toggle.
    pub fn with_italic(mut self, on: bool) -> Self {
        self.cfg.italic = on;
        self.rebuild();
        self
    }
    /// Show/hide the Underline toggle.
    pub fn with_underline(mut self, on: bool) -> Self {
        self.cfg.underline = on;
        self.rebuild();
        self
    }
    /// Show/hide the Strikethrough toggle.
    pub fn with_strikethrough(mut self, on: bool) -> Self {
        self.cfg.strikethrough = on;
        self.rebuild();
        self
    }
    /// Show/hide the alignment trio (left / center / right).
    pub fn with_alignment(mut self, on: bool) -> Self {
        self.cfg.alignment = on;
        self.rebuild();
        self
    }
    /// Show/hide the ordered/bullet list toggles.
    pub fn with_lists(mut self, on: bool) -> Self {
        self.cfg.lists = on;
        self.rebuild();
        self
    }
    /// Show/hide the outdent/indent buttons.
    pub fn with_indent(mut self, on: bool) -> Self {
        self.cfg.indent = on;
        self.rebuild();
        self
    }
    /// Show/hide the undo/redo buttons.
    pub fn with_history(mut self, on: bool) -> Self {
        self.cfg.history = on;
        self.rebuild();
        self
    }
    /// Show/hide the font-size dropdown.
    pub fn with_font_size_combo(mut self, on: bool) -> Self {
        self.cfg.font_size = on;
        self.rebuild();
        self
    }
    /// Show/hide **both** colour swatches (text colour + highlight).
    pub fn with_colors(mut self, on: bool) -> Self {
        self.cfg.text_color = on;
        self.cfg.highlight = on;
        self.rebuild();
        self
    }

    /// Enable/disable the hover tooltips carried by **every** control (Bold,
    /// alignment, colours, undo/redo, …).  On by default — the toolbar ships
    /// self-documenting.  Pass `false` for a bare strip when the embedding app
    /// provides its own help affordance.
    pub fn with_tooltips(mut self, on: bool) -> Self {
        self.cfg.tooltips = on;
        self.rebuild();
        self
    }

    /// Replace the font-size steps (points) offered by the size dropdown.
    pub fn with_font_sizes(mut self, sizes: Vec<f64>) -> Self {
        self.font_sizes = sizes;
        self.rebuild();
        self
    }

    /// Enable the font-family dropdown over `names`.  `item_fonts`, when
    /// supplied (one [`Font`] per name), renders each dropdown row in its own
    /// face; pass `None` for a plain-text list.  Selecting a family issues a
    /// [`SetFontFamily`](super::commands::RichCommand::SetFontFamily) — the
    /// app's resolver maps family + bold/italic to a concrete face.
    ///
    /// `item_fonts` need not match `names` in length: a row with no matching
    /// entry falls back to the toolbar's base font, and any extra fonts beyond
    /// `names.len()` are ignored.
    pub fn with_families(mut self, names: Vec<String>, item_fonts: Option<Vec<Arc<Font>>>) -> Self {
        self.families = if names.is_empty() {
            None
        } else {
            Some(Families { names, item_fonts })
        };
        self.rebuild();
        self
    }

    /// Gate the Bold / Italic toggles on the selection's family actually
    /// shipping that [`Variant`] — a family with no bold face greys out the Bold
    /// toggle.  Without a check both stay enabled.  The check only fires for a
    /// consistent, explicit family; an inherited-default or mixed selection
    /// keeps the toggles enabled.
    pub fn with_variant_check(mut self, check: impl Fn(&str, Variant) -> bool + 'static) -> Self {
        self.variant_check = Some(Rc::new(check));
        self.rebuild();
        self
    }

    // ── Layout-props builders ──────────────────────────────────────────────

    /// Set the toolbar's outer margin.
    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    /// Set the horizontal anchor used when placed in a layout.
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    /// Set the vertical anchor used when placed in a layout.
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }

    /// Reconstruct the control tree from the current config.
    ///
    /// `root[0]` is the two-row [`FlexColumn`] that determines the toolbar's
    /// size.  When either colour swatch is enabled, `root[1]` is the
    /// self-contained colour-picker overlay — a modal dialog that paints through
    /// the global-overlay pass, so it floats over the editor without enlarging
    /// the strip (see [`layout`](Widget::layout), which excludes it from the
    /// reported size).
    fn rebuild(&mut self) {
        let mut col = FlexColumn::new().with_gap(6.0);
        let row1 = self.build_row1();
        if !row1.children().is_empty() {
            col.push(row1, 0.0);
        }
        let row2 = self.build_row2();
        if !row2.children().is_empty() {
            col.push(row2, 0.0);
        }
        let mut root: Vec<Box<dyn Widget>> = vec![Box::new(col)];
        if self.cfg.text_color || self.cfg.highlight {
            root.push(color::color_overlay(&self.font, &self.handle, &self.picker));
        }
        self.root = root;
    }

    /// Wrap `w` in a hover [`Tooltip`] labelled `text` when tooltips are enabled
    /// (the default); otherwise return it untouched.  Centralises the toolbar's
    /// default-on tooltip policy so every control participates uniformly and a
    /// single [`with_tooltips`](Self::with_tooltips) toggle gates them all.
    fn tip(&self, w: Box<dyn Widget>, text: impl Into<String>) -> Box<dyn Widget> {
        if self.cfg.tooltips {
            Box::new(Tooltip::new(w, text, Arc::clone(&self.font)))
        } else {
            w
        }
    }

    /// Row 1: inline character formatting, font family + size, colours.
    fn build_row1(&self) -> Box<dyn Widget> {
        let mut row = controls::new_row();
        let check = self.variant_check.as_ref();
        if self.cfg.bold {
            row = row.add(self.tip(
                controls::style_toggle(
                    &self.font,
                    &self.handle,
                    controls::ICON_BOLD,
                    |c| c.bold,
                    super::commands::RichCommand::ToggleBold,
                    Some(Variant::Bold),
                    check,
                ),
                "Bold",
            ));
        }
        if self.cfg.italic {
            row = row.add(self.tip(
                controls::style_toggle(
                    &self.font,
                    &self.handle,
                    controls::ICON_ITALIC,
                    |c| c.italic,
                    super::commands::RichCommand::ToggleItalic,
                    Some(Variant::Italic),
                    check,
                ),
                "Italic",
            ));
        }
        if self.cfg.underline {
            row = row.add(self.tip(
                controls::style_toggle(
                    &self.font,
                    &self.handle,
                    controls::ICON_UNDERLINE,
                    |c| c.underline,
                    super::commands::RichCommand::ToggleUnderline,
                    None,
                    None,
                ),
                "Underline",
            ));
        }
        if self.cfg.strikethrough {
            row = row.add(self.tip(
                controls::style_toggle(
                    &self.font,
                    &self.handle,
                    controls::ICON_STRIKE,
                    |c| c.strikethrough,
                    super::commands::RichCommand::ToggleStrikethrough,
                    None,
                    None,
                ),
                "Strikethrough",
            ));
        }
        if let Some(families) = &self.families {
            row = row.add(self.tip(
                controls::family_combo(&self.font, &self.handle, families),
                "Font family",
            ));
        }
        if self.cfg.font_size {
            row = row.add(self.tip(
                controls::size_combo(&self.font, &self.handle, &self.font_sizes),
                "Font size",
            ));
        }
        if self.cfg.text_color {
            row = row.add(self.tip(
                color::text_color_button(&self.font, &self.picker),
                "Text color",
            ));
        }
        if self.cfg.highlight {
            row = row.add(self.tip(
                color::highlight_button(&self.font, &self.picker),
                "Highlight color",
            ));
            row = row.add(self.tip(
                color::remove_highlight_button(&self.font, &self.handle),
                "Remove highlight",
            ));
        }
        Box::new(row)
    }

    /// Row 2: alignment, lists, indent, history.
    fn build_row2(&self) -> Box<dyn Widget> {
        let modifier = primary_modifier_label();
        let mut row = controls::new_row();
        if self.cfg.alignment {
            row = row.add(self.tip(
                controls::align_toggle(&self.font, &self.handle, controls::ICON_ALIGN_LEFT, TextHAlign::Left),
                "Align left",
            ));
            row = row.add(self.tip(
                controls::align_toggle(&self.font, &self.handle, controls::ICON_ALIGN_CENTER, TextHAlign::Center),
                "Align center",
            ));
            row = row.add(self.tip(
                controls::align_toggle(&self.font, &self.handle, controls::ICON_ALIGN_RIGHT, TextHAlign::Right),
                "Align right",
            ));
        }
        if self.cfg.lists {
            row = row.add(self.tip(
                controls::list_toggle(&self.font, &self.handle, controls::ICON_LIST_OL, ListKind::Ordered),
                "Numbered list",
            ));
            row = row.add(self.tip(
                controls::list_toggle(&self.font, &self.handle, controls::ICON_LIST_UL, ListKind::Bullet),
                "Bulleted list",
            ));
        }
        if self.cfg.indent {
            row = row.add(self.tip(
                controls::command_button(&self.font, &self.handle, controls::ICON_OUTDENT, super::commands::RichCommand::Outdent),
                "Decrease indent",
            ));
            row = row.add(self.tip(
                controls::command_button(&self.font, &self.handle, controls::ICON_INDENT, super::commands::RichCommand::Indent),
                "Increase indent",
            ));
        }
        if self.cfg.history {
            // Undo/redo are the only toolbar actions with a real key binding
            // (the editor binds `{mod}+Z` / `{mod}+Y`), so they get a shortcut hint.
            row = row.add(self.tip(
                controls::undo_button(&self.font, &self.handle),
                format!("Undo ({modifier}+Z)"),
            ));
            row = row.add(self.tip(
                controls::redo_button(&self.font, &self.handle),
                format!("Redo ({modifier}+Y)"),
            ));
        }
        Box::new(row)
    }

    /// Read-only view of the common style under the current selection (the data
    /// the toggles reflect); handy for tests and custom active-state logic.
    pub fn common_style_of_selection(&self) -> CommonStyle {
        self.handle.common_style_of_selection()
    }
}

impl Widget for RichTextToolbar {
    fn type_name(&self) -> &'static str {
        "RichTextToolbar"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.root
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.root
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

    fn measure_min_height(&self, available_w: f64) -> f64 {
        self.root
            .first()
            .map(|c| c.measure_min_height(available_w))
            .unwrap_or(0.0)
    }

    fn layout(&mut self, available: Size) -> Size {
        // root[0] (the rows) sizes the toolbar; root[1..] (the colour overlay)
        // is laid out so its modal dialog can size/position itself, but excluded
        // from the reported size so opening the picker never grows the strip —
        // the modal paints via the global-overlay pass regardless of bounds.
        let mut reported = Size::new(0.0, 0.0);
        for (i, child) in self.root.iter_mut().enumerate() {
            if i == 0 {
                let desired = child.layout(available);
                child.set_bounds(Rect::new(0.0, 0.0, desired.width, desired.height));
                reported = desired;
            } else {
                // The overlay's modal dialog must size itself against ample room
                // — a thin toolbar strip's `available` (a few dozen px tall)
                // would clamp the dialog's min-size against too small a slot and
                // panic. It positions itself via the paint-time viewport clamp,
                // so the generous layout size here has no effect on where it
                // lands. Zero bounds keep it out of normal hit-testing while
                // closed; its window paints via the global-overlay pass.
                child.layout(OVERLAY_LAYOUT_ROOM);
                child.set_bounds(Rect::new(0.0, 0.0, 0.0, 0.0));
            }
        }
        self.bounds = Rect::new(0.0, 0.0, reported.width, reported.height);
        reported
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

impl Drop for RichTextToolbar {
    /// If the toolbar is torn down while its colour dialog is still open
    /// mid-preview, the shared editor core would be left with undo suspended and
    /// a dangling preview snapshot (dead undo; a later `cancel_preview` could
    /// clobber the doc). Unwind the session so the editor stays usable.
    ///
    /// The guard fires only when *this* toolbar's own picker is open, and
    /// `cancel_preview` is a no-op when no session is active, so it never
    /// disturbs an unrelated editor. The held [`RichEditHandle`] keeps the core
    /// alive for the call, so this is safe regardless of drop order relative to
    /// the editor widget.
    fn drop(&mut self) {
        if self.picker.get() != PickerKind::None {
            self.handle.cancel_preview();
            self.picker.set(PickerKind::None);
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
