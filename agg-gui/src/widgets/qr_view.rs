//! `QrView` — renders a QR code for an arbitrary string.
//!
//! Adapted from the Marbles project's `qr_widget` into agg-gui proper so any
//! app can show a scannable code (per the project rule: missing widgets are
//! added to agg-gui, not worked around downstream).
//!
//! The encoded text can be fixed (`QrView::new`) or pulled live from a shared
//! `Rc<RefCell<String>>` (`with_text_source`) — handy when the value (e.g. a
//! LAN URL with a freshly-minted peer id) isn't known until after the widget
//! tree is built.  An optional visibility cell lets a parent hide the code
//! once it has served its purpose.
//!
//! Opts into agg-gui's per-widget backbuffer so the modules are rasterised
//! once and reused as a cached bitmap on subsequent frames.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use qrcodegen::{QrCode, QrCodeEcc};

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::widget::Widget;

pub struct QrView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    /// Fixed text; ignored when `text_source` is set.
    text: String,
    /// Optional live text source. When present its value wins over `text`.
    text_source: Option<Rc<RefCell<String>>>,
    /// Last text the cached bitmap was painted for. If the live text diverges,
    /// the cache is invalidated so the next paint re-rasterises.
    rasterised_text: String,
    cache: crate::widget::BackbufferCache,
    /// Optional visibility flag; `None` means always visible.
    visible: Option<Rc<Cell<bool>>>,
    /// Quiet-zone padding as a fraction of the smaller side (default 0.08).
    quiet_zone: f64,
}

impl QrView {
    /// A QR view with fixed text.
    pub fn new<S: Into<String>>(text: S) -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, 160.0, 160.0),
            children: Vec::new(),
            text: text.into(),
            text_source: None,
            rasterised_text: String::new(),
            cache: crate::widget::BackbufferCache::new(),
            visible: None,
            quiet_zone: 0.08,
        }
    }

    /// Pull the encoded text from a shared cell each paint. Updates to the
    /// cell are picked up automatically (the cache invalidates on change).
    pub fn with_text_source(mut self, source: Rc<RefCell<String>>) -> Self {
        self.text_source = Some(source);
        self
    }

    /// Gate visibility on a shared flag (e.g. hide once connected).
    pub fn with_visibility(mut self, visible: Rc<Cell<bool>>) -> Self {
        self.visible = Some(visible);
        self
    }

    pub fn with_quiet_zone(mut self, fraction: f64) -> Self {
        self.quiet_zone = fraction.max(0.0);
        self
    }

    /// Replace the fixed text. No effect on the `text_source` path.
    pub fn set_text(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        self.text.clear();
        self.text.push_str(text);
        self.cache.invalidate();
    }

    /// The text that should be encoded this frame.
    fn current_text(&self) -> String {
        match &self.text_source {
            Some(src) => src.borrow().clone(),
            None => self.text.clone(),
        }
    }

    fn is_shown(&self) -> bool {
        self.visible.as_ref().map(|c| c.get()).unwrap_or(true)
    }
}

impl Widget for QrView {
    fn type_name(&self) -> &'static str {
        "QrView"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        if (bounds.width - self.bounds.width).abs() > 0.5
            || (bounds.height - self.bounds.height).abs() > 0.5
        {
            self.cache.invalidate();
        }
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn hit_test(&self, _local_pos: Point) -> bool {
        false
    }

    fn is_visible(&self) -> bool {
        self.is_shown()
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let text = self.current_text();
        if text != self.rasterised_text {
            self.rasterised_text.clear();
            self.rasterised_text.push_str(&text);
            self.cache.invalidate();
        }

        let w = self.bounds.width;
        let h = self.bounds.height;
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        // The code is drawn into a centred square so it stays scannable even
        // when the host area is non-square.
        let side = w.min(h);
        let ox = (w - side) * 0.5;
        let oy = (h - side) * 0.5;

        // White background behind the (potentially) larger area keeps the
        // quiet zone clean and LCD text caches happy with an opaque dst.
        ctx.set_fill_color(Color::from_rgb8(255, 255, 255));
        ctx.begin_path();
        ctx.rect(ox, oy, side, side);
        ctx.fill();

        let qr = match QrCode::encode_text(&text, QrCodeEcc::Low) {
            Ok(qr) => qr,
            Err(_) => return,
        };
        let modules = qr.size();
        if modules <= 0 {
            return;
        }

        let pad = side * self.quiet_zone;
        let inner = side - 2.0 * pad;
        if inner <= 0.0 {
            return;
        }
        let module_size = inner / modules as f64;
        let origin_x = ox + pad;
        // qrcodegen is row-major top-down; agg-gui is Y-up, so flip the row.
        let origin_y = oy + pad;

        ctx.set_fill_color(Color::from_rgb8(0, 0, 0));
        for j in 0..modules {
            for i in 0..modules {
                if !qr.get_module(i, j) {
                    continue;
                }
                let x = origin_x + i as f64 * module_size;
                let y = origin_y + (modules - 1 - j) as f64 * module_size;
                ctx.begin_path();
                ctx.rect(x, y, module_size + 0.5, module_size + 0.5);
                ctx.fill();
            }
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn backbuffer_cache_mut(&mut self) -> Option<&mut crate::widget::BackbufferCache> {
        // Hidden → drop the cached blit so the framework's invisible-widget
        // branch doesn't composite a stale bitmap.
        if !self.is_shown() {
            return None;
        }
        Some(&mut self.cache)
    }
}
