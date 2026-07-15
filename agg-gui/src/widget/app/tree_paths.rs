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
///
/// Subtrees that are not visible, or that sit under a widget which
/// [`Widget::blocks_child_interaction`] (a disabled scope), are skipped
/// entirely: keyboard focus must not reach a widget the user cannot see or
/// interact with — matching egui's `UiBuilder::disabled()`/`invisible()`.
pub(super) fn collect_focusable(
    widget: &dyn Widget,
    current_path: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
    if !widget.is_visible() || widget.blocks_child_interaction() {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::geometry::{Rect, Size};

    /// A leaf widget that reports itself focusable (stands in for a TextField).
    struct Focusable {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for Focusable {
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _e: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn is_focusable(&self) -> bool {
            true
        }
    }

    /// A scope wrapping one focusable child, mirroring the demo's `GalleryScope`
    /// Visible / Interactive controls via `is_visible` / `blocks_child_interaction`.
    struct Scope {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        visible: bool,
        blocked: bool,
    }
    impl Widget for Scope {
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _e: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn is_visible(&self) -> bool {
            self.visible
        }
        fn blocks_child_interaction(&self) -> bool {
            self.blocked
        }
    }

    fn scope(visible: bool, blocked: bool) -> Scope {
        Scope {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            children: vec![Box::new(Focusable {
                bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
                children: Vec::new(),
            })],
            visible,
            blocked,
        }
    }

    fn focusable_paths(root: &dyn Widget) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        collect_focusable(root, &mut Vec::new(), &mut out);
        out
    }

    #[test]
    fn interactive_scope_exposes_child_to_focus() {
        let root = scope(true, false);
        assert_eq!(focusable_paths(&root), vec![vec![0]]);
    }

    #[test]
    fn non_interactive_scope_hides_child_from_focus() {
        // Interactive off (blocks_child_interaction): Tab must not reach the field.
        let root = scope(true, true);
        assert!(focusable_paths(&root).is_empty());
    }

    #[test]
    fn invisible_scope_hides_child_from_focus() {
        // Visible off: the field is neither painted nor focusable.
        let root = scope(false, false);
        assert!(focusable_paths(&root).is_empty());
    }
}
