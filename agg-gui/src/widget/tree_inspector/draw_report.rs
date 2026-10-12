//! Runaway-repaint diagnostic: [`debug_draw_report`] names everything
//! currently keeping the reactive host awake.
//!
//! Part of `tree_inspector`, beside the inspector snapshot it mirrors.

use super::super::*;

// ---------------------------------------------------------------------------
// Runaway-repaint diagnostic report
// ---------------------------------------------------------------------------

/// Recursively collect every *visible* widget whose [`Widget::needs_draw`]
/// currently returns `true`, recording its child-index path, type name, and
/// whether it is "self-hot" (no visible child is independently asking for a
/// draw, so this widget's own state is the driver).
///
/// The walk descends into **all** visible children — it does not prune on a
/// parent whose `needs_draw()` is `false`.  That deliberately catches
/// propagation gaps (a hot child under a container that forgot to OR its
/// children into `needs_draw`, the exact class of bug behind the intermittent
/// runaway), which a prune-on-false walk would hide.
fn collect_needs_draw(
    widget: &dyn Widget,
    path: &mut Vec<usize>,
    out: &mut Vec<(Vec<usize>, &'static str, bool)>,
) {
    // Same visibility gate the host loop uses: invisible subtrees never keep
    // the app awake, so they are irrelevant to a runaway and are skipped.
    if !widget.is_visible() {
        return;
    }
    if widget.needs_draw() {
        let child_hot = widget.children().iter().any(|c| c.needs_draw());
        out.push((path.clone(), widget.type_name(), !child_hot));
    }
    for (i, child) in widget.children().iter().enumerate() {
        path.push(i);
        collect_needs_draw(child.as_ref(), path, out);
        path.pop();
    }
}

/// Build a human-readable diagnostic naming everything currently keeping the
/// reactive host awake, for capturing the intermittent "continuous rendering
/// never quiesces" runaway on real hardware.
///
/// The report contains, in order:
/// 1. the raw immediate-draw flag (read side-effect-free — see
///    [`crate::animation::peek_draw_signals`], which unlike `wants_draw`
///    neither pumps async wakeups nor clears a due deadline),
/// 2. the next scheduled-draw deadline with remaining time (and a `DUE`
///    marker when already past),
/// 3. the drained draw-request provenance tags, deduplicated with counts
///    (always empty in release builds — the trace is debug-only), and
/// 4. every visible widget whose `needs_draw()` is `true`, as `[path] Type`,
///    with a `<- self` marker on the ones whose own state (not a child's)
///    is the driver.
///
/// Takes the tree `root` as a parameter so it can be invoked from a host
/// shell that holds `&App` (`app.root()`), matching the introspection style
/// of [`collect_inspector_nodes`].
#[doc(hidden)]
pub fn debug_draw_report(root: &dyn Widget) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let (needs_flag, deadline) = crate::animation::peek_draw_signals();
    let now = crate::clock::now();

    let _ = writeln!(s, "== agg-gui draw report ==");
    let _ = writeln!(s, "immediate needs_draw flag: {needs_flag}");
    match deadline {
        Some(when) => {
            let remaining_ms = when.saturating_duration_since(now).as_secs_f64() * 1000.0;
            let due = now >= when;
            let _ = writeln!(
                s,
                "next scheduled deadline: {remaining_ms:.1} ms{}",
                if due { " (DUE)" } else { "" }
            );
        }
        None => {
            let _ = writeln!(s, "next scheduled deadline: none");
        }
    }

    // (c) Drained provenance tags, deduplicated with counts (most-frequent
    // first).  Draining is what the quiescence guard already does; doing it
    // here means a subsequent report starts from a clean trace.
    let tags = crate::animation::drain_draw_trace();
    if tags.is_empty() {
        let _ = writeln!(
            s,
            "draw-trace tags: none (empty in release builds — trace is debug-only)"
        );
    } else {
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for t in tags {
            if let Some(entry) = counts.iter_mut().find(|(name, _)| *name == t) {
                entry.1 += 1;
            } else {
                counts.push((t, 1));
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let _ = writeln!(s, "draw-trace tags ({} distinct):", counts.len());
        for (name, count) in counts {
            let _ = writeln!(s, "  {count:>4} x {name}");
        }
    }

    // (d) Every visible widget whose needs_draw() is true.
    let mut hot: Vec<(Vec<usize>, &'static str, bool)> = Vec::new();
    collect_needs_draw(root, &mut Vec::new(), &mut hot);
    if hot.is_empty() {
        let _ = writeln!(s, "widgets wanting draw: none");
    } else {
        let _ = writeln!(s, "widgets wanting draw ({}):", hot.len());
        for (path, type_name, self_hot) in hot {
            let path_str = if path.is_empty() {
                "root".to_string()
            } else {
                path.iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            };
            let _ = writeln!(
                s,
                "  [{path_str}] {type_name}{}",
                if self_hot { "  <- self" } else { "" }
            );
        }
    }

    s
}
