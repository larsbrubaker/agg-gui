//! `rich_text` — a styled rich-text document model, command engine, layout
//! engine, and read-only view widget.
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
//!   line/fragment geometry ([`DocLayout`]).
//! * [`view`] — [`RichTextView`], the phase-1 read-only display widget.
//!
//! # Phase plan
//!
//! Phase 1 lands the model, command engine, layout, and a read-only view.
//! Phase 2 adds the interactive editor: a caret + selection, keyboard handling
//! built on the structural edits here, undo/redo via the existing
//! [`crate::undo`] stack, a formatting toolbar driven by [`range_common_style`],
//! and a demo window.

pub mod commands;
pub mod layout;
pub mod model;
pub mod view;

pub use commands::{apply_command, range_common_style, style_at, CommonStyle, RichCommand, MAX_INDENT};
pub use layout::{BlockLayout, DocLayout, FontResolver, LineFragment, LineLayout};
pub use model::{
    insert_text, merge_block_with_prev, remove_range, split_block, Block, DocPos, DocRange,
    InlineStyle, ListKind, RichDoc, TextRun,
};
pub use view::{uniform_resolver, RichTextView, SharedResolver};
