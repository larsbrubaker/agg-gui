//! WebGL2 resource container for the WASM platform shell.
//!
//! After the demo-content/3D-animation extraction, this file is a thin
//! WASM-only shim:
//!
//! - **`GlState`** — owns the `Rc<glow::Context>` for this thread.
//!   That's the entire WASM-specific responsibility: bridging the
//!   `web_sys::WebGl2RenderingContext` we got from the canvas into the
//!   `glow::Context` everyone else uses.
//!
//! - **Re-exports of `GlCubeWidget` + `CUBE_SCREEN_RECT`** from
//!   `demo_gl::bar_grid` so existing import paths in `lib.rs` keep
//!   working.  The renderer + widget shell live in `demo-gl` so
//!   `demo-native` and `demo-wasm` exercise *byte-identical* compiled
//!   code for the demo content.  The platform crates contain only
//!   OS-shell glue (event loop, window/canvas init, state-storage
//!   backend) — never any demo content.
//!
//! Previously this file held a duplicate of the bar-grid renderer plus
//! a dead-code `GlPresenter` (legacy AGG-framebuffer-as-texture blit
//! path, unused since the GL backend started rendering directly).
//! Both are gone — see `demo-gl/src/bar_grid.rs` for the single source
//! of truth, and the git history of this file for the legacy code.

use std::rc::Rc;

// Re-exports — keeps `lib.rs` imports compatible across the move.
pub use demo_gl::{GlCubeWidget, CUBE_SCREEN_RECT};

/// WASM-side GL context owner.  Created once per page load with the
/// `glow::Context` derived from the canvas's `WebGl2RenderingContext`.
/// The browser sandbox forbids cross-thread sharing of WebGL contexts,
/// so a single-threaded `Rc` is the right primitive.
pub struct GlState {
    gl: Rc<glow::Context>,
}

impl GlState {
    pub unsafe fn new(gl: glow::Context) -> Self {
        Self { gl: Rc::new(gl) }
    }

    /// Reference-counted clone of the GL context (cheap `Rc` increment).
    pub fn gl_rc(&self) -> Rc<glow::Context> {
        Rc::clone(&self.gl)
    }
}
