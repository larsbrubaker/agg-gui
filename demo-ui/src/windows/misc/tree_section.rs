//! Tree section of the Misc Demos window — port of egui's `Tree` demo.
//!
//! A recursive collapsing tree where every node carries a "+" button that adds
//! a child and (below the root) a "delete" button that removes the node from
//! its parent.  The mutable model lives in an `Rc<RefCell<Node>>`; a `Rebuilder`
//! (keyed on a version counter) regenerates the `CollapsingHeader` subtree
//! whenever the structure changes, since agg-gui trees are otherwise built once.
//!
//! Consumed by `misc_demos.rs`, which mounts this section inside its outer
//! collapsing list.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{Button, CollapsingHeader, FlexColumn, Font, Rebuilder, Widget};

/// A tree node: just its children (matching egui's `Tree(Vec<Self>)`).
#[derive(Default, Clone)]
struct Node {
    children: Vec<Node>,
}

impl Node {
    /// The same starting shape egui seeds its demo tree with.
    fn demo() -> Node {
        Node {
            children: vec![
                Node {
                    children: vec![Node::default(); 4],
                },
                Node {
                    children: vec![
                        Node {
                            children: vec![Node::default(); 2],
                        };
                        3
                    ],
                },
            ],
        }
    }

    fn at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut cur = self;
        for &i in path {
            cur = cur.children.get_mut(i)?;
        }
        Some(cur)
    }
}

/// Append a fresh child to the node addressed by `path`.
fn add_child(root: &Rc<RefCell<Node>>, path: &[usize]) {
    if let Some(node) = root.borrow_mut().at_mut(path) {
        node.children.push(Node::default());
    }
}

/// Remove the node addressed by `path` from its parent (no-op at the root).
fn delete_node(root: &Rc<RefCell<Node>>, path: &[usize]) {
    let Some((&last, parent_path)) = path.split_last() else {
        return;
    };
    if let Some(parent) = root.borrow_mut().at_mut(parent_path) {
        if last < parent.children.len() {
            parent.children.remove(last);
        }
    }
}

fn build_node(
    node: &Node,
    path: Vec<usize>,
    depth: usize,
    name: String,
    font: &Arc<Font>,
    root: &Rc<RefCell<Node>>,
    version: &Rc<Cell<u64>>,
) -> Box<dyn Widget> {
    let mut content = FlexColumn::new().with_gap(2.0).with_padding(2.0);

    // Non-root nodes can delete themselves; egui colours this red.
    if depth > 0 {
        let root = Rc::clone(root);
        let version = Rc::clone(version);
        let p = path.clone();
        content.push(
            Box::new(
                Button::new("delete", Arc::clone(font))
                    .with_font_size(11.0)
                    .on_click(move || {
                        delete_node(&root, &p);
                        version.set(version.get() + 1);
                    }),
            ),
            0.0,
        );
    }

    for (i, child) in node.children.iter().enumerate() {
        let mut child_path = path.clone();
        child_path.push(i);
        content.push(
            build_node(
                child,
                child_path,
                depth + 1,
                format!("child #{i}"),
                font,
                root,
                version,
            ),
            0.0,
        );
    }

    // Every node can grow a new child.
    {
        let root = Rc::clone(root);
        let version = Rc::clone(version);
        let p = path.clone();
        content.push(
            Box::new(
                Button::new("+", Arc::clone(font))
                    .with_font_size(11.0)
                    .on_click(move || {
                        add_child(&root, &p);
                        version.set(version.get() + 1);
                    }),
            ),
            0.0,
        );
    }

    Box::new(
        CollapsingHeader::new(name, Arc::clone(font))
            .default_open(depth < 1)
            .with_content(Box::new(content)),
    )
}

/// Build the Tree section content: a `Rebuilder` wrapping the recursive
/// collapsing tree.
///
/// Note: because the whole subtree is regenerated on each structural change,
/// per-node expand/collapse state resets to its default when a node is added or
/// deleted (agg-gui's `CollapsingHeader` keeps its open flag internally rather
/// than persisting it by id).
pub fn tree_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let root = Rc::new(RefCell::new(Node::demo()));
    let version = Rc::new(Cell::new(0_u64));

    let version_fn = {
        let version = Rc::clone(&version);
        move || version.get()
    };
    let builder = {
        let font = Arc::clone(font);
        let root = Rc::clone(&root);
        let version = Rc::clone(&version);
        move || {
            let node = root.borrow();
            build_node(
                &node,
                Vec::new(),
                0,
                "root".to_string(),
                &font,
                &root,
                &version,
            )
        }
    };

    Box::new(Rebuilder::new(version_fn, builder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_delete_mutate_by_path() {
        let root = Rc::new(RefCell::new(Node::demo()));
        // root has 2 children initially.
        assert_eq!(root.borrow().children.len(), 2);

        add_child(&root, &[]);
        assert_eq!(root.borrow().children.len(), 3);

        // child #0 starts with 4 grandchildren.
        assert_eq!(root.borrow().children[0].children.len(), 4);
        add_child(&root, &[0]);
        assert_eq!(root.borrow().children[0].children.len(), 5);

        // Delete grandchild [0,2].
        delete_node(&root, &[0, 2]);
        assert_eq!(root.borrow().children[0].children.len(), 4);

        // Deleting the root path is a no-op.
        delete_node(&root, &[]);
        assert_eq!(root.borrow().children.len(), 3);
    }
}
