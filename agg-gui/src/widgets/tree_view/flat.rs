//! Cached flat-row engine for `TreeView`: the child index and the list of
//! visible rows, rebuilt only when the tree's structure or expansion changes.
//!
//! `TreeView::nodes` is a public `Vec`, so callers may edit `parent`,
//! `order`, `is_expanded` (or push / clear nodes) directly.  Rather than
//! trusting every caller to report edits, [`FlatCache::refresh`] keeps a
//! compact snapshot ([`NodeKey`], 16 bytes per node) of exactly the fields
//! that shape the visible list and compares it in one linear pass — no
//! string hashing.  On a structural change (parent / order / node count) the
//! child index is rebuilt (O(n log n)); on an expansion change only the flat
//! rows are rebuilt, in O(visible rows) by walking the child index.  When
//! nothing changed, nothing is rebuilt.
//!
//! Consumed by `mod.rs` (keyboard and selection helpers), `widget_impl.rs`
//! (layout / paint) and `api.rs` (programmatic scrolling and reveal).

use super::node::{FlatRow, TreeNode};

/// The fields of one node that decide which rows are visible, and in what
/// order.  `parent` is `usize::MAX` for root nodes.
#[derive(Clone, Copy, PartialEq, Eq)]
struct NodeKey {
    parent: usize,
    order: u32,
    expanded: bool,
    may_have_children: bool,
}

impl NodeKey {
    fn of(n: &TreeNode) -> Self {
        Self {
            parent: n.parent.unwrap_or(usize::MAX),
            order: n.order,
            expanded: n.is_expanded,
            may_have_children: n.may_have_children,
        }
    }
}

/// Child index plus the cached visible rows (see the module docs).
#[derive(Default)]
pub(super) struct FlatCache {
    /// Snapshot the cache was built from, parallel to `TreeView::nodes`.
    keys: Vec<NodeKey>,
    /// Root nodes, sorted by `order` (ties keep index order).
    roots: Vec<usize>,
    /// CSR child lists: the children of node `i` are
    /// `child_list[child_start[i]..child_start[i + 1]]`, sorted by `order`.
    child_start: Vec<usize>,
    child_list: Vec<usize>,
    /// Visible rows in display order (depth first, siblings by `order`).
    pub rows: Vec<FlatRow>,
    /// `false` forces a full rebuild on the next `refresh`.
    valid: bool,
}

impl FlatCache {
    /// Drop everything; the next [`refresh`](Self::refresh) rebuilds.
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Bring the cache up to date with `nodes`.  Returns `true` when the
    /// visible rows were rebuilt.
    pub fn refresh(&mut self, nodes: &[TreeNode]) -> bool {
        let (structure, expansion) = if !self.valid || self.keys.len() != nodes.len() {
            self.keys = nodes.iter().map(NodeKey::of).collect();
            (true, true)
        } else {
            let mut structure = false;
            let mut expansion = false;
            for (key, node) in self.keys.iter_mut().zip(nodes) {
                let now = NodeKey::of(node);
                if *key != now {
                    structure |= key.parent != now.parent || key.order != now.order;
                    expansion |= key.expanded != now.expanded
                        || key.may_have_children != now.may_have_children;
                    *key = now;
                }
            }
            (structure, expansion)
        };
        if structure {
            self.rebuild_index(nodes);
        }
        if structure || expansion {
            self.rebuild_rows();
            self.valid = true;
            return true;
        }
        false
    }

    /// Children of `idx`, sorted by `order` (empty for out-of-range ids).
    /// Valid as of the last [`refresh`](Self::refresh).
    pub fn children(&self, idx: usize) -> &[usize] {
        match (self.child_start.get(idx), self.child_start.get(idx + 1)) {
            (Some(&a), Some(&b)) => &self.child_list[a..b],
            _ => &[],
        }
    }

    /// Display position of `node_idx` among the visible rows, if visible.
    pub fn position_of(&self, node_idx: usize) -> Option<usize> {
        self.rows.iter().position(|r| r.node_idx == node_idx)
    }

    /// Rebuild the root list and the CSR child index from `self.keys`.
    fn rebuild_index(&mut self, nodes: &[TreeNode]) {
        let n = self.keys.len();
        let mut counts = vec![0usize; n + 1];
        self.roots.clear();
        for (i, key) in self.keys.iter().enumerate() {
            if key.parent == usize::MAX {
                self.roots.push(i);
            } else if key.parent < n {
                counts[key.parent + 1] += 1;
            }
            // A parent index past the end matches no node: the node is
            // unreachable, exactly as before the cache existed.
        }
        for i in 0..n {
            counts[i + 1] += counts[i];
        }
        self.child_start = counts.clone();
        self.child_list = vec![0; counts[n]];
        let mut fill = counts;
        for (i, key) in self.keys.iter().enumerate() {
            if key.parent != usize::MAX && key.parent < n {
                self.child_list[fill[key.parent]] = i;
                fill[key.parent] += 1;
            }
        }
        // Stable sorts: siblings with equal `order` keep index order.
        self.roots.sort_by_key(|&i| nodes[i].order);
        for p in 0..n {
            let (a, b) = (self.child_start[p], self.child_start[p + 1]);
            if b - a > 1 {
                self.child_list[a..b].sort_by_key(|&i| nodes[i].order);
            }
        }
    }

    /// Depth-first walk of the expanded part of the tree — O(visible rows).
    fn rebuild_rows(&mut self) {
        self.rows.clear();
        let mut stack: Vec<(usize, u32)> = self.roots.iter().rev().map(|&i| (i, 0)).collect();
        while let Some((idx, depth)) = stack.pop() {
            let (a, b) = (self.child_start[idx], self.child_start[idx + 1]);
            let kids = &self.child_list[a..b];
            let key = self.keys[idx];
            self.rows.push(FlatRow {
                node_idx: idx,
                depth,
                has_children: !kids.is_empty() || key.may_have_children,
            });
            if key.expanded {
                stack.extend(kids.iter().rev().map(|&c| (c, depth + 1)));
            }
        }
    }
}
