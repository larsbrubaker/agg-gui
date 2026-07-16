//! `RichTextToolbar` — a configurable, batteries-included formatting toolbar
//! driven by a [`RichEditHandle`].
//!
//! This is the library counterpart of the demo's toolbar
//! (`demo-ui/src/windows/rich_text_demo/toolbar.rs`): a self-contained widget an
//! embedding app can drop above a [`RichTextEdit`](super::RichTextEdit) to get
//! bold/italic/underline/strike, alignment, lists, indent/outdent, undo/redo, a
//! font-size dropdown, and text/highlight colours — all wired to the same
//! shared editor core the widget renders.  Every control group can be toggled
//! off through the builder; all are on by default.
//!
//! # Layout
//!
//! Controls flow across **two rows** (a [`FlexColumn`] of two [`FlexRow`]s),
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
//! # Colours
//!
//! The text/highlight swatches open a floating
//! [`color_wheel_picker_dialog`](crate::color_wheel_picker_dialog).  A floating
//! dialog needs the full canvas, which a thin toolbar strip cannot provide, so
//! the picker lives in a **companion overlay** returned by
//! [`color_overlay`](RichTextToolbar::color_overlay): add it to a top-level
//! [`Stack`](crate::widgets::primitives::Stack) that spans the editor area.  See
//! the example below.  (Colour changes currently apply on *Select* only; live
//! preview is pending a handle preview-session API — see [`color`].)
//!
//! # Example
//!
//! Extends the module-level embedding example with a toolbar and its colour
//! overlay, composed as `Stack[ FlexColumn[toolbar, editor], color_overlay ]`.
//!
//! ```
//! use std::sync::Arc;
//! use agg_gui::Font;
//! use agg_gui::widgets::rich_text::{single_font_resolver, RichDoc, RichTextEdit};
//! use agg_gui::widgets::rich_text::toolbar::{RichTextToolbar, Variant};
//! use agg_gui::{FlexColumn, Stack};
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
//! let color_overlay = toolbar.color_overlay();
//!
//! // The toolbar strip sits above the editor; the picker floats over the whole
//! // body via the companion overlay.
//! let body = FlexColumn::new()
//!     .add(Box::new(toolbar))
//!     .add_flex(Box::new(editor), 1.0);
//! let _root = Stack::new()
//!     .with_hit_children_only(false)
//!     .add(Box::new(body))
//!     .add_aligned(color_overlay);
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::flex::FlexColumn;

use super::commands::CommonStyle;
use super::editor::RichEditHandle;
use super::model::ListKind;

mod color;
mod controls;

use crate::widgets::text_area::TextHAlign;

/// Which colour the toolbar's floating picker is currently editing, shared
/// between the swatch buttons and the [`color_overlay`](RichTextToolbar::color_overlay).
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
        }
    }
}

/// Default font-size steps (points) offered by the size dropdown, 8–32.
const DEFAULT_FONT_SIZES: &[f64] = &[
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0,
];

/// A configurable formatting toolbar bound to a [`RichEditHandle`].
///
/// Build it with [`new`](Self::new), tweak the control roster and font
/// family/size options through the `with_*` builders, then place it in a layout
/// like any other widget.  For colours, also add [`color_overlay`](Self::color_overlay)
/// to a top-level stack (see the [module example](self)).
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

    /// Build the floating colour-picker overlay for this toolbar.  Add it to a
    /// top-level [`Stack`](crate::widgets::primitives::Stack) via `add_aligned`
    /// so the dialog can float over the editor (see the [module example](self)).
    /// Returns a zero-size layer while no swatch is open, and a no-op layer if
    /// both colour swatches are disabled.
    pub fn color_overlay(&self) -> Box<dyn Widget> {
        color::color_overlay(&self.font, &self.handle, &self.picker)
    }

    /// Reconstruct the two-row control tree from the current config.
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
        self.root = vec![Box::new(col)];
    }

    /// Row 1: inline character formatting, font family + size, colours.
    fn build_row1(&self) -> Box<dyn Widget> {
        let mut row = controls::new_row();
        let check = self.variant_check.as_ref();
        if self.cfg.bold {
            row = row.add(controls::style_toggle(
                &self.font,
                &self.handle,
                controls::ICON_BOLD,
                |c| c.bold,
                super::commands::RichCommand::ToggleBold,
                Some(Variant::Bold),
                check,
            ));
        }
        if self.cfg.italic {
            row = row.add(controls::style_toggle(
                &self.font,
                &self.handle,
                controls::ICON_ITALIC,
                |c| c.italic,
                super::commands::RichCommand::ToggleItalic,
                Some(Variant::Italic),
                check,
            ));
        }
        if self.cfg.underline {
            row = row.add(controls::style_toggle(
                &self.font,
                &self.handle,
                controls::ICON_UNDERLINE,
                |c| c.underline,
                super::commands::RichCommand::ToggleUnderline,
                None,
                None,
            ));
        }
        if self.cfg.strikethrough {
            row = row.add(controls::style_toggle(
                &self.font,
                &self.handle,
                controls::ICON_STRIKE,
                |c| c.strikethrough,
                super::commands::RichCommand::ToggleStrikethrough,
                None,
                None,
            ));
        }
        if let Some(families) = &self.families {
            row = row.add(controls::family_combo(&self.font, &self.handle, families));
        }
        if self.cfg.font_size {
            row = row.add(controls::size_combo(&self.font, &self.handle, &self.font_sizes));
        }
        if self.cfg.text_color {
            row = row.add(color::text_color_button(&self.font, &self.picker));
        }
        if self.cfg.highlight {
            row = row.add(color::highlight_button(&self.font, &self.picker));
        }
        Box::new(row)
    }

    /// Row 2: alignment, lists, indent, history.
    fn build_row2(&self) -> Box<dyn Widget> {
        let mut row = controls::new_row();
        if self.cfg.alignment {
            row = row.add(controls::align_toggle(&self.font, &self.handle, controls::ICON_ALIGN_LEFT, TextHAlign::Left));
            row = row.add(controls::align_toggle(&self.font, &self.handle, controls::ICON_ALIGN_CENTER, TextHAlign::Center));
            row = row.add(controls::align_toggle(&self.font, &self.handle, controls::ICON_ALIGN_RIGHT, TextHAlign::Right));
        }
        if self.cfg.lists {
            row = row.add(controls::list_toggle(&self.font, &self.handle, controls::ICON_LIST_OL, ListKind::Ordered));
            row = row.add(controls::list_toggle(&self.font, &self.handle, controls::ICON_LIST_UL, ListKind::Bullet));
        }
        if self.cfg.indent {
            row = row.add(controls::command_button(&self.font, &self.handle, controls::ICON_OUTDENT, super::commands::RichCommand::Outdent));
            row = row.add(controls::command_button(&self.font, &self.handle, controls::ICON_INDENT, super::commands::RichCommand::Indent));
        }
        if self.cfg.history {
            row = row.add(controls::undo_button(&self.font, &self.handle));
            row = row.add(controls::redo_button(&self.font, &self.handle));
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
        if let Some(child) = self.root.first_mut() {
            let desired = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, desired.width, desired.height));
            self.bounds = Rect::new(0.0, 0.0, desired.width, desired.height);
            desired
        } else {
            Size::new(0.0, 0.0)
        }
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
