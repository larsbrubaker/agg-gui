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
/// 1. "Take Screenshot" button + "Capture continuously" toggle.
/// 2. Below: export actions for the most recent screenshot.
/// 3. Below: a preview panel that displays the most recent capture (via
///    `DrawCtx::draw_image_rgba`) or a "No screenshot taken yet." placeholder.
///
/// The platform harness watches `screenshot_request`. On a capture frame it
/// renders twice: pass 1 paints an empty preview pane (so the captured pixels
/// don't nest a stale previous capture), then reads the GL back buffer into
/// `screenshot_image`; pass 2 re-renders with the fresh image visible.
/// `screenshot_capturing` is the flag the harness sets around pass 1 so the
/// preview pane knows to hide itself.
pub fn screenshot_demo(
    font: Arc<Font>,
    screenshot_request: Rc<Cell<bool>>,
    screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    screenshot_capturing: Rc<Cell<bool>>,
) -> Box<dyn Widget> {
    // "Capture continuously" is window-local: a sub-widget re-arms the
    // screenshot request every layout while the checkbox is on, and
    // independently issues a repaint request so the host loop keeps running.
    // Nothing about continuous capture leaks to DemoHandles / the harness.
    let continuous = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "Capture the current frame and display it below.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );

    let continuous_cb = Rc::clone(&continuous);

    col.push(
        Box::new(action_row(
            Arc::clone(&font),
            Rc::clone(&screenshot_request),
            Rc::clone(&screenshot_image),
        )),
        0.0,
    );

    let checkbox_row = FlexRow::new().with_gap(10.0).add(Box::new(
        agg_gui::Checkbox::new(
            "Capture continuously",
            Arc::clone(&font),
            continuous_cb.get(),
        )
        .with_font_size(12.0)
        .with_v_anchor(VAnchor::CENTER)
        .with_state_cell(Rc::clone(&continuous_cb)),
    ));
    col.push(Box::new(checkbox_row), 0.0);

    // Drives continuous-capture cadence: while enabled AND this widget is
    // being painted (i.e. the Screenshot window is visible), arm the
    // screenshot_request and request another frame.  When the window is
    // collapsed/hidden, this widget's paint isn't called -> no re-arm -> loop
    // goes idle naturally.  When the checkbox is off, no request is armed.
    col.push(
        Box::new(ContinuousCapture {
            bounds: Rect::default(),
            children: Vec::new(),
            enabled: Rc::clone(&continuous),
            request: Rc::clone(&screenshot_request),
        }),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Preview pane.  During a capture pass (screenshot_capturing=true) it
    // paints only the frame background so the captured pixels don't include
    // the preview-of-a-preview from the previous frame.
    col.push(
        Box::new(ImageView {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            source: Rc::clone(&screenshot_image),
            capturing: Rc::clone(&screenshot_capturing),
        }),
        1.0,
    );

    Box::new(col)
}

fn action_row(
    font: Arc<Font>,
    screenshot_request: Rc<Cell<bool>>,
    screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
) -> FlexRow {
    let req_for_btn = Rc::clone(&screenshot_request);
    let download_image = Rc::clone(&screenshot_image);
    let download_enabled = Rc::clone(&screenshot_image);
    let copy_image = Rc::clone(&screenshot_image);
    let copy_enabled = Rc::clone(&screenshot_image);

    FlexRow::new()
        .with_gap(10.0)
        .add(Box::new(
            Button::new("\u{F030}  Take Screenshot", Arc::clone(&font))
                .with_font_size(12.0)
                .with_v_anchor(VAnchor::CENTER)
                .on_click(move || {
                    req_for_btn.set(true);
                    agg_gui::animation::request_tick();
                }),
        ))
        .add(Box::new(
            Button::new(EXPORT_BUTTON_LABEL, Arc::clone(&font))
                .with_font_size(12.0)
                .with_v_anchor(VAnchor::CENTER)
                .with_enabled_fn(move || download_enabled.borrow().is_some())
                .on_click(move || {
                    let img = download_image.borrow();
                    if let Some((pixels, w, h)) = img.as_ref() {
                        if let Err(err) = agg_gui::screenshot::download_rgba_as_png(
                            pixels.as_slice(),
                            *w,
                            *h,
                            "agg-gui-screenshot.png",
                        ) {
                            eprintln!("screenshot download failed: {err}");
                        }
                    }
                }),
        ))
        .add(Box::new(
            Button::new("\u{F0C5}  Copy", Arc::clone(&font))
                .with_font_size(12.0)
                .with_v_anchor(VAnchor::CENTER)
                .with_enabled_fn(move || copy_enabled.borrow().is_some())
                .on_click(move || {
                    let img = copy_image.borrow();
                    if let Some((pixels, w, h)) = img.as_ref() {
                        if let Err(err) =
                            agg_gui::screenshot::copy_rgba_to_clipboard(pixels.as_slice(), *w, *h)
                        {
                            eprintln!("screenshot clipboard copy failed: {err}");
                        }
                    }
                }),
        ))
}

// ── ContinuousCapture: while enabled + painted, drive capture cadence ──
//
// Zero-height widget whose only job is to be in the widget tree so that its
// `paint` runs exactly when the enclosing Screenshot window is visible.  The
// parent Window's `paint` is skipped when collapsed / closed / hidden, which
// transitively skips this widget, which ends the capture loop.
struct ContinuousCapture {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    enabled: Rc<Cell<bool>>,
    request: Rc<Cell<bool>>,
}

impl Widget for ContinuousCapture {
    fn type_name(&self) -> &'static str {
        "ContinuousCapture"
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
    fn show_in_inspector(&self) -> bool {
        false
    }
    fn layout(&mut self, _: Size) -> Size {
        Size::ZERO
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {
        if self.enabled.get() {
            self.request.set(true);
            agg_gui::animation::request_tick();
        }
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── ImageView: paints an `Rc<RefCell<Option<(rgba, w, h)>>>` as an image ─────

struct ImageView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    source: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    /// Set by the platform harness during the FIRST pass of a capture frame.
    /// When true, paint only the background so the captured pixels don't
    /// contain this pane's previous image.
    capturing: Rc<Cell<bool>>,
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

        // During the first render pass of a capture frame, skip painting the
        // image / placeholder so the captured pixels show an empty pane instead
        // of nested screenshots.
        if self.capturing.get() {
            return;
        }

        let src = self.source.borrow();
        if let Some((pixels, iw, ih)) = src.as_ref() {
            // Shrink-to-fit preserving aspect ratio.
            let iwf = *iw as f64;
            let ihf = *ih as f64;
            let scale = (w / iwf).min(h / ihf);
            let dw = iwf * scale;
            let dh = ihf * scale;
            let dx = (w - dw) * 0.5;
            let dy = (h - dh) * 0.5;
            // Arc-keyed draw: the GL backend caches textures by the Arc's
            // pointer identity, so a new screenshot (new Arc) evicts the prior
            // entry correctly.
            ctx.draw_image_rgba_arc(pixels, *iw, *ih, dx, dy, dw, dh);

            // Outline in the text color so the image boundary is always visible
            // against the neutral pane.
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
