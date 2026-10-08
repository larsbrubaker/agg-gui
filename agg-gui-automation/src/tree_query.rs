//! Finding widgets and asking where they are: [`WidgetHandle`] (a found
//! widget that stays found across frames), lookup by name, a widget's screen
//! rectangle and its rectangle after every ancestor's clip, C#'s
//! `ActuallyVisibleOnScreen`, and the `Parents<T>()` / `Children<T>()`
//! extension methods (agg-sharp `Gui/ExtensionMethods.cs`).
//!
//! A handle wraps agg-gui's [`WidgetAnchor`]: the child-index path plus the
//! identity of each widget along it, so a handle follows its widget when a
//! parent reorders its children and reports it detached once it has left the
//! tree. Every query takes the tree's root (`App::root()`); a detached handle
//! answers `None` / `false` / empty.
//!
//! Rectangles are agg-gui's native space: logical units, Y-up, with the
//! root's lower-left corner at the window's. They are accumulated with the
//! same transform chain [`agg_gui::find_widget_screen_rect`] and the
//! inspector use (each ancestor's bounds origin, then its
//! `inspector_child_transform`). Conversion to the runner's Y-down pixel
//! search regions belongs to the name-lookup layer.

use agg_gui::{Rect, Size, TransAffine, Widget, WidgetAnchor};

/// A found widget (the Rust side of C# holding a `GuiWidget` reference).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetHandle {
    anchor: WidgetAnchor,
}

impl WidgetHandle {
    /// The widget at `path` under `root`; `None` when the path names none.
    pub fn new(root: &dyn Widget, path: &[usize]) -> Option<Self> {
        WidgetAnchor::new(root, path).map(|anchor| Self { anchor })
    }

    /// The path the handle last resolved to.
    pub fn path(&self) -> &[usize] {
        self.anchor.path()
    }

    /// The widget's current path, following reorders; `None` once detached.
    pub fn resolve(&self, root: &dyn Widget) -> Option<Vec<usize>> {
        self.anchor.resolve(root)
    }

    /// [`resolve`](Self::resolve) and remember the result, so later lookups
    /// start from it. Returns whether the widget is still attached.
    pub fn refresh(&mut self, root: &dyn Widget) -> bool {
        self.anchor.refresh(root).is_some()
    }

    /// Whether the widget is still in the tree under `root`.
    pub fn is_attached(&self, root: &dyn Widget) -> bool {
        self.resolve(root).is_some()
    }

    /// The widget itself.
    pub fn widget<'a>(&self, root: &'a dyn Widget) -> Option<&'a dyn Widget> {
        let path = self.resolve(root)?;
        agg_gui::widget::walk_path(root, &path)
    }

    /// The widget itself, mutably.
    pub fn widget_mut<'a>(&self, root: &'a mut dyn Widget) -> Option<&'a mut dyn Widget> {
        let path = self.resolve(root)?;
        agg_gui::widget::walk_path_mut(root, &path)
    }

    /// The widget as its concrete type `T` (C# `widget as T`), through
    /// [`Widget::as_any`].
    pub fn downcast<'a, T: 'static>(&self, root: &'a dyn Widget) -> Option<&'a T> {
        self.widget(root)?.as_any()?.downcast_ref::<T>()
    }

    /// The widget's name (C# `Name`, agg-gui [`Widget::id`]).
    pub fn name(&self, root: &dyn Widget) -> Option<String> {
        self.widget(root)?.id().map(str::to_string)
    }
}

/// Every widget under `root` (itself included) whose name is `name`, in paint
/// order (parents before children, earlier siblings first).
pub fn find_by_name(root: &dyn Widget, name: &str) -> Vec<WidgetHandle> {
    let mut paths = Vec::new();
    collect_named(root, name, &mut Vec::new(), &mut paths);
    paths
        .iter()
        .filter_map(|path| WidgetHandle::new(root, path))
        .collect()
}

fn collect_named(node: &dyn Widget, name: &str, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    if node.id() == Some(name) {
        out.push(path.clone());
    }
    for (i, child) in node.children().iter().enumerate() {
        path.push(i);
        collect_named(child.as_ref(), name, path, out);
        path.pop();
    }
}

/// Whether `widget` is a `T` (C#'s `OfType<T>` / `is T`).
fn is_type<T: 'static>(widget: &dyn Widget) -> bool {
    widget.as_any().is_some_and(|a| a.is::<T>())
}

/// The direct children of `handle`'s widget, in order (C#
/// `Children<GuiWidget>()`).
pub fn children(root: &dyn Widget, handle: &WidgetHandle) -> Vec<WidgetHandle> {
    children_where(root, handle, |_| true)
}

/// The direct children that are `T` (C# `Children<T>()`).
pub fn children_of_type<T: 'static>(root: &dyn Widget, handle: &WidgetHandle) -> Vec<WidgetHandle> {
    children_where(root, handle, is_type::<T>)
}

fn children_where(
    root: &dyn Widget,
    handle: &WidgetHandle,
    keep: impl Fn(&dyn Widget) -> bool,
) -> Vec<WidgetHandle> {
    let Some(path) = handle.resolve(root) else {
        return Vec::new();
    };
    let Some(widget) = agg_gui::widget::walk_path(root, &path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, child) in widget.children().iter().enumerate() {
        if keep(child.as_ref()) {
            let mut child_path = path.clone();
            child_path.push(i);
            out.extend(WidgetHandle::new(root, &child_path));
        }
    }
    out
}

/// The ancestors of `handle`'s widget, nearest first, up to and including
/// `root` (C# `Parents<GuiWidget>()`).
pub fn parents(root: &dyn Widget, handle: &WidgetHandle) -> Vec<WidgetHandle> {
    parents_where(root, handle, |_| true)
}

/// The ancestors that are `T`, nearest first (C# `Parents<T>()`).
pub fn parents_of_type<T: 'static>(root: &dyn Widget, handle: &WidgetHandle) -> Vec<WidgetHandle> {
    parents_where(root, handle, is_type::<T>)
}

fn parents_where(
    root: &dyn Widget,
    handle: &WidgetHandle,
    keep: impl Fn(&dyn Widget) -> bool,
) -> Vec<WidgetHandle> {
    let Some(path) = handle.resolve(root) else {
        return Vec::new();
    };
    (0..path.len())
        .rev()
        .map(|depth| &path[..depth])
        .filter(|p| agg_gui::widget::walk_path(root, p).is_some_and(&keep))
        .filter_map(|p| WidgetHandle::new(root, p))
        .collect()
}

/// Where a widget sits on screen, and what of it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPlacement {
    /// The widget's bounds in window space (logical, Y-up).
    pub screen_rect: Rect,
    /// `screen_rect` cut by every ancestor's `clip_children_rect` and by the
    /// viewport; empty (zero width or height) when nothing of it shows.
    pub clipped_rect: Rect,
    /// Every widget from the root down to this one reports `is_visible()`.
    pub visible_chain: bool,
}

/// Walk from `root` to `handle`'s widget, accumulating its screen rectangle
/// and the clip every ancestor imposes. `viewport` is the window's logical
/// size. `None` when the handle is detached.
pub fn placement(
    root: &dyn Widget,
    handle: &WidgetHandle,
    viewport: Size,
) -> Option<ScreenPlacement> {
    let path = handle.resolve(root)?;
    let mut node = root;
    let mut parent_to_screen = TransAffine::new();
    let mut clip = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let mut visible_chain = node.is_visible();
    for &idx in &path {
        let b = node.bounds();
        let mut local_to_screen = parent_to_screen;
        local_to_screen.translate(b.x, b.y);
        if let Some((x, y, w, h)) = node.clip_children_rect() {
            clip = intersect(
                clip,
                transform_rect_aabb(&local_to_screen, Rect::new(x, y, w, h)),
            );
        }
        local_to_screen.premultiply(&node.inspector_child_transform());
        parent_to_screen = local_to_screen;
        node = node.children().get(idx)?.as_ref();
        visible_chain &= node.is_visible();
    }
    let screen_rect = transform_rect_aabb(&parent_to_screen, node.bounds());
    Some(ScreenPlacement {
        screen_rect,
        clipped_rect: intersect(clip, screen_rect),
        visible_chain,
    })
}

/// The widget's bounds in window space (logical, Y-up).
pub fn screen_rect(root: &dyn Widget, handle: &WidgetHandle) -> Option<Rect> {
    // The viewport only affects the clipped rectangle.
    placement(root, handle, Size::new(f64::INFINITY, f64::INFINITY)).map(|p| p.screen_rect)
}

/// The part of the widget left after every ancestor's clip and the viewport.
pub fn clipped_rect(root: &dyn Widget, handle: &WidgetHandle, viewport: Size) -> Option<Rect> {
    placement(root, handle, viewport).map(|p| p.clipped_rect)
}

/// C# `ActuallyVisibleOnScreen`: the widget is still in the window, it and
/// every ancestor are visible, and some of it survives the ancestors' clips
/// and the viewport.
pub fn actually_visible_on_screen(
    root: &dyn Widget,
    handle: &WidgetHandle,
    viewport: Size,
) -> bool {
    placement(root, handle, viewport).is_some_and(|p| {
        p.visible_chain && p.clipped_rect.width > 0.0 && p.clipped_rect.height > 0.0
    })
}

/// The overlap of two rectangles; zero-sized at the nearer edge when they do
/// not overlap.
fn intersect(a: Rect, b: Rect) -> Rect {
    let left = a.left().max(b.left());
    let bottom = a.bottom().max(b.bottom());
    let right = a.right().min(b.right());
    let top = a.top().min(b.top());
    Rect::new(
        left,
        bottom,
        (right - left).max(0.0),
        (top - bottom).max(0.0),
    )
}

/// The axis-aligned box around `rect`'s corners after `t` (exact for
/// translation plus scale, as in agg-gui's inspector).
fn transform_rect_aabb(t: &TransAffine, rect: Rect) -> Rect {
    let corners = [
        (rect.left(), rect.bottom()),
        (rect.right(), rect.bottom()),
        (rect.left(), rect.top()),
        (rect.right(), rect.top()),
    ];
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (mut x, mut y) in corners {
        t.transform(&mut x, &mut y);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Rect::new(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}
