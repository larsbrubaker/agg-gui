//! TEMPORARY, env-gated per-section timing for `paint_subtree_backbuffered`.
//!
//! Enabled only when the `AGG_PAINT_TIMING` environment variable is set (read
//! once via a `OnceLock`, so the per-frame check is a single atomic load). It
//! attributes the cost of a CPU LCD backbuffer raster + blit frame to its
//! individual sections (widget paint, LcdGfxCtx/LcdBuffer setup, plane update,
//! blit, overlay) so we can see where a dirty-strip edit re-raster spends its
//! time. Off by default; imposes no cost on normal frames.
//!
//! This is a measurement aid owned by `paint.rs` — remove it once the
//! strip-repaint cost investigation there is settled.

use std::sync::OnceLock;
use std::time::Instant;

/// Whether `AGG_PAINT_TIMING` was set in the environment. Cached on first read.
pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("AGG_PAINT_TIMING").is_some())
}

/// Start a stopwatch when timing is on; `None` (a no-op) when it is off.
pub(crate) fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// Milliseconds elapsed since `t`, or `0.0` when timing is off (`t` is `None`).
pub(crate) fn ms(t: &Option<Instant>) -> f64 {
    t.as_ref()
        .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Per-frame section timings for one `paint_subtree_backbuffered` call.
#[derive(Default)]
pub(crate) struct PaintTiming {
    pub widget_paint_ms: f64,
    pub lcd_ctx_setup_ms: f64,
    pub plane_update_ms: f64,
    pub make_mut_ms: f64,
    pub rowcopy_ms: f64,
    pub blit_ms: f64,
    pub overlay_ms: f64,
    /// Did this frame re-raster at all (vs. a pure cache blit)?
    pub did_raster: bool,
    /// Did the raster take the dirty-line-strip fast path?
    pub is_strip: bool,
    /// Top-down physical row range repainted on a strip frame.
    pub strip_rows: Option<(u32, u32)>,
    /// Total physical height of the backbuffer, for the strip-narrowness check.
    pub h_phys: u32,
}

/// Per-frame section timings for one `TextArea::paint` call. Emitted (by the
/// caller) only on dirty-line-strip frames so the output stays manageable — a
/// strip edit should touch ~1-2 lines, so this exposes where the ~34ms
/// `widget_paint` cost the backbuffer probe attributed to `TextArea::paint`
/// actually lands.
#[derive(Default)]
pub(crate) struct TextAreaPaintTiming {
    pub head_ms: f64,
    pub bg_fill_ms: f64,
    pub clip_ms: f64,
    pub state_clone_ms: f64,
    pub sel_loop_ms: f64,
    pub text_loop_ms: f64,
    pub tail_ms: f64,
    /// Lines that passed culling and actually painted in the text loop.
    pub painted_lines: u32,
    /// Cumulative time inside `paint_highlighted_line` for those lines.
    pub hl_ms: f64,
}

impl TextAreaPaintTiming {
    pub fn emit(&self) {
        eprintln!(
            "[ta paint strip] head={:.3} bg_fill={:.3} clip={:.3} state_clone={:.3} \
             sel_loop={:.3} text_loop={:.3} (painted_lines={} hl={:.3}) tail={:.3}",
            self.head_ms,
            self.bg_fill_ms,
            self.clip_ms,
            self.state_clone_ms,
            self.sel_loop_ms,
            self.text_loop_ms,
            self.painted_lines,
            self.hl_ms,
            self.tail_ms,
        );
    }
}

impl PaintTiming {
    pub fn emit(&self) {
        let kind = if !self.did_raster {
            "blit-only"
        } else if self.is_strip {
            "strip"
        } else {
            "full"
        };
        let rows = match self.strip_rows {
            Some((s, e)) => format!("{s}..{e} of {} ({} rows)", self.h_phys, e.saturating_sub(s)),
            None => format!("(full {} rows)", self.h_phys),
        };
        eprintln!(
            "[paint {kind}] widget_paint={:.3} lcd_ctx_setup={:.3} plane_update={:.3} \
             (make_mut={:.3} rowcopy={:.3}) blit={:.3} overlay={:.3} rows={rows}",
            self.widget_paint_ms,
            self.lcd_ctx_setup_ms,
            self.plane_update_ms,
            self.make_mut_ms,
            self.rowcopy_ms,
            self.blit_ms,
            self.overlay_ms,
        );
    }
}
