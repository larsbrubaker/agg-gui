//! Screenshot demo window.
//!
//! The widget tree lives in `demo-ui`, while capture orchestration and
//! platform export actions live in `agg_gui::screenshot` so native and WASM
//! shells use the same public library surface.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Label, Rect, Separator, Size,
    VAnchor, Widget,
};

#[cfg(not(target_arch = "wasm32"))]
const EXPORT_BUTTON_LABEL: &str = "\u{F0C7}  Save...";
#[cfg(target_arch = "wasm32")]
const EXPORT_BUTTON_LABEL: &str = "\u{F019}  Download";

// ---------------------------------------------------------------------------
// Screenshot demo
// ---------------------------------------------------------------------------

/// Build the Screenshot demo.  Matches egui's `ScreenshotDemo`:
///
/// 1. Egui wording plus "Take Screenshot" button and "Capture continuously"
///    toggle on one row.
/// 2. Below: agg-gui export actions for the most recent screenshot.
/// 3. Below: a preview panel that displays the most recent capture (via
///    `DrawCtx::draw_captured_screenshot` on GPU-direct backends, or
///    `DrawCtx::draw_image_rgba_arc` on software backends).
///
/// Continuous capture is driven by the platform harness (it reads the
/// shared `screenshot_continuous` cell each frame and re-arms
/// `screenshot_request`).  Driving it from the harness instead of from a
/// child widget makes it robust against the Window's retained backbuffer
/// short-circuiting child paint between captures.
#[allow(clippy::too_many_arguments)]
pub fn screenshot_demo(
    font: Arc<Font>,
    screenshot_request: Rc<Cell<bool>>,
    screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    _screenshot_capturing: Rc<Cell<bool>>,
    screenshot_available: Rc<Cell<bool>>,
    screenshot_save_pending: Rc<Cell<bool>>,
    screenshot_copy_pending: Rc<Cell<bool>>,
    screenshot_continuous: Rc<Cell<bool>>,
) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "This demo showcases how to take screenshots via ViewportCommand::Screenshot.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(
        Box::new(capture_row(
            Arc::clone(&font),
            Rc::clone(&screenshot_request),
            Rc::clone(&screenshot_continuous),
        )),
        0.0,
    );

    col.push(
        Box::new(export_row(
            Arc::clone(&font),
            Rc::clone(&screenshot_available),
            Rc::clone(&screenshot_save_pending),
            Rc::clone(&screenshot_copy_pending),
        )),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(
        Box::new(ImageView {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            source: Rc::clone(&screenshot_image),
            continuous: Rc::clone(&screenshot_continuous),
        }),
        1.0,
    );

    Box::new(col)
}

fn capture_row(
    font: Arc<Font>,
    screenshot_request: Rc<Cell<bool>>,
    continuous: Rc<Cell<bool>>,
) -> FlexRow {
    let req_for_btn = Rc::clone(&screenshot_request);

    FlexRow::new()
        .with_gap(10.0)
        .add(Box::new(
            Button::new("\u{F030}  Take Screenshot", Arc::clone(&font))
                .with_font_size(12.0)
                .with_v_anchor(VAnchor::CENTER)
                .on_click(move || {
                    req_for_btn.set(true);
                    agg_gui::animation::request_draw();
                }),
        ))
        .add(Box::new(
            agg_gui::Checkbox::new("Capture continuously", Arc::clone(&font), continuous.get())
                .with_font_size(12.0)
                .with_v_anchor(VAnchor::CENTER)
                .with_state_cell(Rc::clone(&continuous)),
        ))
}

fn export_row(
    font: Arc<Font>,
    screenshot_available: Rc<Cell<bool>>,
    screenshot_save_pending: Rc<Cell<bool>>,
    screenshot_copy_pending: Rc<Cell<bool>>,
) -> FlexRow {
    // Save / Copy run in event-dispatch with no `DrawCtx` access, so they
    // can't read pixels themselves.  They flip a pending flag; the platform
    // harness drains it post-paint via `DrawCtx::read_captured_screenshot`
    // (or the legacy `screenshot_image` fallback for software backends).
    let save_enabled = Rc::clone(&screenshot_available);
    let copy_enabled = Rc::clone(&screenshot_available);

    FlexRow::new()
        .with_gap(10.0)
        .add(Box::new(
            Button::new(EXPORT_BUTTON_LABEL, Arc::clone(&font))
                .with_font_size(12.0)
                .with_v_anchor(VAnchor::CENTER)
                .with_enabled_fn(move || save_enabled.get())
                .on_click(move || {
                    screenshot_save_pending.set(true);
                    agg_gui::animation::request_draw();
                }),
        ))
        .add(Box::new(
            Button::new("\u{F0C5}  Copy", Arc::clone(&font))
                .with_font_size(12.0)
                .with_v_anchor(VAnchor::CENTER)
                .with_enabled_fn(move || copy_enabled.get())
                .on_click(move || {
                    screenshot_copy_pending.set(true);
                    agg_gui::animation::request_draw();
                }),
        ))
}

// ── ImageView: paints the latest screenshot, GPU-direct or via Vec<u8> ──────

struct ImageView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    source: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    /// Continuous-capture flag.  When set, `needs_draw` returns `true` so
    /// the enclosing Window's retained backbuffer re-rasters every frame —
    /// otherwise the GPU capture texture would update underneath a cached
    /// preview pane that never re-samples it.
    continuous: Rc<Cell<bool>>,
}

impl Widget for ImageView {
    fn type_name(&self) -> &'static str {
        "ImageView"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn needs_draw(&self) -> bool {
        self.continuous.get()
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height.max(120.0));
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Frame: use the app background rather than widget fill so it reads as
        // a neutral preview pane, not a white card, on every theme.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.fill();

        // Helper to compute the shrink-to-fit destination rect and paint
        // the outline.  Used by both the GPU-direct and CPU fallback paths.
        let compute_dst = |iw: u32, ih: u32| -> (f64, f64, f64, f64) {
            let iwf = iw as f64;
            let ihf = ih as f64;
            let scale = (w / iwf).min(h / ihf);
            let dw = iwf * scale;
            let dh = ihf * scale;
            let dx = (w - dw) * 0.5;
            let dy = (h - dh) * 0.5;
            (dx, dy, dw, dh)
        };

        // GPU-direct path: backends that hold the capture as a GPU texture
        // (currently the wgpu backend) sample it straight into the preview
        // pane via `draw_captured_screenshot`.  No CPU readback, no
        // re-upload — critical for continuous capture, where the previous
        // round-trip blew the per-frame budget after 1-3 frames.
        if ctx.has_captured_screenshot() {
            if let Some((iw, ih)) = ctx.captured_screenshot_size() {
                let (dx, dy, dw, dh) = compute_dst(iw, ih);
                if ctx.draw_captured_screenshot(dx, dy, dw, dh) {
                    ctx.set_stroke_color(v.text_color);
                    ctx.set_line_width(1.0);
                    ctx.begin_path();
                    ctx.rect(dx, dy, dw, dh);
                    ctx.stroke();
                    return;
                }
            }
        }

        // Fallback path: software backends still populate the `source` cell
        // with `Arc<Vec<u8>>` and the widget round-trips through
        // `draw_image_rgba_arc`.  Unchanged from the pre-GPU-direct flow.
        let src = self.source.borrow();
        if let Some((pixels, iw, ih)) = src.as_ref() {
            let (dx, dy, dw, dh) = compute_dst(*iw, *ih);
            ctx.draw_image_rgba_arc(pixels, *iw, *ih, dx, dy, dw, dh);
            ctx.set_stroke_color(v.text_color);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rect(dx, dy, dw, dh);
            ctx.stroke();
        } else {
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(13.0);
            ctx.set_fill_color(v.text_dim);
            let msg = "No screenshot taken yet.";
            if let Some(m) = ctx.measure_text(msg) {
                let tx = (w - m.width) * 0.5;
                let ty = (h - (m.ascent - m.descent)) * 0.5;
                ctx.fill_text(msg, tx, ty);
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
    }

    #[test]
    fn screenshot_demo_keeps_egui_capture_row_before_export_actions() {
        let mut demo = screenshot_demo(
            font(),
            Rc::new(Cell::new(false)),
            Rc::new(RefCell::new(None)),
            Rc::new(Cell::new(false)),
            Rc::new(Cell::new(false)),
            Rc::new(Cell::new(false)),
            Rc::new(Cell::new(false)),
            Rc::new(Cell::new(false)),
        );

        demo.layout(Size::new(320.0, 260.0));
        let children = demo.children();

        // Label, capture row, export row, separator, image view.
        assert_eq!(children.len(), 5);
        assert_eq!(children[1].type_name(), "FlexRow");
        assert_eq!(children[2].type_name(), "FlexRow");
        assert_eq!(children[4].type_name(), "ImageView");
    }
}
