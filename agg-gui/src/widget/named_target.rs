//! [`NamedTarget`]: a named thing a widget draws without being a widget of
//! its own — a drop-down row, a menu row, a 3D control — reported through
//! [`Widget::find_named_targets`](super::Widget::find_named_targets) so GUI
//! automation can find it by name and click it.
//!
//! The port of agg-sharp's `GuiWidget.WidgetAndPosition` with a `NamedObject`
//! (`Gui/GUIWidget.cs`): C# widgets that draw named rows override
//! `FindDescendants` and add one entry per matching row, positioned at the
//! row's centre (`TextSuggestionPopup`). Here the widget reports the row's
//! rectangle and the automation crate (`agg-gui-automation`'s `tree_query`)
//! aims at its centre.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use crate::geometry::Rect;

/// One named, non-widget target inside a widget; see the module docs.
#[derive(Clone)]
pub struct NamedTarget {
    /// Where the target is drawn, in the reporting widget's local space
    /// (Y-up, from its lower-left corner). It may lie outside the widget's
    /// bounds (an open drop-down's rows hang below or above its box).
    pub rect_local: Rect,
    /// Whether the target takes input (C# `MenuItem.Enabled`).
    pub enabled: bool,
    /// The thing the name stands for (C#'s `NamedObject`), for a test to
    /// downcast: a drop-down row reports its index as `usize`.
    pub object: Option<Rc<dyn Any>>,
}

impl NamedTarget {
    /// An enabled target at `rect_local` carrying no object.
    pub fn new(rect_local: Rect) -> Self {
        Self {
            rect_local,
            enabled: true,
            object: None,
        }
    }

    /// Builder: set [`enabled`](Self::enabled).
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Builder: set [`object`](Self::object).
    pub fn with_object(mut self, object: Rc<dyn Any>) -> Self {
        self.object = Some(object);
        self
    }

    /// The object as a `T`, if it is one.
    pub fn object_as<T: 'static>(&self) -> Option<&T> {
        self.object.as_deref()?.downcast_ref::<T>()
    }
}

impl fmt::Debug for NamedTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedTarget")
            .field("rect_local", &self.rect_local)
            .field("enabled", &self.enabled)
            .field("object", &self.object.as_ref().map(|_| "dyn Any"))
            .finish()
    }
}

/// Equal when the rectangles and enabled flags match and both carry the same
/// object (the same allocation) or neither carries one.
impl PartialEq for NamedTarget {
    fn eq(&self, other: &Self) -> bool {
        self.rect_local == other.rect_local
            && self.enabled == other.enabled
            && match (&self.object, &other.object) {
                (None, None) => true,
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                _ => false,
            }
    }
}
