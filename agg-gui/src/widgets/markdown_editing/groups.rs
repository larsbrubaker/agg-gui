//! Markdown that spans several blocks: one `<div align>` wrapper, one
//! blockquote or one top-level list.  Ports agg-sharp's `RichBlockGroups.cs`.
//!
//! A single group is shared by reference among its member blocks
//! (`Rc<RefCell<_>>`, as C#'s shared class instance), so membership is explicit
//! rather than inferred from separators (two adjacent quotes or lists stay two
//! groups).  Cloning a [`super::RichBlock`] keeps the reference;
//! [`super::RichDocument::deep_clone`] clones each group once and remaps its
//! members.
//!
//! Writer rule: a group writes its original bytes (member separators and
//! sources, plus an align group's wrapper) only while its members are still one
//! contiguous run of exactly `original_member_count` blocks, all clean (and,
//! for an align group, all still at the group's `alignment`).  Otherwise the
//! whole group is regenerated — wrapper, list indentation from depth, and the
//! separators inside it — with at least one blank line between it and
//! neighbours of another kind.

use std::cell::RefCell;
use std::rc::Rc;

use super::block::RichAlignment;

/// A `<div align>` wrapper around paragraphs and headings.  A "left" wrapper is
/// a group too, so an untouched one round-trips; blocks outside any group are
/// simply Left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichAlignGroup {
    /// How many blocks the group had when parsed; a different count means an
    /// edit added or removed members.  0 for a group created by an edit, which
    /// is never written from original bytes.
    pub original_member_count: usize,
    /// The wrapper's alignment as parsed.  A member whose alignment differs has
    /// been re-aligned, so the group no longer writes its original bytes.
    pub alignment: RichAlignment,
    /// From the `<div align="...">` tag through the blank line(s) before the
    /// first member's source.  Written after the first member's
    /// `separator_before`, so separators stay whitespace only.
    pub open_source: String,
    /// From the blank line(s) after the last member's source through `</div>`;
    /// the line break after the tag is the next block's separator.
    pub close_source: String,
    /// For a one-line element (`<h2 align="center">Title</h2>`), the whole line
    /// as parsed; `None` for a wrapper.  The member's own `original_source`
    /// stays its markdown (`## Title`), so a clean member written without the
    /// group still reads back as what it is; this line replaces it only while
    /// the group is intact.
    pub one_line_source: Option<String>,
}

/// One blockquote; each member Quote block is one of its paragraphs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichQuoteGroup {
    /// See [`RichAlignGroup::original_member_count`].
    pub original_member_count: usize,
}

/// One top-level list; its nested items (any depth) belong to the same group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichListGroup {
    /// See [`RichAlignGroup::original_member_count`].
    pub original_member_count: usize,
}

/// A shared reference to an align group (one instance per wrapper).
pub type RichAlignGroupRef = Rc<RefCell<RichAlignGroup>>;
/// A shared reference to a quote group (one instance per blockquote).
pub type RichQuoteGroupRef = Rc<RefCell<RichQuoteGroup>>;
/// A shared reference to a list group (one instance per top-level list).
pub type RichListGroupRef = Rc<RefCell<RichListGroup>>;

/// Wraps a new group for sharing among its members.
pub fn shared<T>(group: T) -> Rc<RefCell<T>> {
    Rc::new(RefCell::new(group))
}

/// Clones each distinct group (by identity) once, so copies keep grouping
/// exactly as the originals while sharing no state with them: the `Remap`
/// helper of `RichDocument.Clone`.
#[derive(Default)]
pub(super) struct GroupRemap<T: Clone> {
    /// (original, copy) pairs, matched by identity.
    pairs: Vec<GroupPair<T>>,
}

type GroupPair<T> = (Rc<RefCell<T>>, Rc<RefCell<T>>);

impl<T: Clone> GroupRemap<T> {
    pub(super) fn new() -> Self {
        Self { pairs: Vec::new() }
    }

    pub(super) fn remap(&mut self, group: &Option<Rc<RefCell<T>>>) -> Option<Rc<RefCell<T>>> {
        let group = group.as_ref()?;
        if let Some((_, copy)) = self.pairs.iter().find(|(o, _)| Rc::ptr_eq(o, group)) {
            return Some(Rc::clone(copy));
        }
        let copy = shared(group.borrow().clone());
        self.pairs.push((Rc::clone(group), Rc::clone(&copy)));
        Some(copy)
    }
}
