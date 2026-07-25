// Pointer / keyboard event handling for `Window`.  Lifted out of
// `widget_impl.rs` so that file stays under the 800-line limit enforced
// by `tests/file_line_count.rs`, following the same shape as the
// sibling `paint.rs`: each helper takes `&mut Window` instead of
// `&mut self` so the trait `impl Widget for Window` stays a thin
// dispatcher in `widget_impl.rs`.
//
// This module owns the whole interactive surface of a window: title-bar
// drag and double-click maximise, the close / maximise chrome buttons,
// resize-edge grabs (whose geometry lives in `window.rs` via
// `resize_dir` / `apply_resize`, and whose snapping lives in
// `snap_glue.rs`), the modal click-away and Escape dismissals routed
// through `close.rs`, and the frameless (`chrome == false`) fast path
// that forwards everything to the content child.

use super::*;

impl Window {
    /// Whether `local` (window-local, Y-up) lies within this window's bounds.
    /// Used by the click-away path to tell an inside press from an outside one
    /// once the modal grab has routed it here regardless of position.
    fn point_in_local_bounds(&self, local: Point) -> bool {
        local.x >= 0.0
            && local.x <= self.bounds.width
            && local.y >= 0.0
            && local.y <= self.bounds.height
    }
}

/// Body of `<Window as Widget>::on_event`.
pub(super) fn on_event(window: &mut Window, event: &Event) -> EventResult {
    if !window.requested_visible() {
        return EventResult::Ignored;
    }

    // Frameless windows are thin hosts for an inline editor (a chrome-less
    // TextField sized to a value pill). They have no title bar, resize
    // handles, or chrome buttons, so — apart from the modal click-away /
    // Escape dismissals — every event routes straight to the content
    // child. That is what lets the field place its cursor and receive
    // typing when a host dispatches events to us directly (the node
    // editor's in-pane overlay path), mirroring what the framework's
    // subtree walk already does for a child mounted in the main tree.
    if !window.chrome {
        if let Event::MouseDown { pos, .. } = event {
            if window.modal
                && window.click_away == ClickAwayAction::Close
                && !window.point_in_local_bounds(*pos)
            {
                // Click-away: the on_close callback decides commit vs
                // revert from the `ClickAway` reason.
                window.close(CloseReason::ClickAway);
                return EventResult::Consumed;
            }
        }
        if let Event::KeyDown {
            key: Key::Escape, ..
        } = event
        {
            if window.modal {
                window.close(CloseReason::Escape);
                return EventResult::Consumed;
            }
        }
        // This forward only re-delivers events the child IGNORED via the
        // framework's normal subtree routing (e.g. hover moves) — a benign
        // duplicate. Cursor placement (MouseDown) is single-dispatch: the
        // subtree walk hands it to the child first, which consumes it, so
        // it never reaches this fallback twice.
        if let Some(child) = window.children.first_mut() {
            let cb = child.bounds();
            let forwarded = match event {
                Event::MouseDown {
                    pos,
                    button,
                    modifiers,
                } => Event::MouseDown {
                    pos: Point::new(pos.x - cb.x, pos.y - cb.y),
                    button: *button,
                    modifiers: *modifiers,
                },
                Event::MouseUp {
                    pos,
                    button,
                    modifiers,
                } => Event::MouseUp {
                    pos: Point::new(pos.x - cb.x, pos.y - cb.y),
                    button: *button,
                    modifiers: *modifiers,
                },
                Event::MouseMove { pos } => Event::MouseMove {
                    pos: Point::new(pos.x - cb.x, pos.y - cb.y),
                },
                other => other.clone(),
            };
            return child.on_event(&forwarded);
        }
        return EventResult::Ignored;
    }

    match event {
        Event::MouseMove { pos } => {
            let was_close = window.close_hovered;
            let was_max = window.maximize_hovered;
            let was_dir = window.hover_dir;
            window.close_hovered = window.in_close_button(*pos);
            window.maximize_hovered = window.in_maximize_button(*pos);

            match window.drag_mode {
                DragMode::Move => {
                    let world = Point::new(pos.x + window.bounds.x, pos.y + window.bounds.y);
                    let dx = world.x - window.drag_start_world.x;
                    let dy = world.y - window.drag_start_world.y;
                    window.bounds.x = (window.drag_start_bounds.x + dx).round();
                    window.bounds.y = (window.drag_start_bounds.y + dy).round();
                    // Snap pass — runs only when the global flag
                    // is on.  Reads the thread-local target list
                    // populated by every other window's `layout`
                    // and writes the resulting visual guides for
                    // `SnapOverlay` to render.
                    window.apply_move_snap();
                    window.clamp_to_canvas();
                    window.hover_dir = None;
                    set_cursor_icon(CursorIcon::Grabbing);
                    crate::animation::request_draw_without_invalidation();
                    return EventResult::Ignored;
                }
                DragMode::Resize(dir) => {
                    let world = Point::new(pos.x + window.bounds.x, pos.y + window.bounds.y);
                    window.apply_resize(world);
                    window.apply_resize_snap(dir);
                    set_cursor_icon(resize_cursor(dir));
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                DragMode::None => {
                    // Track which edge/corner the cursor is hovering over so
                    // paint_overlay can draw the appropriate highlight.
                    window.hover_dir = window.resize_dir(*pos);
                    if let Some(dir) = window.hover_dir {
                        set_cursor_icon(resize_cursor(dir));
                    }
                }
            }
            if was_close != window.close_hovered
                || was_max != window.maximize_hovered
                || was_dir != window.hover_dir
            {
                crate::animation::request_draw();
            }
            EventResult::Ignored
        }

        // Click-away dismissal: while this window holds the modal grab, the
        // App routes EVERY press to our subtree — including presses OUTSIDE
        // our bounds (they arrive here with an out-of-range local `pos`). If
        // click-away is enabled, ANY button pressed outside closes us through
        // the unified `close()` path with `ClickAway` and swallows the press
        // so it never activates whatever sat underneath. This arm precedes
        // the button-specific handlers so a right/middle press-away dismisses
        // too (a right-click must not, e.g., start a title drag). Wheel
        // events are `MouseWheel`, not `MouseDown`, so scrolling outside
        // stays inert.
        Event::MouseDown { pos, .. }
            if window.modal
                && window.click_away == ClickAwayAction::Close
                && !window.point_in_local_bounds(*pos) =>
        {
            window.close(CloseReason::ClickAway);
            EventResult::Consumed
        }

        Event::MouseDown { button, pos, .. }
            if matches!(*button, MouseButton::Left | MouseButton::Middle) =>
        {
            let is_left_click = *button == MouseButton::Left;

            // Press-to-raise: any direct press on this window brings it forward.
            window.raise_request.set(true);
            // Z-order changes are visible; repaint.
            crate::animation::request_draw();
            if let Some(cb) = window.on_raised.as_mut() {
                cb(&window.title);
            }

            // Close button — highest priority.
            if is_left_click && window.in_close_button(*pos) {
                window.close(CloseReason::CloseButton);
                return EventResult::Consumed;
            }

            // Maximize / Restore button.
            if is_left_click && window.in_maximize_button(*pos) {
                window.toggle_maximize();
                crate::animation::request_draw();
                return EventResult::Consumed;
            }

            // Route the click into the title-bar sub-tree FIRST so
            // any child widget there (currently the chevron) gets a
            // chance to consume it.  `WindowTitleBar` lives outside
            // `Window.children` because the body content owns that
            // slot, so the framework's normal hit-test pass never
            // descends into it — we run the framework's hit-test
            // + dispatch helpers manually on the sub-tree instead.
            if is_left_click && window.in_title_bar(*pos) {
                let tb_bounds = window.title_bar.bounds();
                let tb_local = Point::new(pos.x - tb_bounds.x, pos.y - tb_bounds.y);
                if let Some(path) = crate::widget::hit_test_subtree(&window.title_bar, tb_local) {
                    // Path could be empty (clicked the bar itself
                    // but not a child) — skip in that case so the
                    // title-drag handling further down still runs.
                    if !path.is_empty() {
                        // Preserve modifiers from the original event.
                        let mods = match event {
                            Event::MouseDown { modifiers, .. } => *modifiers,
                            _ => Default::default(),
                        };
                        let translated = Event::MouseDown {
                            pos: tb_local,
                            button: *button,
                            modifiers: mods,
                        };
                        let result = crate::widget::dispatch_event_dyn(
                            &mut window.title_bar,
                            &path,
                            &translated,
                            tb_local,
                        );
                        if result.is_consumed() {
                            // Chevron flag is drained in `layout`,
                            // but we also want this frame to redraw
                            // before that.
                            crate::animation::request_draw();
                            return EventResult::Consumed;
                        }
                    }
                }
            }

            // Resize edge — check before title bar to handle corner overlap.
            if let Some(dir) = window.resize_dir(*pos) {
                // Only start resize if not in the close button area and not a pure title bar drag.
                // The N edge overlaps the title bar — prefer resize over drag from the top N px.
                let world = Point::new(pos.x + window.bounds.x, pos.y + window.bounds.y);
                window.drag_mode = DragMode::Resize(dir);
                window.drag_start_world = world;
                window.drag_start_bounds = window.bounds;
                return EventResult::Consumed;
            }

            // Title bar drag + double-click maximize.
            if window.in_title_bar(*pos) {
                // Double-click detection.
                let is_double = if is_left_click {
                    let now = Instant::now();
                    window
                        .last_title_click
                        .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                        .unwrap_or(false)
                } else {
                    false
                };

                if is_double {
                    // Windows convention: double-click title bar toggles
                    // maximize / restore.  Collapse/expand lives on the
                    // chevron button to the left.
                    window.toggle_maximize();
                    window.last_title_click = None;
                    crate::animation::request_draw();
                } else {
                    if is_left_click {
                        window.last_title_click = Some(Instant::now());
                    }
                    let world = Point::new(pos.x + window.bounds.x, pos.y + window.bounds.y);
                    window.drag_mode = DragMode::Move;
                    window.drag_start_world = world;
                    window.drag_start_bounds = window.bounds;
                }
                return EventResult::Consumed;
            }

            // Click on content area: consume so it doesn't fall through.
            if is_left_click && !window.collapsed {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }

        Event::MouseUp {
            button: MouseButton::Left | MouseButton::Middle,
            ..
        } => {
            let was_dragging = window.drag_mode != DragMode::None;
            window.drag_mode = DragMode::None;
            if was_dragging {
                // Drag ended — wipe the snap guides so the
                // overlay clears.  Cheap no-op when snapping was
                // off (guide buffer was already empty).
                crate::snap::clear_guides();
                crate::animation::request_draw();
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }

        // Escape closes a modal window (standard dialog convention),
        // running the same teardown as the × button so `on_close` fires.
        // Modal key routing bubbles Escape up to us when no inner field
        // consumed it (see `App::on_key_down`); non-modal windows ignore it
        // so app-level shortcuts still see the key.
        Event::KeyDown {
            key: Key::Escape, ..
        } if window.modal => {
            window.close(CloseReason::Escape);
            EventResult::Consumed
        }

        _ => EventResult::Ignored,
    }
}
