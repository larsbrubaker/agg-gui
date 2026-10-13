//! `markdown_editing` — the rich Markdown editor's document model, a port of
//! agg-sharp's `Markdig.Agg.Editing` (`MarkdigAgg/Editing/`).
//!
//! The editor reads Markdown into a [`RichDocument`] (blocks of styled inlines
//! that partition the source exactly), edits that model, and writes Markdown
//! back: unedited blocks verbatim, edited (`dirty`) blocks regenerated.  It is
//! a separate model from [`super::rich_text`]'s `RichDoc`: this one keeps the
//! Markdown source structure (headings, list groups, quotes, code blocks,
//! tables, `<div align>` wrappers) a byte-identical round trip needs.
//!
//! # Module layout
//!
//! * [`inline`] — [`RichInline`]: a styled run of text or an atom the caret
//!   steps over whole (`RichInline.cs`).
//! * [`inlines`] — offset arithmetic over a list of inlines (`RichInlines.cs`).
//! * [`block`] — [`RichBlock`] and its kinds, list info and table cells
//!   (`RichBlock.cs`).
//! * [`groups`] — the shared `<div align>` / blockquote / list groups
//!   (`RichBlockGroups.cs`).
//! * [`document`] — [`RichDocument`] and [`DocPosition`] (`RichDocument.cs`).
//! * [`markdown_parser`] — markdown into the document, its blocks partitioning
//!   the source exactly (`RichMarkdownParser.cs`), on pulldown-cmark's source
//!   ranges (`parse_tree`); `parser_inlines` reads inlines,
//!   `code_and_tables` code blocks and pipe tables (`RichCodeAndTables.cs`),
//!   `align_groups` folds `<div align>` wrappers (`RichAlignGroups.cs`), and
//!   `pipe_tables` finds pipe tables as Markdig does (the viewer uses it too).
//! * [`markdown_writer`] — the document back to markdown: unedited blocks and
//!   groups verbatim, edited ones regenerated (`RichMarkdownWriter.cs`).
//! * [`group_writer`] — changed lists, quotes and `<div align>` wrappers
//!   rebuilt whole (`RichGroupWriter.cs`).
//! * [`inline_writer`] — a list of inlines as markdown (`RichInlineWriter.cs`).
//! * `escaping` — escaped text, code spans and link tails
//!   (`RichMarkdownEscaping.cs`).
//! * [`selection`] — [`RichSelection`], what an edit returns to show next
//!   (`RichSelection.cs`).
//! * [`edit_operations`] — typing, range deletes, Enter, Backspace and Delete
//!   (`RichEditOperations.cs`); [`table_code_operations`] the same inside a
//!   code block or table, plus the table's row/column operations
//!   (`RichTableCodeOperations.cs`); `group_repair` restores the group
//!   invariants after an edit (`RichGroupRepair.cs`).  Where C# throws on a
//!   caller's mistake the operations return a [`RichEditError`]
//!   ([`edit_error`]).
//! * [`style_operations`] — bold, italic, code, strike and links over a
//!   selection, and the style the toolbar shows (`RichStyleOperations.cs`),
//!   with the style types in [`inline_style`].
//! * [`block_operations`] — Normal/Heading, lists, quotes, alignment,
//!   indent/outdent and the line-start shortcuts (`RichBlockOperations.cs`);
//!   `block_containers` turns blocks into list items or quotes and joins the
//!   groups they touch.
//! * [`code_block_operations`] — the Code block button: text blocks into one
//!   fenced code block and back (`RichCodeBlockOperations.cs`).
//! * [`paste_operations`] — copy and paste without a clipboard: a selection
//!   sliced out as a fragment document, its visible text, clipboard text read
//!   as a fragment, and `paste_insert` pasting one at a caret
//!   (`RichPasteOperations.cs`).
//! * [`edit_history`] — undo/redo snapshots and which edits merge into one
//!   step (`RichEditHistory.cs`).
//! * [`edit_commands`] — the toolbar's view of the document's owner, and a
//!   widget-free implementation (`IRichEditCommands.cs`).
//! * `char_class` — .NET's character classes, as the writer sees them.
//! * [`block_layout`] — one block laid out to a width: wrapped lines, the
//!   list marker, quote bar and code background, hit testing and caret boxes
//!   (`RichBlockLayout.cs`, `IRichBlockLayout.cs`), over [`layout_line`]'s
//!   lines, fragments and caret stops (`RichLayoutLine.cs`), `layout_items`'
//!   items and line breaking (`RichLayoutItems.cs`, with `mark_class`'s
//!   combining marks) and [`layout_style`]'s fonts, colours and spacing
//!   (`RichLayoutStyle.cs`).
//! * [`table_layout`] — a Table block laid out to a width: the grid, padded
//!   cells laid out like paragraphs, header and stripe boxes, hit testing and
//!   pixel-snapped grid lines (`RichTableLayout.cs`).
//! * [`block_painter`] — paints a laid-out text block or table through a
//!   `DrawCtx`: backgrounds, quote bar, list marker, grid, text, images,
//!   selection and caret (`RichBlockPainter.cs`).
//! * [`document_view`] — every block's cached layout stacked top to bottom,
//!   list numbers and list ends, relayout of only what an edit changed, and
//!   painting of the blocks in view (`RichDocumentView.cs`).
//! * [`caret_navigator`] — where the caret keys take the caret through the
//!   laid-out lines, cells and blocks (`RichCaretNavigator.cs`).
//! * [`caret_blink`] — the caret's 0.6 s on / 0.6 s off blink on the UI
//!   clock (`RichCaretBlink.cs`).
//! * [`editor_state`] — [`RichEditorState`], the editor without its widget:
//!   document, selection, history, pending style, view, scroll and the one
//!   edit pipeline (`RichMarkdownEditWidget.cs`'s state,
//!   `RichEditorKeyboard.Apply`); `editor_keyboard` its keys
//!   (`RichEditorKeyboard.cs`), [`editor_mouse`] its mouse
//!   (`RichEditorMouse.cs`) and `editor_clipboard` cut, copy and paste
//!   (`RichEditorClipboard.cs`).
//! * [`edit_widget`] — [`RichMarkdownEditWidget`], the editor as an agg-gui
//!   widget over the state (`RichMarkdownEditWidget.cs`); `edit_widget_events`
//!   its events, scroll bar, auto-scroll and painting, [`raw_hosts`] the Raw
//!   blocks' rendered hosts and [`image_cache`] its images
//!   (`RichImageCache.cs`).
//! * [`format_toolbar`] — [`MarkdownFormatToolbar`], the formatting strip
//!   acting through [`RichEditCommands`] (`MarkdownFormatToolbar.cs`), with
//!   `format_toolbar_widgets` its buttons, groups and size list
//!   (`MarkdownFormatButton.cs`, `MarkdownFormatGroup.cs`) and [`link_editor`]
//!   its link row (`MarkdownLinkEditor.cs`).
//! * [`rich_markdown_editor`] — [`RichMarkdownEditor`], the toolbar over the
//!   editing widget (`RichMarkdownEditor.cs`).
//!
//! # Offsets
//!
//! A caret offset counts each run character (a Rust `char`, a Unicode scalar
//! value) and each atom as one.  agg-sharp counts UTF-16 code units, so a
//! character outside the Basic Multilingual Plane (most emoji) is one caret
//! position here and two there; the two agree on all BMP text.

mod align_groups;
pub mod block;
mod block_containers;
pub mod block_layout;
pub mod block_operations;
pub mod block_painter;
pub mod caret_blink;
pub mod caret_navigator;
mod char_class;
mod code_and_tables;
pub mod code_block_operations;
pub mod document;
pub mod document_view;
pub mod edit_commands;
pub mod edit_error;
pub mod edit_history;
pub mod edit_operations;
pub mod edit_widget;
mod edit_widget_events;
mod editor_clipboard;
mod editor_keyboard;
pub mod editor_mouse;
pub mod editor_state;
mod escaping;
pub mod format_toolbar;
mod format_toolbar_cells;
mod format_toolbar_widgets;
mod group_repair;
pub mod group_writer;
pub mod groups;
pub mod image_cache;
pub mod inline;
pub mod inline_style;
pub mod inline_writer;
pub mod inlines;
mod layout_items;
pub mod layout_line;
pub mod layout_style;
pub mod link_editor;
mod mark_class;
pub mod markdown_parser;
pub mod markdown_writer;
mod parse_tree;
pub(crate) mod parser_inlines;
mod paste_insert;
pub mod paste_operations;
pub(crate) mod pipe_tables;
pub mod raw_hosts;
pub mod rich_markdown_editor;
pub mod selection;
pub mod style_operations;
pub mod table_code_operations;
pub mod table_layout;
mod toolbar_icons;

pub use block::{
    RichAlignment, RichBlock, RichBlockId, RichBlockKind, RichListInfo, RichTableCell,
};
pub use block_layout::{RichBlockGeometry, RichBlockLayout, RichLayoutError};
pub use block_operations::{RichBlockState, RichListKind};
pub use block_painter::{RichBlockPainter, RichEditColors, RichImage, RichLayout, RichPaintInputs};
pub use document::{DocPosition, RichDocument};
pub use document_view::{RichDocumentView, RichViewPaint, RichViewSync};
pub use edit_commands::{RichDocumentCommands, RichEdit, RichEditCommands};
pub use edit_error::RichEditError;
pub use edit_history::{RichEditHistory, RichEditKind};
pub use edit_widget::{colors_for, RichMarkdownEditWidget};
pub use editor_state::{RichEditorState, RichRawLinkLookup, RichStyleBuilder, SCROLL_BAR_ROOM};
pub use format_toolbar::{
    MarkdownFormatButtonKind, MarkdownFormatButtonState, MarkdownFormatToolbar,
    MarkdownLinkRequestHandler,
};
pub use format_toolbar_widgets::{
    group_height, MarkdownFormatButton, MarkdownFormatGroup, BUTTON_DESIGN_WIDTH, ICON_DESIGN_SIZE,
};
pub use groups::shared as shared_group;
pub use groups::{
    RichAlignGroup, RichAlignGroupRef, RichListGroup, RichListGroupRef, RichQuoteGroup,
    RichQuoteGroupRef,
};
pub use image_cache::{RichImageCache, RichImageProvider, RichResolveUrl};
pub use inline::{InlineAtom, InlineAtomKind, RichInline, RichInlineContent, RichRun};
pub use inline_style::{RichInlineStyle, RichPendingStyle, RichStyleCoverage, RichStyleState};
pub use inline_writer::RichInlineContext;
pub use inlines::OffsetOutOfRange;
pub use layout_line::{RichCaret, RichCaretStop, RichLayoutFragment, RichLayoutLine};
pub use layout_style::{RichFace, RichLayoutFonts, RichLayoutStyle, RichThemeColors};
pub use link_editor::{with_scheme, MarkdownLinkEditor};
pub use raw_hosts::{RichRawHost, RichRawHosts};
pub use rich_markdown_editor::RichMarkdownEditor;
pub use selection::RichSelection;
pub use table_layout::{RichTableCellLayout, RichTableGridSnap, RichTableLayout};
