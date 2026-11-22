//! One-shot capture of the next drawn frame, asked for by app code and served
//! by whichever shell draws the window.
//!
//! A widget asks from a click handler, with no access to the shell, the way it
//! asks for a layout with [`crate::animation::request_layout`]:
//!
//! ```ignore
//! let handle = agg_gui::frame_capture::request_frame_capture(Some(view_rect), |result| {
//!     match result {
//!         Ok(frame) => attach_screenshot(frame.rgba, frame.width, frame.height),
//!         Err(e) => show_message(&e.to_string()),
//!     }
//! });
//! self.pending_capture = Some(handle); // dropped when the app stops waiting
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
//! The call returns a [`FrameCaptureRequestHandle`]. An app that stops
//! waiting (its own time limit ran out: a window with no working GPU, or
//! minimized, paints no frame) drops it, which withdraws the request: no
//! read-back is done for it and its callback never runs.
//!
//! Frames nobody asked to capture take the shell's ordinary path: nothing here
//! costs anything while the queue is empty ([`has_pending_frame_capture`]).
//!
//! # What "the frame" is
//!
//! The first frame the shell paints after the request: everything the host
//! paints into it (the widget tree, by default) and whatever its after-paint
//! hook draws into the context, exactly as the window shows it. Not part of
//! it: the native shell's presented-only overlay
//! (`ShellHost::paint_presented_overlay`, such as a simulated pointer), drawn
//! onto the window after the frame is read, and anything the operating system
//! or browser draws over the window. A frame the surface refuses (minimized,
//! occluded) is not painted, so the request waits for the next one that is.
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

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

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
/// shell that went away. A request its app withdrew
/// ([`FrameCaptureRequestHandle`]) delivers nothing, either way.
pub struct FrameCaptureRequest {
    region: Option<Rect>,
    on_done: Option<Callback>,
    /// Shared with the app's handle; set when the app withdraws.
    withdrawn: Rc<Cell<bool>>,
}

impl FrameCaptureRequest {
    /// The requested region in window coordinates; `None` is the whole frame.
    pub fn region(&self) -> Option<Rect> {
        self.region
    }

    /// Whether the app has withdrawn the request: a shell drops it, and
    /// abandons a read already started for it, instead of delivering.
    pub fn is_withdrawn(&self) -> bool {
        self.withdrawn.get()
    }

    /// Call the request's callback with `result`, unless the app withdrew it.
    pub fn deliver(mut self, result: FrameCaptureResult) {
        if self.withdrawn.get() {
            return;
        }
        if let Some(cb) = self.on_done.take() {
            cb(result);
        }
    }
}

impl Drop for FrameCaptureRequest {
    fn drop(&mut self) {
        if self.withdrawn.get() {
            return;
        }
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

/// The app's hold on a capture it asked for ([`request_frame_capture`]).
/// While it is kept, nothing changes. Dropping it, or calling
/// [`withdraw`](Self::withdraw), withdraws the request: one not yet taken by
/// a frame leaves the queue, and one already taken has its read abandoned at
/// the shell's next poll (its buffers released). The callback never runs
/// after the withdrawal. Once the callback has run, withdrawing does nothing.
///
/// `let _ = request_frame_capture(..)` drops the handle at once and cancels
/// the capture; bind it to a named variable or store it until the callback
/// runs.
#[must_use = "dropping the handle withdraws the capture: `let _ = request_frame_capture(..)` drops the handle at once and cancels the capture; bind it to a named variable or store it until the callback runs"]
#[derive(Debug)]
pub struct FrameCaptureRequestHandle {
    withdrawn: Rc<Cell<bool>>,
}

impl FrameCaptureRequestHandle {
    /// Withdraw the request now (dropping the handle does the same).
    pub fn withdraw(self) {}
}

impl Drop for FrameCaptureRequestHandle {
    fn drop(&mut self) {
        self.withdrawn.set(true);
        // Taken out under the borrow, dropped after it: the request's
        // callback may own other handles whose drop reaches the queue too.
        // A queue already borrowed (a withdrawal from inside a shell's take)
        // leaves the request to be dropped by its withdrawn flag.
        let removed = QUEUE.with(|q| {
            let mut q = q.try_borrow_mut().ok()?;
            let at = q
                .iter()
                .position(|r| Rc::ptr_eq(&r.withdrawn, &self.withdrawn))?;
            Some(q.remove(at))
        });
        drop(removed);
    }
}

/// Ask for the next drawn frame (or `region` of it, in window coordinates —
/// see the module docs) to be delivered once to `on_done`. Makes a frame get
/// drawn even when the app is idle. Call on the UI thread; `on_done` runs
/// there too, on a later frame. Keep the returned handle while waiting:
/// dropping it withdraws the request ([`FrameCaptureRequestHandle`]).
/// `let _ = request_frame_capture(..)` drops the handle at once and cancels
/// the capture; bind it to a named variable or store it until the callback
/// runs.
pub fn request_frame_capture(
    region: Option<Rect>,
    on_done: impl FnOnce(FrameCaptureResult) + 'static,
) -> FrameCaptureRequestHandle {
    let withdrawn = Rc::new(Cell::new(false));
    QUEUE.with(|q| {
        q.borrow_mut().push(FrameCaptureRequest {
            region,
            on_done: Some(Box::new(on_done)),
            withdrawn: Rc::clone(&withdrawn),
        })
    });
    crate::animation::request_draw();
    FrameCaptureRequestHandle { withdrawn }
}

/// Whether a capture is waiting for the shell's next frame.
pub fn has_pending_frame_capture() -> bool {
    QUEUE.with(|q| q.borrow().iter().any(|r| !r.is_withdrawn()))
}

/// Take every queued capture still wanted, oldest first. For shells: the
/// frame that takes them is the frame they capture.
pub fn take_frame_capture_requests() -> Vec<FrameCaptureRequest> {
    let mut taken = QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()));
    taken.retain(|r| !r.is_withdrawn());
    taken
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
/// frame, or with a non-finite field, is [`FrameCaptureError::RegionEmpty`].
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
    // A rect with a non-finite field names no pixels: an infinite edge would
    // clamp onto the frame, but an infinite width beside it sums to NaN.
    let finite = [r.x, r.y, r.width, r.height, scale]
        .iter()
        .all(|v| v.is_finite());
    if !finite || r.width <= 0.0 || r.height <= 0.0 || scale <= 0.0 {
        return Err(FrameCaptureError::RegionEmpty);
    }
    let (fw, fh) = (f64::from(frame_w), f64::from(frame_h));
    // Pixel edges in a Y-up frame, clipped to it. Finite inputs can still
    // overflow a product to infinity, which the clamp maps onto an edge; a
    // NaN edge (none is reachable from finite inputs, but checked) is empty.
    let left = (r.x * scale).floor().clamp(0.0, fw);
    let right = ((r.x + r.width) * scale).ceil().clamp(0.0, fw);
    let bottom = (r.y * scale).floor().clamp(0.0, fh);
    let top = ((r.y + r.height) * scale).ceil().clamp(0.0, fh);
    if [left, right, bottom, top].iter().any(|v| v.is_nan()) || right <= left || top <= bottom {
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
