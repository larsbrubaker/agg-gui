//! Finding widgets and asking where they are: [`WidgetHandle`] (a found
//! widget that stays found across frames), lookup by name, a widget's screen
//! rectangle and its rectangle after every ancestor's clip, C#'s
//! `ActuallyVisibleOnScreen`, C#'s inherited `Enabled`, [`NamedHit`] (C#'s
//! `GetByNameResults`), and the `Parents<T>()` / `Children<T>()` extension
//! methods (agg-sharp `Gui/ExtensionMethods.cs`).
//!
//! Name lookups ([`find_named_hits`]) also find the non-widget targets a
//! widget reports through [`Widget::find_named_targets`] (C#'s
//! `FindDescendants` overrides that add a `NamedObject`): a hit then carries
//! the [`NamedTarget`] and aims at the centre of its rectangle.
//!
//! A handle wraps agg-gui's [`WidgetAnchor`]: the child-index path plus the
//! identity of each widget along it, so a handle follows its widget when a
//! parent reorders its children and reports it detached once it has left the
//! tree. Every query takes the tree's root (`App::root()`); a detached handle
//! answers `None` / `false` / empty.
//!
//! Rectangles are agg-gui's native space: logical units, Y-up, with the
//! root's lower-left corner at the window's. They follow painting's
//! transform and clip chain ([`crate::paint_geometry`]), so a widget is where
//! the frame draws it, and on show where the frame lets it show. Conversion
//! to the runner's Y-down pixel search regions belongs to the name-lookup
//! layer; choosing among same-named widgets is [`crate::name_pick`]'s.

use agg_gui::{NamedTarget, Point, Rect, Size, Widget, WidgetAnchor};

use crate::paint_geometry;

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

/// A widget, or a named target a widget draws, found by name — C#'s
/// `GetByNameResults`.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedHit {
    /// The widget that carries the name, or that reported `target`.
    pub handle: WidgetHandle,
    /// Where in the widget a click should land, in its local space (Y-up,
    /// from its lower-left corner): its centre (C# `Position`, `Width / 2,
    /// Height / 2`), or the centre of `target`'s rectangle.
    pub offset_hint: Point,
    /// The non-widget target the name belongs to (C#'s `NamedObject` when it
    /// is not the widget itself); `None` when the widget carries the name.
    pub target: Option<NamedTarget>,
}

impl NamedHit {
    /// A hit on the widget itself, aimed at the centre of its `bounds`.
    pub fn widget(handle: WidgetHandle, bounds: Rect) -> Self {
        Self {
            handle,
            offset_hint: Point::new(bounds.width / 2.0, bounds.height / 2.0),
            target: None,
        }
    }

    /// A hit on `target`, reported by `handle`'s widget, aimed at the centre
    /// of its rectangle.
    pub fn target(handle: WidgetHandle, target: NamedTarget) -> Self {
        let r = target.rect_local;
        Self {
            handle,
            offset_hint: Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0),
            target: Some(target),
        }
    }

    /// Whether the name is enabled: the target's own flag for a target (its
    /// widget's chain is asked separately, [`actually_enabled`]); true for a
    /// widget.
    pub fn target_enabled(&self) -> bool {
        self.target.as_ref().is_none_or(|t| t.enabled)
    }
}

/// Every widget and every named target under `root` (itself included) named
/// `name`, in paint order (parents before children, earlier siblings first).
/// A widget's own targets come before the widget, as C#'s overrides add them
/// before calling the base `FindDescendants`.
pub fn find_named_hits(root: &dyn Widget, name: &str) -> Vec<NamedHit> {
    let mut found = Vec::new();
    collect_hits(root, name, &mut Vec::new(), &mut found);
    found
        .into_iter()
        .filter_map(|(path, target)| {
            let handle = WidgetHandle::new(root, &path)?;
            Some(match target {
                Some(target) => NamedHit::target(handle, target),
                None => {
                    let bounds = handle.widget(root)?.bounds();
                    NamedHit::widget(handle, bounds)
                }
            })
        })
        .collect()
}

type FoundName = (Vec<usize>, Option<NamedTarget>);

fn collect_hits(node: &dyn Widget, name: &str, path: &mut Vec<usize>, out: &mut Vec<FoundName>) {
    let mut targets = Vec::new();
    node.find_named_targets(name, &mut targets);
    out.extend(targets.into_iter().map(|t| (path.clone(), Some(t))));
    if node.id() == Some(name) {
        out.push((path.clone(), None));
    }
    for (i, child) in node.children().iter().enumerate() {
        path.push(i);
        collect_hits(child.as_ref(), name, path, out);
        path.pop();
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
    /// `screen_rect` cut by every clip painting puts it under (see
    /// [`placement`]) and by the viewport; empty (zero width or height) when nothing of it shows.
    pub clipped_rect: Rect,
    /// Every widget from the root down to this one reports `is_visible()`.
    pub visible_chain: bool,
}

/// Walk from `root` to `handle`'s widget, replaying painting's transforms
/// and clips ([`paint_geometry`]): pixel snapping, every ancestor's
/// `clip_children_rect` and `child_paint_clip`, and the window alone for a
/// subtree painted in the overlay pass. `viewport` is the window's logical
/// size. `None` when the handle is detached.
pub fn placement(
    root: &dyn Widget,
    handle: &WidgetHandle,
    viewport: Size,
) -> Option<ScreenPlacement> {
    let path = handle.resolve(root)?;
    let widget = agg_gui::widget::walk_path(root, &path)?;
    let painted = paint_geometry::paint_placement(root, &path, viewport)?;
    let b = widget.bounds();
    let screen_rect = painted.screen_rect(Size::new(b.width, b.height));
    Some(ScreenPlacement {
        screen_rect,
        clipped_rect: intersect(painted.clip, screen_rect),
        visible_chain: painted.visible_chain,
    })
}

/// The widget's bounds in window space (logical, Y-up).
pub fn screen_rect(root: &dyn Widget, handle: &WidgetHandle) -> Option<Rect> {
    // The viewport only affects the clipped rectangle.
    placement(root, handle, Size::new(f64::INFINITY, f64::INFINITY)).map(|p| p.screen_rect)
}

/// `rect_local` (in `handle`'s widget's local space) in window space
/// (logical, Y-up): where a [`NamedTarget`] sits on screen.
pub fn local_rect_to_screen(
    root: &dyn Widget,
    handle: &WidgetHandle,
    rect_local: Rect,
) -> Option<Rect> {
    let path = handle.resolve(root)?;
    // The viewport only affects the clip.
    let unbounded = Size::new(f64::INFINITY, f64::INFINITY);
    let painted = paint_geometry::paint_placement(root, &path, unbounded)?;
    Some(paint_geometry::screen_aabb(
        &painted.local_to_screen,
        rect_local,
    ))
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

/// C#'s `Enabled`: the widget and every ancestor report
/// [`Widget::is_enabled`] (a disabled parent disables its children). False
/// once the handle is detached.
pub fn actually_enabled(root: &dyn Widget, handle: &WidgetHandle) -> bool {
    let Some(path) = handle.resolve(root) else {
        return false;
    };
    let mut node = root;
    let mut enabled = node.is_enabled();
    for &idx in &path {
        match node.children().get(idx) {
            Some(child) => node = child.as_ref(),
            None => return false,
        }
        enabled &= node.is_enabled();
    }
    enabled
}

pub use crate::paint_geometry::intersect;
