//! Shared size groups: widgets that name the same group inside a shared size
//! scope all take the width of the widest of them — label columns that line
//! up across rows (agg-sharp `Gui/LayoutEngines/SharedSizeGroup.cs`, with
//! `GuiWidget.SharedSizeGroupName` / `GuiWidget.IsSharedSizeScope`).
//!
//! A member names its group in [`WidgetBase::shared_size_group`] (builder:
//! [`Widget::with_shared_size_group`]); a scope sets
//! [`WidgetBase::shared_size_scope`] ([`Widget::with_shared_size_scope`]).
//! The scope container's `layout` starts with [`layout_as_scope`], which runs
//! the container's own layout, finds every visible member below it (not
//! looking inside nested scopes), raises each group's members to the group's
//! widest width through their minimum width, and lays the container out
//! again so the parents place the wider members. `FlexColumn`, `FlexRow`
//! (C# `LayoutEngineFlow`) and `AbsoluteLayout` (C# `LayoutEngineSimpleAlign`)
//! act as scopes; any container embedding a `WidgetBase` can by making the
//! same call.
//!
//! Divergence from agg-sharp: C# only ever raises a member's
//! `MinimumSize`, so once a group has widened, its columns never narrow
//! again — hiding the widest row or shortening its text leaves every column
//! as wide as before. Here the scope remembers each member's own minimum
//! width and restores it at the start of every scope layout, so the group
//! follows its current widest visible member both ways (pinned by
//! `rust_only_*` tests in `src/tests/shared_size_group_tests.rs`).

use crate::geometry::Size;
use crate::widget::Widget;

/// The minimum width a scope raised a member to, and the member's own
/// minimum width before that, so the next scope layout can put it back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SharedSizeRaise {
    /// The member's minimum width before the group raised it.
    pub own_min_width: f64,
    /// The minimum width the group set.
    pub applied_min_width: f64,
}

/// Lay `scope` out as a shared size scope, or return `None` when it is not
/// one (or this is the inner, plain layout pass of a scope layout).
///
/// A container calls this first thing in `layout` and returns the size it
/// gives: `if let Some(s) = layout_as_scope(self, available) { return s; }`.
/// The call re-enters the container's `layout` for the plain passes, which
/// then return `None` here and run the container's normal layout.
pub fn layout_as_scope(scope: &mut dyn Widget, available: Size) -> Option<Size> {
    let base = scope.widget_base_mut()?;
    if !base.shared_size_scope || base.shared_size_pass {
        return None;
    }
    base.shared_size_pass = true;
    restore_own_widths(scope);
    let mut size = scope.layout(available);
    if apply_shared_sizes(scope) {
        size = scope.layout(available);
    }
    if let Some(base) = scope.widget_base_mut() {
        base.shared_size_pass = false;
    }
    Some(size)
}

/// Equalise the widths of each group's visible members below `scope` to the
/// group's widest current width, by raising their minimum widths (C#
/// `SharedSizeGroup.ApplySharedSizes`). Returns `true` when any member's
/// width differed, meaning the scope must be laid out again; `false` when
/// `scope` is not a shared size scope or has no members.
pub fn apply_shared_sizes(scope: &mut dyn Widget) -> bool {
    if !is_scope(scope) {
        return false;
    }
    let mut groups: Vec<(String, Vec<Vec<usize>>)> = Vec::new();
    collect_group_members(scope, &mut Vec::new(), &mut groups);

    let mut any_changed = false;
    for (_, members) in &groups {
        let max_width = members
            .iter()
            .filter_map(|path| widget_at(scope, path))
            .fold(0.0_f64, |max, member| max.max(member.bounds().width));
        for path in members {
            let Some(member) = widget_at_mut(scope, path) else {
                continue;
            };
            if member.bounds().width == max_width {
                continue;
            }
            any_changed = true;
            // Use the minimum width so FIT members respect the shared width.
            if let Some(base) = member.widget_base_mut() {
                if base.min_size.width < max_width {
                    let own_min_width = base
                        .shared_size_raise
                        .map_or(base.min_size.width, |raise| raise.own_min_width);
                    base.min_size.width = max_width;
                    base.shared_size_raise = Some(SharedSizeRaise {
                        own_min_width,
                        applied_min_width: max_width,
                    });
                }
            }
        }
    }
    any_changed
}

fn is_scope(widget: &dyn Widget) -> bool {
    widget.widget_base().is_some_and(|b| b.shared_size_scope)
}

/// Gather the visible members under `parent` by group, in tree order,
/// stopping at nested scopes (a nested scope may itself be a member).
fn collect_group_members(
    parent: &dyn Widget,
    path: &mut Vec<usize>,
    groups: &mut Vec<(String, Vec<Vec<usize>>)>,
) {
    for (i, child) in parent.children().iter().enumerate() {
        if !child.is_visible() {
            continue;
        }
        path.push(i);
        if let Some(name) = child
            .widget_base()
            .and_then(|b| b.shared_size_group.as_deref())
            .filter(|name| !name.is_empty())
        {
            match groups.iter_mut().find(|(group, _)| group == name) {
                Some((_, members)) => members.push(path.clone()),
                None => groups.push((name.to_string(), vec![path.clone()])),
            }
        }
        if !is_scope(child.as_ref()) {
            collect_group_members(child.as_ref(), path, groups);
        }
        path.pop();
    }
}

/// Put back the minimum width of every member (visible or not) the scope
/// raised, unless something else has set a different minimum since.
fn restore_own_widths(parent: &mut dyn Widget) {
    for child in parent.children_mut().iter_mut() {
        if let Some(base) = child.widget_base_mut() {
            if let Some(raise) = base.shared_size_raise.take() {
                if base.min_size.width == raise.applied_min_width {
                    base.min_size.width = raise.own_min_width;
                }
            }
        }
        if !is_scope(child.as_ref()) {
            restore_own_widths(child.as_mut());
        }
    }
}

fn widget_at<'a>(root: &'a dyn Widget, path: &[usize]) -> Option<&'a dyn Widget> {
    let mut node = root;
    for &i in path {
        node = node.children().get(i)?.as_ref();
    }
    Some(node)
}

fn widget_at_mut<'a>(root: &'a mut dyn Widget, path: &[usize]) -> Option<&'a mut dyn Widget> {
    let mut node = root;
    for &i in path {
        node = node.children_mut().get_mut(i)?.as_mut();
    }
    Some(node)
}
