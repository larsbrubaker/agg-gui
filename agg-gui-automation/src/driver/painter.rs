//! Where a [`HeadlessDriver`](super::HeadlessDriver) paints its frames.
//!
//! The driver decides *when* a frame paints (the shells' frame policy); a
//! [`HeadlessPainter`] decides *into what*. [`SoftwarePainter`] — agg-gui's
//! software [`GfxCtx`] over a [`Framebuffer`] — is the default. An application
//! whose real renderer is the GPU (MatterCAD paints through agg-gui-wgpu's
//! `HeadlessFrame`) plugs in its own painter with
//! [`HeadlessDriver::with_painter`](super::HeadlessDriver::with_painter), or one
//! that keeps no pixels at all (a context that draws nothing) where no GPU
//! adapter exists. This crate does not depend on agg-gui-wgpu; the GPU painter
//! lives with the application that wants it.

use agg_gui::{App, Framebuffer, GfxCtx};

/// A paint target for the headless driver; see the module docs.
pub trait HeadlessPainter {
    /// Paint `app` into a fresh frame of `width × height` physical pixels
    /// (already laid out by the driver). A shell presents a fresh surface
    /// each frame, so a painter starts every frame from clear.
    fn paint(&mut self, app: &mut App, width: u32, height: u32);

    /// The window was resized to `width × height` physical pixels; the next
    /// frame paints at that size. Painters that size themselves in
    /// [`paint`](Self::paint) need do nothing.
    fn resize(&mut self, width: u32, height: u32) {
        let _ = (width, height);
    }

    /// The last painted frame, when this painter keeps it in memory as a
    /// [`Framebuffer`] (the software painter does). `None` sends the driver
    /// to [`capture`](Self::capture).
    fn screen(&self) -> Option<&Framebuffer> {
        None
    }

    /// The last painted frame copied out of the painter, for painters whose
    /// pixels live elsewhere (a GPU painter reads its target back here, only
    /// when a test asks for the screen). `None` means the painter has no
    /// pixels; the driver then reports a blank screen of the window's size.
    fn capture(&self) -> Option<Framebuffer> {
        None
    }
}

/// The default painter: agg-gui's software renderer into a [`Framebuffer`].
pub struct SoftwarePainter {
    framebuffer: Framebuffer,
}

impl SoftwarePainter {
    /// A software painter for a `width × height` physical-pixel window.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            framebuffer: Framebuffer::new(width.max(1), height.max(1)),
        }
    }

    /// The last painted frame.
    pub fn framebuffer(&self) -> &Framebuffer {
        &self.framebuffer
    }
}

impl HeadlessPainter for SoftwarePainter {
    fn paint(&mut self, app: &mut App, width: u32, height: u32) {
        if (self.framebuffer.width(), self.framebuffer.height()) != (width, height) {
            self.framebuffer.resize(width.max(1), height.max(1));
        }
        // A shell presents a fresh surface each frame; start from clear.
        self.framebuffer.pixels_mut().fill(0);
        let mut ctx = GfxCtx::new(&mut self.framebuffer);
        app.paint(&mut ctx);
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.framebuffer.resize(width.max(1), height.max(1));
    }

    fn screen(&self) -> Option<&Framebuffer> {
        Some(&self.framebuffer)
    }
}
