//! Paint containment: a widget whose paint callback panics costs its own
//! frame and nothing else.
//!
//! agg-sharp's hosts catch a throw out of a widget's draw at the window's
//! paint (`MacSystemWindow` / `WinformsSystemWindow` / `X11SystemWindow` paint
//! catch blocks), report it through `UiThread.ReportUnhandledException` and
//! keep the loop running; `GlStackBalance.ResetForFrame` forgets the matrix
//! pushes the abandoned frame leaked, because a draw that throws every frame
//! would otherwise trip the stack-depth guard at the host's own first push.
//!
//! Here the boundary is each widget callback ([`Widget::paint`],
//! [`Widget::paint_overlay`], [`Widget::finish_paint`],
//! [`Widget::paint_global_overlay`]) rather than the whole frame, so the rest
//! of the tree still paints. A panic is reported as
//! [`UnhandledOrigin::Paint`](crate::unhandled::UnhandledOrigin::Paint)
//! through [`crate::report_unhandled`]; the context's `save`/layer levels
//! ([`DrawCtx::paint_nesting`]) and the paint clip stack are put back to what
//! they were before the callback ran. With no handler installed on the thread
//! the panic is re-raised (as for idle actions), so a unit test that paints a
//! broken widget still fails.

use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

use super::*;
use crate::unhandled::{report_panic, UnhandledOrigin};

/// Run one widget paint callback `call` under containment; see the module docs.
pub(super) fn contained(ctx: &mut dyn DrawCtx, call: impl FnOnce(&mut dyn DrawCtx)) {
    let nesting = ctx.paint_nesting();
    let clip_depth = PAINT_CLIP_STACK.with(|stack| stack.borrow().len());
    // AssertUnwindSafe: after a panic nothing the closure captured is trusted
    // again this frame. `ctx` is rebalanced below and used only through that
    // (and the traversal's own save/restore pairs); the widget behind `call`
    // may be half updated, which is its own state and is repainted from
    // scratch next frame; and the clip stack is cut back to its entry depth.
    let Err(payload) = catch_unwind(AssertUnwindSafe(|| call(&mut *ctx))) else {
        return;
    };
    ctx.unwind_paint_nesting(nesting);
    PAINT_CLIP_STACK.with(|stack| stack.borrow_mut().truncate(clip_depth));
    if let Some(unreported) = report_panic(UnhandledOrigin::Paint, payload) {
        resume_unwind(unreported);
    }
}
