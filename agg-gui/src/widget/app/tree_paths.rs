//! Index-path helpers over the widget tree.
//!
//! Carved out of `app.rs` so that file stays under the workspace 800-line
//! cap. These walk the `App` root by `Vec<usize>` child-index paths: collect
//! every focusable widget in paint order, and resolve a path to a (mutable or
//! shared) widget reference. Used by `App`'s focus, hit-test, and
//! bring-to-front logic.

use crate::widget::Widget;

/// Collect all focusable widgets in paint order (DFS root → leaves).
/// Returns their paths as `Vec<Vec<usize>>`.
pub(super) fn collect_focusable(
    widget: &dyn Widget,
    current_path: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
    if widget.is_focusable() {
        out.push(current_path.clone());
    }
    for (i, child) in widget.children().iter().enumerate() {
        current_path.push(i);
        collect_focusable(child.as_ref(), current_path, out);
        current_path.pop();
    }
}

/// Get a mutable reference to the widget at the given path.
pub(super) fn widget_at_path<'a>(
    root: &'a mut Box<dyn Widget>,
    path: &[usize],
) -> &'a mut dyn Widget {
    if path.is_empty() {
        return root.as_mut();
    }
    let idx = path[0];
    widget_at_path(&mut root.children_mut()[idx], &path[1..])
}

pub(super) fn widget_at_path_ref<'a>(root: &'a dyn Widget, path: &[usize]) -> &'a dyn Widget {
    if path.is_empty() {
        return root;
    }
    let idx = path[0];
    widget_at_path_ref(root.children()[idx].as_ref(), &path[1..])
}
