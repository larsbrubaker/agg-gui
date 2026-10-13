//! `MarkdownFormatToolbar` — the rich Markdown editor's formatting strip:
//! character styles, text size, alignment, lists and quote, then link, code
//! block and table, each run of related buttons an outlined group of Font
//! Awesome icons.  Ports agg-sharp's `MarkdownFormatToolbar.cs` (with
//! `MarkdownLinkRequestEventArgs`).
//!
//! Every button acts through [`RichEditCommands`] and shows the state at the
//! caret or selection: a style button is pressed when all the selection has
//! the style and shaded when only part does, the size list is blank over
//! mixed sizes, and alignment is disabled in lists and quotes, where it is not
//! offered.  The widgets (buttons, groups, the size list, the wrapping strip)
//! are in `format_toolbar_widgets`; the link row is `link_editor`.
//!
//! Differences from C#, from Rust's ownership rather than behaviour:
//! - The toolbar does not hold the commands: C#'s `Commands` setter is
//!   [`MarkdownFormatToolbar::attach`], and every call that acts takes the
//!   commands (`&mut dyn RichEditCommands`).  A button the user clicks queues
//!   its action, and the selection- and document-changed handlers it adds
//!   only raise flags; [`MarkdownFormatToolbar::sync`] runs the queued
//!   actions and the refresh those flags ask for.  The composite
//!   `RichMarkdownEditor` calls it after every event and layout, so it acts
//!   within the frame.  C#'s `InvokeClick` is [`MarkdownFormatToolbar::click`].
//! - The handlers cannot be removed from a [`RichEditCommands`], so a toolbar
//!   attached to new commands ignores the old ones' handlers by generation.
//! - agg-gui has no `ThemeConfig`: the group height is the buttons' 28 design
//!   units plus the outline (C# also takes the theme's field height when that
//!   is taller), and the colours are agg-gui's visuals.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::geometry::Rect;
use crate::icon_font;
use crate::layout_props::WidgetBase;
use crate::text::Font;
use crate::widget::Widget;

use super::block::{RichAlignment, RichBlockKind};
use super::block_operations::{self, RichListKind};
use super::code_block_operations;
use super::document::{DocPosition, RichDocument};
use super::edit_commands::RichEditCommands;
use super::edit_error::RichEditError;
use super::inline_style::{RichInlineStyle, RichPendingStyle, RichStyleCoverage};
use super::link_editor::{with_scheme, LinkRowRequest, MarkdownLinkEditor};
use super::markdown_writer;
use super::selection::RichSelection;
use super::style_operations;
use super::table_code_operations;

/// The size list's fixed entries; "Heading 4" - "Heading 6" are added while
/// the selection is on one.
pub const SIZE_NAMES: [&str; 4] = ["Normal", "Heading 1", "Heading 2", "Heading 3"];

/// Design units of space above the groups and, matching it, below them, so
/// the row does not sit tight under a host's tab bar or rest on the text.
pub const DESIGN_EDGE_SPACE: f64 = 6.0;

/// Design units of padding above and below each row of the wrapping strip.
pub(super) const ROW_DESIGN_PADDING: f64 = 1.0;

/// A toolbar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkdownFormatButtonKind {
    Bold,
    Italic,
    Strike,
    Code,
    AlignLeft,
    AlignCenter,
    AlignRight,
    BulletList,
    NumberedList,
    Quote,
    Link,
    CodeBlock,
    Table,
}

impl MarkdownFormatButtonKind {
    /// Every button, in reading order.
    pub const ALL: [Self; 13] = [
        Self::Bold,
        Self::Italic,
        Self::Strike,
        Self::Code,
        Self::AlignLeft,
        Self::AlignCenter,
        Self::AlignRight,
        Self::BulletList,
        Self::NumberedList,
        Self::Quote,
        Self::Link,
        Self::CodeBlock,
        Self::Table,
    ];

    /// The groups, left to right: character styles, alignment, lists and
    /// quote, then link, code block and table.  The size list stands alone
    /// between the first two.
    pub const GROUPS: [&'static [Self]; 4] = [
        &[Self::Bold, Self::Italic, Self::Strike, Self::Code],
        &[Self::AlignLeft, Self::AlignCenter, Self::AlignRight],
        &[Self::BulletList, Self::NumberedList, Self::Quote],
        &[Self::Link, Self::CodeBlock, Self::Table],
    ];

    pub(super) fn index(self) -> usize {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0)
    }

    /// The Font Awesome glyph that names the button (`crate::icon_font`); the toolbar draws its own
    /// vector icon instead (`toolbar_icons`).
    pub fn icon(self) -> char {
        match self {
            Self::Bold => icon_font::BOLD,
            Self::Italic => icon_font::ITALIC,
            Self::Strike => icon_font::STRIKETHROUGH,
            Self::Code => icon_font::CODE,
            Self::AlignLeft => icon_font::ALIGN_LEFT,
            Self::AlignCenter => icon_font::ALIGN_CENTER,
            Self::AlignRight => icon_font::ALIGN_RIGHT,
            Self::BulletList => icon_font::LIST_UL,
            Self::NumberedList => icon_font::LIST_OL,
            Self::Quote => icon_font::QUOTE_LEFT,
            Self::Link => icon_font::LINK,
            Self::CodeBlock => icon_font::FILE_CODE,
            Self::Table => icon_font::TABLE,
        }
    }

    /// The button's tooltip; `mac` spells the shortcuts for a Mac.
    pub fn tool_tip(self, mac: bool) -> String {
        match self {
            Self::Bold => format!("Bold ({})", shortcut("B", mac)),
            Self::Italic => format!("Italic ({})", shortcut("I", mac)),
            Self::Strike => "Strikethrough".into(),
            Self::Code => "Inline code".into(),
            Self::AlignLeft => "Align left".into(),
            Self::AlignCenter => "Center".into(),
            Self::AlignRight => "Align right".into(),
            Self::BulletList => "Bulleted list".into(),
            Self::NumberedList => "Numbered list".into(),
            Self::Quote => "Quote".into(),
            Self::Link => "Add or edit a link".into(),
            Self::CodeBlock => "Code block".into(),
            Self::Table => "Insert a table".into(),
        }
    }
}

/// A tooltip's shortcut spelled in words ("Command+B" on a Mac, "Ctrl+B"
/// elsewhere): the UI font has no modifier-key symbols.
pub fn shortcut(key: &str, mac: bool) -> String {
    format!("{}{key}", if mac { "Command+" } else { "Ctrl+" })
}

/// What a button shows: whether it can be pressed, and how much of the
/// selection has its style (C#'s `Enabled` and `Coverage`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkdownFormatButtonState {
    pub enabled: bool,
    pub coverage: RichStyleCoverage,
}

impl MarkdownFormatButtonState {
    /// Pressed: the style covers the whole selection.
    pub fn is_pressed(&self) -> bool {
        self.coverage == RichStyleCoverage::All
    }
}

/// A host's link dialog, raised with the current link's url (`None` for no
/// link) — C#'s `LinkRequested` with `MarkdownLinkRequestEventArgs`.
pub type MarkdownLinkRequestHandler = Box<dyn FnMut(Option<&str>)>;

/// A user action waiting for the commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToolbarRequest {
    Click(MarkdownFormatButtonKind),
    Size(usize),
}

/// What the toolbar's widgets read and its handlers flag.
#[derive(Default)]
pub(super) struct ToolbarModel {
    pub(super) buttons: [MarkdownFormatButtonState; 13],
    pub(super) size_enabled: bool,
    /// The size list's entry, `None` when blank (mixed sizes).
    pub(super) size_index: Option<usize>,
    pub(super) size_items: Vec<String>,
    /// The heading level of the extra entry ("Heading 4" - "Heading 6"), or 0.
    pub(super) extra_size_level: u8,
}

#[derive(Default)]
pub(super) struct ToolbarShared {
    pub(super) model: RefCell<ToolbarModel>,
    pub(super) requests: RefCell<Vec<ToolbarRequest>>,
    generation: Cell<u64>,
    /// True while a toolbar edit runs: its handler calls are answered by one
    /// refresh after it.
    editing: Cell<bool>,
    selection_moved: Cell<bool>,
    document_changed: Cell<bool>,
}

impl ToolbarShared {
    pub(super) fn push(&self, request: ToolbarRequest) {
        self.requests.borrow_mut().push(request);
        crate::animation::request_draw();
    }
}

static NEXT_FOCUS_ID: AtomicU64 = AtomicU64::new(0x4D44_4544_0000_0000);

/// A focus id for the markdown editor's own widgets, from a range apps do not
/// pick by hand.
pub(super) fn next_focus_id() -> crate::focus::FocusId {
    NEXT_FOCUS_ID.fetch_add(1, Ordering::Relaxed)
}

/// The formatting toolbar; see the module header.
pub struct MarkdownFormatToolbar {
    pub(super) bounds: Rect,
    pub(super) base: WidgetBase,
    /// The wrapping strip, then the link row.
    pub(super) children: Vec<Box<dyn Widget>>,
    pub(super) shared: Rc<ToolbarShared>,
    link_editor: MarkdownLinkEditor,
    link_field_focus: crate::focus::FocusId,
    link_requested: Vec<MarkdownLinkRequestHandler>,
    /// The selection the link row opened on: the row's box takes the focus,
    /// and the link must land on the text selected when Link was pressed.
    link_selection: Option<RichSelection>,
}

impl MarkdownFormatToolbar {
    /// A toolbar in `font` with `icon_font`'s (Font Awesome 4.7) glyphs,
    /// spelling its shortcuts for a Mac when `mac`.  Attach it to the
    /// document's owner with [`Self::attach`].
    pub fn new(font: Arc<Font>, icon_font: Arc<Font>, mac: bool) -> Self {
        let shared = Rc::new(ToolbarShared::default());
        shared.model.borrow_mut().size_items = SIZE_NAMES.iter().map(|s| s.to_string()).collect();
        let link_editor = MarkdownLinkEditor::new();
        let link_field_focus = next_focus_id();
        let strip = super::format_toolbar_widgets::build_strip(&shared, &font, &icon_font, mac);
        let row = link_editor.widget(Arc::clone(&font), link_field_focus);
        Self {
            bounds: Rect::default(),
            base: super::format_toolbar_widgets::toolbar_base(),
            children: vec![Box::new(strip), Box::new(row)],
            shared,
            link_editor,
            link_field_focus,
            link_requested: Vec::new(),
            link_selection: None,
        }
    }

    /// Makes `commands` what the toolbar edits (C#'s `Commands` setter): the
    /// toolbar follows its handlers, stops following the previous commands',
    /// closes the link row (its captured selection belongs to the old
    /// document) and refreshes.
    pub fn attach(&mut self, commands: &mut dyn RichEditCommands) {
        let generation = self.shared.generation.get() + 1;
        self.shared.generation.set(generation);
        self.link_editor.close();
        self.link_selection = None;
        let weak = Rc::downgrade(&self.shared);
        commands.on_selection_changed(flag_handler(weak.clone(), generation, |s| {
            s.selection_moved.set(true)
        }));
        commands.on_document_changed(flag_handler(weak, generation, |s| {
            s.document_changed.set(true)
        }));
        self.shared.selection_moved.set(false);
        self.shared.document_changed.set(false);
        self.refresh(commands);
    }

    /// Stops following any commands (C#'s `Commands = null`): every control
    /// shows the empty state.
    pub fn detach(&mut self) {
        self.shared.generation.set(self.shared.generation.get() + 1);
        self.link_editor.close();
        self.link_selection = None;
        let mut model = self.shared.model.borrow_mut();
        model.buttons = Default::default();
        model.size_enabled = false;
    }

    /// Runs the actions the user queued and the refresh the commands'
    /// handlers asked for.  A caret that moved under an open link row closes
    /// it: the row no longer matches it.
    pub fn sync(&mut self, commands: &mut dyn RichEditCommands) -> Result<(), RichEditError> {
        let mut result = Ok(());
        let requests = std::mem::take(&mut *self.shared.requests.borrow_mut());
        for request in requests {
            let step = match request {
                ToolbarRequest::Click(kind) => self.click(kind, commands),
                ToolbarRequest::Size(index) => self.set_size_index(index, commands),
            };
            result = result.and(step);
        }
        for request in self.link_editor.take_requests() {
            let step = match request {
                LinkRowRequest::Accept => {
                    let url = self.link_editor.url();
                    self.apply_link_on(&url, self.link_selection, commands)
                }
                LinkRowRequest::Remove => self.edit(commands, self.link_selection, |d, s| {
                    style_operations::remove_link(d, s)
                }),
                LinkRowRequest::Cancel => {
                    commands.focus_text();
                    Ok(())
                }
            };
            result = result.and(step);
        }
        let moved = self.shared.selection_moved.replace(false);
        let changed = self.shared.document_changed.replace(false);
        if moved {
            self.link_editor.close();
        }
        if moved || changed {
            self.refresh(commands);
        }
        result
    }

    /// Presses `kind` (C#'s `InvokeClick`).
    pub fn click(
        &mut self,
        kind: MarkdownFormatButtonKind,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        use MarkdownFormatButtonKind as K;
        let alignment = |a: RichAlignment| {
            move |d: &mut RichDocument, s: RichSelection| {
                block_operations::set_alignment(d, s, a);
                Ok(s)
            }
        };
        match kind {
            K::Bold => self.toggle_style(RichInlineStyle::BOLD, commands),
            K::Italic => self.toggle_style(RichInlineStyle::ITALIC, commands),
            K::Strike => self.toggle_style(RichInlineStyle::STRIKE, commands),
            K::Code => self.toggle_style(RichInlineStyle::CODE, commands),
            K::AlignLeft => self.edit(commands, None, alignment(RichAlignment::Left)),
            K::AlignCenter => self.edit(commands, None, alignment(RichAlignment::Center)),
            K::AlignRight => self.edit(commands, None, alignment(RichAlignment::Right)),
            K::BulletList | K::NumberedList => {
                let ordered = kind == K::NumberedList;
                self.edit(commands, None, move |d, s| {
                    block_operations::toggle_list(d, s, ordered);
                    Ok(s)
                })
            }
            K::Quote => self.edit(commands, None, |d, s| {
                block_operations::toggle_quote(d, s);
                Ok(s)
            }),
            K::Link => {
                self.show_link_editor(commands);
                Ok(())
            }
            K::CodeBlock => self.edit(commands, None, code_block_operations::toggle_code_block),
            K::Table => self.edit(commands, None, |d, s| {
                table_code_operations::insert_table(d, s.end(), 1, 2)
            }),
        }
    }

    /// Picks size-list entry `index` (C#'s `SizeList.SelectedIndex` set by
    /// the user): Normal, a heading, or the extra deep heading entry.
    pub fn set_size_index(
        &mut self,
        index: usize,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        let (count, extra) = {
            let model = self.shared.model.borrow();
            (model.size_items.len(), model.extra_size_level)
        };
        if index >= count {
            return Ok(());
        }
        let level = if index < SIZE_NAMES.len() {
            index as u8
        } else {
            extra
        };
        self.edit(commands, None, move |d, s| {
            block_operations::set_block_kind(d, s, level)?;
            Ok(s)
        })
    }

    /// What button `kind` shows.
    pub fn button(&self, kind: MarkdownFormatButtonKind) -> MarkdownFormatButtonState {
        self.shared.model.borrow().buttons[kind.index()]
    }

    /// The size list's entry, `None` when blank (C#'s `SelectedIndex` -1).
    pub fn size_index(&self) -> Option<usize> {
        self.shared.model.borrow().size_index
    }

    /// The size list's shown label ("" when blank).
    pub fn size_label(&self) -> String {
        let model = self.shared.model.borrow();
        model
            .size_index
            .and_then(|i| model.size_items.get(i).cloned())
            .unwrap_or_default()
    }

    /// The size list's entries.
    pub fn size_items(&self) -> Vec<String> {
        self.shared.model.borrow().size_items.clone()
    }

    /// Whether the size list can be used.
    pub fn size_enabled(&self) -> bool {
        self.shared.model.borrow().size_enabled
    }

    /// The built-in url row under the strip, shown by the Link button when
    /// no host handles [`Self::on_link_requested`].
    pub fn link_editor(&self) -> &MarkdownLinkEditor {
        &self.link_editor
    }

    /// The link row's OK (or Enter) — C#'s `LinkEditor.Accept()`.
    pub fn accept_link_editor(
        &mut self,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        self.link_editor.request(LinkRowRequest::Accept);
        self.sync(commands)
    }

    /// The link row's Remove — C#'s `LinkEditor.Remove()`.
    pub fn remove_link_editor(
        &mut self,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        self.link_editor.request(LinkRowRequest::Remove);
        self.sync(commands)
    }

    /// The link row's Cancel (or Escape) — C#'s `LinkEditor.Cancel()`.
    pub fn cancel_link_editor(
        &mut self,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        self.link_editor.request(LinkRowRequest::Cancel);
        self.sync(commands)
    }

    /// Adds a handler that replaces the link row: the Link button (and the
    /// editor's Cmd/Ctrl+K) then raise it with the current link's url, and
    /// the host calls [`Self::apply_link`] or [`Self::remove_link`] (C#'s
    /// `LinkRequested`).
    pub fn on_link_requested(&mut self, handler: MarkdownLinkRequestHandler) {
        self.link_requested.push(handler);
    }

    /// Links the selection (or the link at the caret) to `url`; an empty url
    /// removes the link, as clearing the box in a link dialog does elsewhere.
    pub fn apply_link(
        &mut self,
        url: &str,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        self.apply_link_on(url, None, commands)
    }

    /// Removes the link at the caret or over the selection, keeping its text.
    pub fn remove_link(
        &mut self,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        self.edit(commands, None, style_operations::remove_link)
    }

    /// The Link button: a host's dialog if it asked for one, otherwise the
    /// built-in url row, focused.
    pub fn show_link_editor(&mut self, commands: &mut dyn RichEditCommands) {
        let document = commands.document();
        if document.blocks.is_empty() {
            return;
        }
        let selection = clamp(document, commands.selection());
        self.link_selection = Some(selection);
        let url = style_operations::style_at(document, selection, RichPendingStyle::default())
            .ok()
            .and_then(|state| state.link_url);
        if !self.link_requested.is_empty() {
            for handler in &mut self.link_requested {
                handler(url.as_deref());
            }
            return;
        }
        self.link_editor.open(url.as_deref());
        crate::focus::request_focus(self.link_field_focus);
    }

    /// Brings every control in line with the caret or selection.  Runs on
    /// each selection and document change.
    pub fn refresh(&mut self, commands: &dyn RichEditCommands) {
        use MarkdownFormatButtonKind as K;
        let document = commands.document();
        let has_document = !document.blocks.is_empty();
        let selection = if has_document {
            clamp(document, commands.selection())
        } else {
            RichSelection::default()
        };
        let style = if has_document {
            style_operations::style_at(document, selection, commands.pending_style())
                .unwrap_or_default()
        } else {
            Default::default()
        };
        let block = if has_document {
            block_operations::block_state_at(document, selection)
        } else {
            Default::default()
        };
        let styled = has_document && touches(document, selection, true);
        let text_blocks = has_document && touches(document, selection, false);

        let mut model = self.shared.model.borrow_mut();
        let mut set = |kind: K, enabled: bool, coverage: RichStyleCoverage| {
            model.buttons[kind.index()] = MarkdownFormatButtonState { enabled, coverage };
        };
        for (kind, flag) in [
            (K::Bold, RichInlineStyle::BOLD),
            (K::Italic, RichInlineStyle::ITALIC),
            (K::Strike, RichInlineStyle::STRIKE),
            (K::Code, RichInlineStyle::CODE),
        ] {
            let coverage = if styled {
                style.coverage(flag)
            } else {
                RichStyleCoverage::None
            };
            set(kind, styled, coverage);
        }

        for (kind, alignment) in [
            (K::AlignLeft, RichAlignment::Left),
            (K::AlignCenter, RichAlignment::Center),
            (K::AlignRight, RichAlignment::Right),
        ] {
            set(
                kind,
                block.can_align,
                all_if(block.can_align && block.alignment == Some(alignment)),
            );
        }

        set(
            K::BulletList,
            text_blocks,
            list_coverage(block.list, RichListKind::Bullet),
        );
        set(
            K::NumberedList,
            text_blocks,
            list_coverage(block.list, RichListKind::Numbered),
        );
        set(K::Quote, text_blocks, all_if(block.quote));

        let link_enabled = styled
            || (has_document
                && selection.is_empty()
                && style_operations::typing_style(document, selection.caret).is_some());
        set(K::Link, link_enabled, all_if(style.link_url.is_some()));
        set(
            K::CodeBlock,
            has_document && code_block_operations::can_toggle_code_block(document, selection),
            all_if(
                has_document
                    && document.blocks[selection.start().block_index].kind
                        == RichBlockKind::CodeBlock,
            ),
        );
        set(K::Table, has_document, RichStyleCoverage::None);

        model.size_enabled = text_blocks;
        show_size(
            &mut model,
            if text_blocks {
                block.heading_level
            } else {
                Some(0)
            },
        );
        drop(model);
        crate::animation::request_draw();
    }

    fn apply_link_on(
        &mut self,
        url: &str,
        on: Option<RichSelection>,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        let url = with_scheme(url);
        if url.is_empty() {
            return self.edit(commands, on, style_operations::remove_link);
        }
        self.edit(commands, on, move |d, s| {
            style_operations::set_link(d, s, &url, None)
        })
    }

    fn toggle_style(
        &mut self,
        style: RichInlineStyle,
        commands: &mut dyn RichEditCommands,
    ) -> Result<(), RichEditError> {
        // With nothing selected the style applies to what is typed next, as in any word processor.
        if commands.selection().is_empty() {
            // Setting it raises the selection-changed handlers, which refresh the buttons.
            let pending = commands.pending_style().toggle(style);
            commands.set_pending_style(pending);
            self.sync(commands)?;
            commands.focus_text();
            return Ok(());
        }
        self.edit(commands, None, move |d, s| {
            style_operations::toggle_style(d, s, style)
        })
    }

    /// Runs one toolbar edit through the seam on `on` (or the current
    /// selection), reporting a change only when the markdown it writes
    /// differs, so a click that did nothing leaves no undo step.  Refreshes
    /// once afterwards, rather than once per handler the edit raises, and
    /// hands the focus back to the text.
    fn edit(
        &mut self,
        commands: &mut dyn RichEditCommands,
        on: Option<RichSelection>,
        mut op: impl FnMut(&mut RichDocument, RichSelection) -> Result<RichSelection, RichEditError>,
    ) -> Result<(), RichEditError> {
        if commands.document().blocks.is_empty() {
            return Ok(());
        }
        self.shared.editing.set(true);
        let result = commands.apply_edit(&mut |document, selection| {
            let before = markdown_writer::write(document);
            let next = op(document, clamp(document, on.unwrap_or(selection)))?;
            let changed = markdown_writer::write(document) != before;
            Ok((next, changed))
        });
        self.shared.editing.set(false);
        self.shared.selection_moved.set(false);
        self.shared.document_changed.set(false);
        self.refresh(commands);
        commands.focus_text();
        result
    }
}

fn flag_handler(
    shared: Weak<ToolbarShared>,
    generation: u64,
    flag: fn(&ToolbarShared),
) -> Box<dyn FnMut()> {
    Box::new(move || {
        if let Some(shared) = shared.upgrade() {
            if shared.generation.get() == generation && !shared.editing.get() {
                flag(&shared);
                crate::animation::request_draw();
            }
        }
    })
}

fn all_if(all: bool) -> RichStyleCoverage {
    if all {
        RichStyleCoverage::All
    } else {
        RichStyleCoverage::None
    }
}

/// A list button shows mixed when the selection holds some items but not all
/// of that kind.
fn list_coverage(state: RichListKind, kind: RichListKind) -> RichStyleCoverage {
    if state == kind {
        RichStyleCoverage::All
    } else if state == RichListKind::Mixed {
        RichStyleCoverage::Mixed
    } else {
        RichStyleCoverage::None
    }
}

/// Selects the size list's entry for `level`: blank for `None` (mixed sizes),
/// and a "Heading N" entry added for levels 4-6 so a real heading never reads
/// as blank.
fn show_size(model: &mut ToolbarModel, level: Option<u8>) {
    let names = SIZE_NAMES.len() as u8;
    let extra = match level {
        Some(deep) if deep >= names => deep,
        _ => 0,
    };
    if extra != model.extra_size_level {
        model.size_items.truncate(SIZE_NAMES.len());
        if extra != 0 {
            model.size_items.push(format!("Heading {extra}"));
        }
        model.extra_size_level = extra;
    }
    model.size_index = level.map(|shown| usize::from(shown.min(names)));
}

/// Whether the selection touches a paragraph-like block (or, with `tables`, a
/// table cell).  Code and Raw blocks have no character or block styles, so
/// those buttons are disabled there rather than doing nothing when pressed.
fn touches(document: &RichDocument, selection: RichSelection, tables: bool) -> bool {
    let last = selection
        .end()
        .block_index
        .min(document.blocks.len().saturating_sub(1));
    (selection.start().block_index..=last).any(|i| {
        document.blocks.get(i).is_some_and(|block| {
            block.is_text_block() || (tables && block.kind == RichBlockKind::Table)
        })
    })
}

/// A selection kept inside the document, so a stale one (an implementation
/// that raised its handlers before storing the new selection, or a document
/// replaced under us) never indexes past the blocks.
pub(super) fn clamp(document: &RichDocument, selection: RichSelection) -> RichSelection {
    let clamp_position = |position: DocPosition| {
        let index = position
            .block_index
            .min(document.blocks.len().saturating_sub(1));
        let Some(block) = document.blocks.get(index) else {
            return DocPosition::new(0, 0);
        };
        let (mut row, mut column) = (0, 0);
        if block.kind == RichBlockKind::Table {
            if block.table_rows.first().map_or(true, |r| r.is_empty()) {
                return DocPosition::new(index, 0);
            }
            row = position.row.min(block.table_rows.len() - 1);
            column = position
                .column
                .min(block.table_rows[row].len().saturating_sub(1));
        }
        let offset = position.offset.min(block.text_length(row, column));
        DocPosition::in_cell(index, offset, row, column)
    };
    RichSelection {
        anchor: clamp_position(selection.anchor),
        caret: clamp_position(selection.caret),
        ..selection
    }
}
