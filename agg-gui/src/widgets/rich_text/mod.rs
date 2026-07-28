//! `rich_text` — a styled rich-text document model, command engine, layout
//! engine, and the widgets ([`RichTextView`], [`RichTextEdit`]) that render and
//! edit it.  This is the toolkit for **embedding a real text editor in your
//! agg-gui app**, not a fixed demo widget: you own the document, supply the
//! fonts, and drive formatting through commands.
//!
//! # Module layout
//!
//! * [`model`] — the document types ([`RichDoc`], [`Block`], [`TextRun`],
//!   [`InlineStyle`], [`ListKind`]), positions/ranges ([`DocPos`], [`DocRange`]),
//!   and the structural edit primitives (insert / remove / split / merge /
//!   normalize).
//! * [`commands`] — [`RichCommand`] plus [`apply_command`], with toolbar-state
//!   helpers ([`style_at`], [`range_common_style`], [`CommonStyle`]).
//! * [`layout`] — width-constrained, per-run-font layout producing paint-ready
//!   line/fragment geometry ([`DocLayout`]).  Catalog-agnostic: it asks a
//!   *resolver* ([`SharedResolver`]) to map each run's style to a
//!   [`Font`](crate::text::Font).
//! * [`view`] — [`RichTextView`], a read-only display widget, plus the
//!   [`single_font_resolver`] helper.
//! * [`editor`] — [`RichTextEdit`], the interactive editor, and the cheap
//!   [`RichEditHandle`] a formatting toolbar drives it through.
//!
//! # Embedding an editor
//!
//! Everything a functional editor needs is public here.  A resolver maps run
//! styles to concrete fonts (see [`single_font_resolver`] for the full
//! contract); [`RichTextEdit::handle`] hands a toolbar a cheap, cloneable driver
//! that shares the very editor being rendered.
//!
//! ```
//! use std::sync::Arc;
//! use agg_gui::Font;
//! use agg_gui::widgets::rich_text::{
//!     single_font_resolver, Block, InlineStyle, RichCommand, RichDoc, RichTextEdit, TextRun,
//! };
//!
//! // 1. Load a font. Any `Arc<Font>` works; here we read the crate's bundled
//! //    face so this example is a real, runnable doc-test.
//! let bytes = std::fs::read(concat!(
//!     env!("CARGO_MANIFEST_DIR"),
//!     "/assets/fonts/NotoSans-Regular.ttf",
//! ))
//! .expect("bundled font readable");
//! let font = Arc::new(Font::from_slice(&bytes).expect("valid font"));
//!
//! // 2. Build a document from styled runs (or `RichDoc::new()` for a blank one).
//! let bold = InlineStyle { bold: true, ..Default::default() };
//! let doc = RichDoc::from_blocks(vec![Block {
//!     runs: vec![
//!         TextRun::new("Hello, ", InlineStyle::default()),
//!         TextRun::new("world", bold),
//!     ],
//!     ..Block::new()
//! }]);
//!
//! // 3. One font -> a working resolver -> a functional editor in one line each.
//! let resolver = single_font_resolver(font);
//! let mut editor = RichTextEdit::new(doc, resolver).with_font_size(18.0);
//!
//! // 4. A cheap handle drives the same editor from a toolbar (clone freely).
//! let toolbar = editor.handle();
//!
//! // 5. Apply formatting commands (a real selection comes from mouse/keyboard
//! //    input dispatched to `editor.on_event`; a collapsed caret arms the format
//! //    for the next typed run).
//! toolbar.exec(&RichCommand::ToggleItalic);
//!
//! // 6. Read toolbar state: what styles the current selection shares.
//! let style = editor.common_style_of_selection();
//! let _ = style.bold; // Option<bool>: Some(v) = all agree, None = mixed.
//!
//! // 7. Undo / redo flow through the shared core (also on the handle).
//! if editor.can_undo() {
//!     editor.undo();
//! }
//! toolbar.redo();
//!
//! assert!(editor.plain_text().starts_with("Hello"));
//! ```
//!
//! To *display* a document without editing, use [`RichTextView`] the same way.
//! For a multi-font resolver with real bold/italic faces backed by a system-font
//! catalog, see `make_resolver` in `demo-ui/src/windows/rich_text_demo`.
//!
//! # Asynchronous font loading
//!
//! The editor caches its layout against `(width, document revision)` and does
//! **not** observe an app's font catalog.  If your app loads faces
//! asynchronously, call [`RichTextEdit::invalidate_layout`] when a newly-arrived
//! font should be picked up (e.g. on each advance of your font-cache epoch), so
//! the doc reshapes and the cached backbuffer re-rasters.

pub mod commands;
pub mod editor;
pub mod layout;
pub mod model;
pub mod rich_clipboard;
pub mod toolbar;
pub mod view;

pub use commands::{
    apply_command, range_common_style, style_at, CommonStyle, RichCommand, MAX_INDENT,
};
pub use editor::{RichEditCore, RichEditHandle, RichTextEdit};
pub use layout::{
    layout_doc, BlockLayout, DocLayout, FontResolver, LineFragment, LineLayout, INDENT_PX,
    LINE_SPACING, LIST_GUTTER_PX, MARKER_GAP_PX,
};
pub use model::{
    insert_text, merge_block_with_prev, remove_range, split_block, Block, DocPos, DocRange,
    InlineStyle, ListKind, RichDoc, TextRun,
};
pub use toolbar::{RichTextToolbar, Variant};
pub use view::{single_font_resolver, uniform_resolver, RichTextView, SharedResolver};
