//! Builder methods for [`super::Window`] — extracted into a submodule so
//! `window.rs` stays under the 800-line guardrail. These are all plain
//! setter / configuration calls that mutate `self` and return it; they
//! live in a sibling `impl Window` block that has full access to private
//! fields by virtue of being inside the parent `window` module.

use std::cell::Cell;
use std::rc::Rc;

use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor};

use super::{Window, VISIBILITY_FADE_SECS};

impl Window {
    /// Treat the window's content as live: invalidate the backbuffer
    /// every frame so custom paint code reading from a mutating data
    /// source (network feed, simulation state, sensor stream) is
    /// rasterised fresh.  Automatically skipped when the window is
    /// collapsed or hidden — no wasted work when the user can't see
    /// the content.
    ///
    /// Use this for diagnostics graphs, telemetry views, or any widget
    /// whose `paint()` reads from an `Rc<RefCell<…>>` model that the
    /// framework can't observe.  The alternative — composing live UI
    /// from widgets that auto-invalidate on setter calls
    /// ([`Label::set_text`](crate::widgets::Label), etc.) — is preferred
    /// when feasible, but for custom direct-to-`DrawCtx` widgets this
    /// is the simplest correct fix.
    ///
    /// See [`Window::new`] for the full discussion of the back-buffer
    /// invalidation contract and the canonical "stale pixels" gotcha
    /// this flag solves.
    pub fn with_live_content(mut self, live: bool) -> Self {
        self.live_content = live;
        self
    }

    /// Register a callback fired whenever this window requests a raise
    /// (click-to-front or visibility rising-edge from the sidebar).
    /// Receives the window title.  The demo uses this to feed a shared
    /// z-order tracker that gets persisted to disk.
    pub fn on_raised(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_raised = Some(Box::new(cb));
        self
    }

    pub fn with_bounds(mut self, b: Rect) -> Self {
        self.pre_collapse_h = b.height;
        self.bounds = b;
        if self.maximized {
            self.pre_maximize_bounds = b;
        }
        self
    }
    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_visible_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        let visible = cell.get();
        self.last_visible.set(visible);
        self.fade_out_active.set(false);
        self.visibility_anim =
            crate::animation::Tween::new(if visible { 1.0 } else { 0.0 }, VISIBILITY_FADE_SECS);
        self.visible_cell = Some(cell);
        self
    }

    pub fn with_reset_cell(mut self, cell: Rc<Cell<Option<Rect>>>) -> Self {
        self.reset_to = Some(cell);
        self
    }

    pub fn with_position_cell(mut self, cell: Rc<Cell<Rect>>) -> Self {
        self.position_cell = Some(cell);
        self
    }

    /// Wire the window's canvas-maximized state into external persistence.
    ///
    /// Call after [`with_bounds`] when restoring saved state so the current
    /// bounds become the pre-maximize bounds used by the first layout pass.
    pub fn with_maximized_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.maximized = cell.get();
        if self.maximized {
            self.pre_maximize_bounds = self.bounds;
        }
        self.maximized_cell = Some(cell);
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    pub fn with_constrain(mut self, constrain: bool) -> Self {
        self.constrain = constrain;
        self
    }

    /// Opt this window in/out of the generic retained GL-FBO backbuffer.
    /// Disabling renders directly into the inherited parent target.
    pub fn with_gl_backbuffer(mut self, enabled: bool) -> Self {
        self.use_gl_backbuffer = enabled;
        self.backbuffer.invalidate();
        self
    }

    /// Make the window size itself to the content's preferred size every frame.
    /// Top-left pin: as content grows/shrinks, the title bar stays where it is.
    pub fn with_auto_size(mut self, auto: bool) -> Self {
        self.auto_size = auto;
        self
    }

    /// Toggle user-dragged resize.  `false` hides every edge/corner handle
    /// and disables resize hit-tests.  Default: `true`.  Matches egui's
    /// `Window::resizable(bool)`.
    pub fn with_resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    /// Fine-grained axis-locking of the resize handles — pass `(true, false)`
    /// for a horizontally-only resizable window, etc.  Implies
    /// `with_resizable(true)`.  Matches egui's `Window::resizable([h, v])`.
    pub fn with_resizable_axes(mut self, h: bool, v: bool) -> Self {
        self.resizable = h || v;
        self.resizable_h = h;
        self.resizable_v = v;
        self
    }

    /// Lock the window's height to its content's required height.
    /// The user can grab a vertical resize handle but the next
    /// layout snaps back — egui's W4 "no scroll, no clip, no
    /// whitespace" contract.  Requires the content tree to expose
    /// its required height via [`Widget::measure_min_height`]; our
    /// `FlexColumn`, `Label`, `TextArea`, and `Container::with_fit_height`
    /// all do.
    pub fn with_tight_content_fit(mut self, on: bool) -> Self {
        self.tight_content_fit = on;
        self
    }

    /// Floor-only variant of [`with_tight_content_fit`]: refuses to
    /// shrink past content but allows the user to pull the window
    /// taller (whitespace below).  Used for windows whose content
    /// includes a flex-fill child like a multiline `TextArea` —
    /// matches egui's W5 where the TextEdit fills extra height and
    /// the user can grow the window further.
    pub fn with_height_floor_to_content(mut self, on: bool) -> Self {
        self.floor_content_height = on;
        self
    }

    /// Wrap the window's content in a built-in vertical [`ScrollView`].
    /// Matches egui's `Window::vscroll(true)`: lets the user shrink the
    /// window below content height without the caller having to wrap the
    /// content in a `ScrollView` manually.  Eager — happens at builder
    /// time so the rest of the layout / event / paint paths see a single
    /// child as usual.  Has no effect when called with `false` (matches
    /// the default).
    ///
    /// Don't combine with [`with_auto_size`]: the ScrollView claims its
    /// full available height, which would make auto-sizing grow the
    /// window to the canvas.  egui's demo never combines the two flags
    /// either.
    pub fn with_vscroll(mut self, vscroll: bool) -> Self {
        if vscroll {
            if let Some(content) = self.children.pop() {
                let scroll = crate::widgets::ScrollView::new(content)
                    .vertical(true)
                    .horizontal(false);
                self.children.push(Box::new(scroll));
            }
        }
        self
    }

    pub fn on_close(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_close = Some(Box::new(cb));
        self
    }
}
