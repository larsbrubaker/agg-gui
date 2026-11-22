//! One-shot capture of the next drawn frame, asked for by app code and served
//! by whichever shell draws the window.
//!
//! A widget asks from a click handler, with no access to the shell, the way it
//! asks for a layout with [`crate::animation::request_layout`]:
//!
//! ```ignore
//! agg_gui::frame_capture::request_frame_capture(Some(view_rect), |result| {
//!     match result {
//!         Ok(frame) => attach_screenshot(frame.rgba, frame.width, frame.height),
//!         Err(e) => show_message(&e.to_string()),
//!     }
//! });
//! ```
//!
//! The request is queued in this thread's list and asks for a frame
//! ([`crate::animation::request_draw`]), so an idle app draws one. The shell
//! takes the queued requests ([`take_frame_capture_requests`]) at the start of
//! its next frame, renders that one frame where it can be read back, starts a
//! non-blocking read of each requested region, and calls each callback once,
//! on a later frame, when its pixels arrive — or with an error when they
//! cannot (device lost, capture unavailable, region empty or off screen).
//! The callback runs on the UI thread, inside the shell's frame, after the
//! frame has been presented.
//!
//! Frames nobody asked to capture take the shell's ordinary path: nothing here
//! costs anything while the queue is empty ([`has_pending_frame_capture`]).
//!
//! The GPU side lives in `agg-gui-wgpu` (`FrameCaptureService`), shared by the
//! native shell (`agg-gui-shell`) and the browser shell (`agg-gui-web-shell`).
//! This module is deliberately GPU-free so app crates can ask without
//! depending on a backend.
//!
//! # Coordinates
//!
//! A region is a [`Rect`] in window coordinates: logical units, Y-up from the
//! window's bottom-left corner — the space the root widget's children's
//! `bounds()` are in (a nested widget converts with
//! `widget::coords`). The shell converts it to framebuffer pixels with
//! [`crate::ux_scale::effective_scale`] ([`region_in_pixels`]). The delivered
//! pixels are tight RGBA8, top row first.

use std::cell::RefCell;
use std::fmt;

use crate::geometry::Rect;

/// The pixels of a captured frame (or region of one): tight RGBA8, top row
/// first, `width * height * 4` bytes, with the window's own colour bytes
/// (sRGB-encoded when the window's surface is).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Why a frame capture delivered no pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameCaptureError {
    /// The GPU device was lost before the frame's pixels could be read.
    DeviceLost(String),
    /// The shell cannot read this frame back (the read failed, or the shell
    /// shut down before serving the request).
    Unavailable(String),
    /// The requested region has no pixels, or lies entirely off the window.
    RegionEmpty,
}

impl fmt::Display for FrameCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceLost(reason) => write!(f, "the graphics device was lost: {reason}"),
            Self::Unavailable(reason) => write!(f, "the frame could not be captured: {reason}"),
            Self::RegionEmpty => write!(f, "the area to capture is empty or off screen"),
        }
    }
}

impl std::error::Error for FrameCaptureError {}

/// What a capture's callback receives.
pub type FrameCaptureResult = Result<CapturedFrame, FrameCaptureError>;

type Callback = Box<dyn FnOnce(FrameCaptureResult)>;

/// One queued capture, as the shell takes it. Delivering consumes it, so a
/// callback runs at most once; dropping it undelivered delivers
/// [`FrameCaptureError::Unavailable`], so a request never waits forever on a
/// shell that went away.
pub struct FrameCaptureRequest {
    region: Option<Rect>,
    on_done: Option<Callback>,
}

impl FrameCaptureRequest {
    /// The requested region in window coordinates; `None` is the whole frame.
    pub fn region(&self) -> Option<Rect> {
        self.region
    }

    /// Call the request's callback with `result`.
    pub fn deliver(mut self, result: FrameCaptureResult) {
        if let Some(cb) = self.on_done.take() {
            cb(result);
        }
    }
}

impl Drop for FrameCaptureRequest {
    fn drop(&mut self) {
        if let Some(cb) = self.on_done.take() {
            cb(Err(FrameCaptureError::Unavailable(
                "the window stopped drawing before the capture was taken".to_string(),
            )));
        }
    }
}

impl fmt::Debug for FrameCaptureRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameCaptureRequest")
            .field("region", &self.region)
            .finish_non_exhaustive()
    }
}

thread_local! {
    static QUEUE: RefCell<Vec<FrameCaptureRequest>> = const { RefCell::new(Vec::new()) };
}

/// Ask for the next drawn frame (or `region` of it, in window coordinates —
/// see the module docs) to be delivered once to `on_done`. Makes a frame get
/// drawn even when the app is idle. Call on the UI thread; `on_done` runs
/// there too, on a later frame.
pub fn request_frame_capture(
    region: Option<Rect>,
    on_done: impl FnOnce(FrameCaptureResult) + 'static,
) {
    QUEUE.with(|q| {
        q.borrow_mut().push(FrameCaptureRequest {
            region,
            on_done: Some(Box::new(on_done)),
        })
    });
    crate::animation::request_draw();
}

/// Whether a capture is waiting for the shell's next frame.
pub fn has_pending_frame_capture() -> bool {
    QUEUE.with(|q| !q.borrow().is_empty())
}

/// Take every queued capture, oldest first. For shells: the frame that takes
/// them is the frame they capture.
pub fn take_frame_capture_requests() -> Vec<FrameCaptureRequest> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

/// A rectangle in framebuffer pixels: origin at the frame's top-left, +Y down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Convert a window-coordinate `region` (logical, Y-up; `None` = the whole
/// frame) to framebuffer pixels of a `frame_w` x `frame_h` frame drawn at
/// `scale` physical pixels per logical unit, clipped to the frame. Partly
/// covered pixels at the edges are included. A region with no pixels on the
/// frame is [`FrameCaptureError::RegionEmpty`].
pub fn region_in_pixels(
    region: Option<Rect>,
    scale: f64,
    frame_w: u32,
    frame_h: u32,
) -> Result<PixelRegion, FrameCaptureError> {
    if frame_w == 0 || frame_h == 0 {
        return Err(FrameCaptureError::RegionEmpty);
    }
    let Some(r) = region else {
        return Ok(PixelRegion {
            x: 0,
            y: 0,
            width: frame_w,
            height: frame_h,
        });
    };
    let (fw, fh) = (f64::from(frame_w), f64::from(frame_h));
    // Pixel edges in a Y-up frame, clipped to it. `clamp` also maps NaN-free
    // infinities onto the frame edges.
    let left = (r.x * scale).floor().clamp(0.0, fw);
    let right = ((r.x + r.width) * scale).ceil().clamp(0.0, fw);
    let bottom = (r.y * scale).floor().clamp(0.0, fh);
    let top = ((r.y + r.height) * scale).ceil().clamp(0.0, fh);
    if r.width.is_nan() || r.height.is_nan() || r.width <= 0.0 || r.height <= 0.0 {
        return Err(FrameCaptureError::RegionEmpty);
    }
    if right <= left || top <= bottom || left.is_nan() || bottom.is_nan() {
        return Err(FrameCaptureError::RegionEmpty);
    }
    // In range [0, frame size] after the clamps, so the casts are exact.
    Ok(PixelRegion {
        x: left as u32,
        y: (fh - top) as u32,
        width: (right - left) as u32,
        height: (top - bottom) as u32,
    })
}

#[cfg(test)]
#[path = "frame_capture_tests.rs"]
mod tests;
