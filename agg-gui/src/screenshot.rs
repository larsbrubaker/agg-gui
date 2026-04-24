//! Screenshot capture handle for agg-gui apps.
//!
//! The GL rendering harness (`GlGfxCtx::read_screenshot` on the desktop GL
//! path + the equivalent WebGL2 read-back in the WASM harness) produces a
//! top-down RGBA8 buffer of the current back buffer.  This module supplies
//! the small shared-state handle that a button or hotkey uses to
//! **request** a capture and that a widget uses to **display** the result.
//!
//! # Threading / ownership
//!
//! All fields are `Rc<...>` — single-threaded, cheap to clone.  Never
//! transfer a [`ScreenshotHandle`] across threads.
//!
//! # Wiring on native (winit + glow)
//!
//! ```ignore
//! let shot = agg_gui::ScreenshotHandle::new();
//!
//! // In a button's on_click:
//! let req = shot.request.clone();
//! Button::new("📷 Capture", font).on_click(move || req.set(true))
//!
//! // In the event loop, AFTER render_frame but BEFORE swap_buffers:
//! if shot.request.get() {
//!     let (rgba, w, h) = gl_ctx.read_screenshot();
//!     *shot.image.borrow_mut() = Some((rgba, w, h));
//!     shot.request.set(false);
//! }
//!
//! // Display: pass `shot.image` to `ImageView`.
//! ```
//!
//! # Wiring on WASM
//!
//! Same Rust-side flow — the browser's WebGL2 context still provides
//! `glReadPixels`, so `GlGfxCtx::read_screenshot()` works unchanged.  The
//! JS side needs no special code beyond driving the animation loop:
//!
//! ```ignore
//! // In the WASM render export (called from JS requestAnimationFrame):
//! if shot.request.get() {
//!     let (rgba, w, h) = gl_ctx.read_screenshot();  // must be BEFORE presenting
//!     *shot.image.borrow_mut() = Some((rgba, w, h));
//!     shot.request.set(false);
//! }
//! ```
//!
//! Note for the LLM / future dev: on WASM, `read_screenshot` MUST be called
//! before the browser composites the canvas (i.e. within the same rAF
//! tick, before yielding).  Because WebGL uses a preserved-drawing-buffer
//! only when explicitly requested, calling it outside that window yields
//! a blank image.  The natural "after paint, before yield" position in the
//! render function is correct.
//!
//! If the app wants to TRIGGER a browser download instead of displaying
//! in-canvas, export a WASM function that calls `read_screenshot`, encode
//! with the `png` crate via `agg_gui::encode_png_rgba` (if available in
//! the surrounding app), and pass the bytes to a JS helper that creates a
//! `Blob` + `URL.createObjectURL` + synthetic `<a download>` click.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

/// Shared capture state.  Clone freely; all inner fields are `Rc<...>`.
#[derive(Clone)]
pub struct ScreenshotHandle {
    /// Set to `true` to request a capture on the next rendered frame.  The
    /// platform harness reads this cell after painting, captures the
    /// framebuffer into `image`, and clears the flag.
    pub request: Rc<Cell<bool>>,
    /// Most recent captured image — top-down RGBA8, plus `(width, height)`.
    /// `None` until the first capture completes.
    pub image: Rc<RefCell<Option<(Vec<u8>, u32, u32)>>>,
}

impl ScreenshotHandle {
    pub fn new() -> Self {
        Self {
            request: Rc::new(Cell::new(false)),
            image: Rc::new(RefCell::new(None)),
        }
    }

    /// Convenience: request a capture.  Equivalent to `self.request.set(true)`.
    pub fn take(&self) {
        self.request.set(true);
    }

    /// `true` while the latest request has not yet been fulfilled.
    pub fn pending(&self) -> bool {
        self.request.get()
    }

    /// Access the most recent capture without consuming it.
    pub fn has_image(&self) -> bool {
        self.image.borrow().is_some()
    }
}

impl Default for ScreenshotHandle {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Export helpers ───────────────────────────────────────────────────────

/// Result of a platform screenshot export operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotExportOutcome {
    /// Native targets write the PNG to disk and return the saved path.
    Saved(std::path::PathBuf),
    /// Browser targets hand the operation to the DOM and return immediately.
    Started,
}

/// Error returned by screenshot export helpers.
#[derive(Debug)]
pub enum ScreenshotExportError {
    InvalidBuffer { expected: usize, actual: usize },
    Encode(String),
    Io(std::io::Error),
    Clipboard(String),
    Unsupported(&'static str),
}

impl fmt::Display for ScreenshotExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBuffer { expected, actual } => {
                write!(
                    f,
                    "invalid RGBA buffer: expected {expected} bytes, got {actual}"
                )
            }
            Self::Encode(msg) => write!(f, "PNG encode failed: {msg}"),
            Self::Io(err) => write!(f, "I/O failed: {err}"),
            Self::Clipboard(msg) => write!(f, "clipboard failed: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported screenshot export: {msg}"),
        }
    }
}

impl std::error::Error for ScreenshotExportError {}

impl From<std::io::Error> for ScreenshotExportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

fn validate_rgba_len(rgba: &[u8], width: u32, height: u32) -> Result<(), ScreenshotExportError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| ScreenshotExportError::Encode("image dimensions overflow".to_string()))?;
    if rgba.len() != expected {
        return Err(ScreenshotExportError::InvalidBuffer {
            expected,
            actual: rgba.len(),
        });
    }
    Ok(())
}

/// Encode a top-down RGBA8 image as a PNG.
pub fn encode_png_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ScreenshotExportError> {
    validate_rgba_len(rgba, width, height)?;

    let mut out = Vec::with_capacity(rgba.len() / 2);
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| ScreenshotExportError::Encode(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| ScreenshotExportError::Encode(e.to_string()))?;
    }
    Ok(out)
}

/// Download or save a top-down RGBA8 screenshot as a PNG.
pub fn download_rgba_as_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    filename: &str,
) -> Result<ScreenshotExportOutcome, ScreenshotExportError> {
    let png = encode_png_rgba(rgba, width, height)?;
    download_png(filename, &png)
}

/// Copy a top-down RGBA8 screenshot to the system clipboard.
pub fn copy_rgba_to_clipboard(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<ScreenshotExportOutcome, ScreenshotExportError> {
    validate_rgba_len(rgba, width, height)?;
    copy_rgba_to_clipboard_impl(rgba, width, height)
}

#[cfg(not(target_arch = "wasm32"))]
fn download_png(
    filename: &str,
    png: &[u8],
) -> Result<ScreenshotExportOutcome, ScreenshotExportError> {
    let dir = downloads_dir();
    std::fs::create_dir_all(&dir)?;
    let path = unique_download_path(&dir, filename);
    std::fs::write(&path, png)?;
    Ok(ScreenshotExportOutcome::Saved(path))
}

#[cfg(not(target_arch = "wasm32"))]
fn downloads_dir() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return std::path::PathBuf::from(profile).join("Downloads");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join("Downloads");
        }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

#[cfg(not(target_arch = "wasm32"))]
fn unique_download_path(dir: &std::path::Path, filename: &str) -> std::path::PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let path = std::path::Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("screenshot");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("png");
    for i in 1.. {
        let name = format!("{stem}-{i}.{ext}");
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("unbounded integer iterator should always produce a path")
}

#[cfg(all(not(target_arch = "wasm32"), feature = "clipboard"))]
fn copy_rgba_to_clipboard_impl(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<ScreenshotExportOutcome, ScreenshotExportError> {
    let image = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Borrowed(rgba),
    };
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_image(image))
        .map_err(|e| ScreenshotExportError::Clipboard(e.to_string()))?;
    Ok(ScreenshotExportOutcome::Started)
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "clipboard")))]
fn copy_rgba_to_clipboard_impl(
    _: &[u8],
    _: u32,
    _: u32,
) -> Result<ScreenshotExportOutcome, ScreenshotExportError> {
    Err(ScreenshotExportError::Unsupported(
        "enable the `clipboard` feature for native image clipboard support",
    ))
}

#[cfg(target_arch = "wasm32")]
fn download_png(
    filename: &str,
    png: &[u8],
) -> Result<ScreenshotExportOutcome, ScreenshotExportError> {
    if wasm_download_png(filename, png) {
        Ok(ScreenshotExportOutcome::Started)
    } else {
        Err(ScreenshotExportError::Unsupported(
            "browser download API is unavailable",
        ))
    }
}

#[cfg(target_arch = "wasm32")]
fn copy_rgba_to_clipboard_impl(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<ScreenshotExportOutcome, ScreenshotExportError> {
    let png = encode_png_rgba(rgba, width, height)?;
    if wasm_copy_png_to_clipboard(&png) {
        Ok(ScreenshotExportOutcome::Started)
    } else {
        Err(ScreenshotExportError::Unsupported(
            "browser image clipboard API is unavailable",
        ))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function wasm_download_png(filename, bytes) {
    try {
        const blob = new Blob([bytes], { type: "image/png" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = filename || "agg-gui-screenshot.png";
        a.style.display = "none";
        document.body.appendChild(a);
        a.click();
        a.remove();
        URL.revokeObjectURL(url);
        return true;
    } catch (err) {
        console.error("agg-gui screenshot download failed", err);
        return false;
    }
}

export function wasm_copy_png_to_clipboard(bytes) {
    try {
        if (!navigator.clipboard || typeof ClipboardItem === "undefined") {
            return false;
        }
        const blob = new Blob([bytes], { type: "image/png" });
        navigator.clipboard
            .write([new ClipboardItem({ "image/png": blob })])
            .catch(err => console.error("agg-gui screenshot clipboard failed", err));
        return true;
    } catch (err) {
        console.error("agg-gui screenshot clipboard failed", err);
        return false;
    }
}
"#)]
extern "C" {
    fn wasm_download_png(filename: &str, bytes: &[u8]) -> bool;
    fn wasm_copy_png_to_clipboard(bytes: &[u8]) -> bool;
}

// ─── Capture-aware render orchestration ─────────────────────────────────
//
// Both the native (winit/glutin) and wasm (rAF/WebGL2) harnesses need the
// same "screenshot capture" flow around their per-frame render:
//
//   1. If a capture was requested:
//        a. Flip `capturing` to true so the Screenshot preview pane
//           paints empty (so captured pixels don't include last frame's
//           preview — the hall-of-mirrors bug).
//        b. Render the frame (platform-specific: clear + paint widgets).
//        c. `glReadPixels` the back buffer (platform-specific).
//        d. Publish the bytes into `image` and clear both flags.
//        e. Render again — this time the preview pane reveals the
//           freshly-captured image.
//   2. Otherwise: render once.
//
// The orchestration (flag flipping, double-render, Arc wrap) is
// platform-agnostic and belongs here; each host supplies two closures:
//  - `render_fn()`          : clear the framebuffer and paint the widget
//                             tree once (the host's existing frame path).
//  - `read_back_buffer()`   : glReadPixels the current framebuffer and
//                             return `(rgba, width, height)`.

/// Run one frame through the screenshot capture flow.
///
/// Call this instead of invoking the per-frame render directly.  It runs
/// the single-render path in the common case and the double-render
/// capture path when `request` is set.
///
/// `ctx` is the host's rendering context (e.g. the GL `GlGfxCtx`) —
/// passed in once and handed through to each closure so the two
/// closures don't both borrow it from their capture environment (which
/// the borrow checker can't reconcile statically even though the
/// closures are invoked sequentially).
///
/// The `image` field uses the `Arc<Vec<u8>>` form so the GL back-end's
/// texture cache can key on the Arc's pointer identity — see
/// `gfx_ctx::draw_image_rgba_arc`.
pub fn run_frame_with_capture<C>(
    request: &Rc<Cell<bool>>,
    capturing: &Rc<Cell<bool>>,
    image: &Rc<RefCell<Option<(std::sync::Arc<Vec<u8>>, u32, u32)>>>,
    ctx: &mut C,
    mut render_fn: impl FnMut(&mut C),
    read_back_buffer: impl FnOnce(&mut C) -> (Vec<u8>, u32, u32),
) {
    if !request.get() {
        render_fn(ctx);
        return;
    }
    capturing.set(true);
    render_fn(ctx);
    let (rgba, w, h) = read_back_buffer(ctx);
    *image.borrow_mut() = Some((std::sync::Arc::new(rgba), w, h));
    capturing.set(false);
    request.set(false);
    render_fn(ctx);
}
