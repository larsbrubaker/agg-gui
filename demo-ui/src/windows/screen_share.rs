//! Phone Screen Share demo.
//!
//! Shows a QR code that a phone scans to load the same agg-gui WASM app; once
//! the phone connects over WebRTC (signaling logic adapted from Marbles), it
//! streams its rendered canvas back as JPEG frames and the QR is replaced by a
//! live view. If the connection drops or goes stale the demo falls back to the
//! QR so it can be re-established.
//!
//! This file is platform-agnostic: it only consumes the
//! [`ScreenShareHandles`](crate::screen_share::ScreenShareHandles) seam. The
//! actual sockets live in `demo-native` / `demo-wasm`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Conditional, DrawCtx, Event, EventResult, FlexColumn, Font, ImageView, QrView, Rect, Size,
    Stack, Widget,
};

use crate::screen_share::ScreenShareHandles;

/// Build the demo window content.
pub fn screen_share_demo(font: Arc<Font>, handles: ScreenShareHandles) -> Box<dyn Widget> {
    let qr_visible = Rc::new(Cell::new(true));
    let img_visible = Rc::new(Cell::new(false));
    let status = Rc::new(RefCell::new(String::from("Starting…")));

    // QR pulls its URL live from the shared cell (filled in by the shell once
    // it knows its peer id / server address) and hides itself once connected.
    let qr = QrView::new("")
        .with_text_source(Rc::clone(&handles.phone_url))
        .with_visibility(Rc::clone(&qr_visible));

    // Live view of the phone, shown only while connected.
    let image = ImageView::new(Arc::clone(&font), Rc::clone(&handles.frame))
        .with_placeholder("Waiting for video…");
    let image_gated = Conditional::new(Rc::clone(&img_visible), Box::new(image));

    // QR and live view overlay the same area; only one is visible at a time.
    let stage = Stack::new()
        .add(Box::new(qr))
        .add(Box::new(image_gated));

    let inner = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .add(Box::new(StatusText::new(Arc::clone(&font), Rc::clone(&status))))
        .add_flex(Box::new(stage), 1.0);

    Box::new(ScreenShareView {
        bounds: Rect::default(),
        children: vec![Box::new(inner)],
        handles,
        qr_visible,
        img_visible,
        status,
    })
}

/// Bookkeeping wrapper. Owns no visuals of its own — each frame it reads the
/// transport, moves the newest frame into the `ImageView` source, and flips the
/// QR / live-view visibility + status text. The inner `FlexColumn` does all the
/// layout and painting through the framework's normal child traversal.
struct ScreenShareView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // exactly one: the inner column
    handles: ScreenShareHandles,
    qr_visible: Rc<Cell<bool>>,
    img_visible: Rc<Cell<bool>>,
    status: Rc<RefCell<String>>,
}

impl ScreenShareView {
    /// Pull transport state and update shared cells. Runs every paint.
    fn sync(&mut self) {
        let connected = {
            let mut transport = self.handles.transport.borrow_mut();
            if let Some(frame) = transport.take_latest_frame() {
                *self.handles.frame.borrow_mut() = Some(frame);
            }
            transport.is_connected()
        };

        self.qr_visible.set(!connected);
        self.img_visible.set(connected);

        let peer = {
            let transport = self.handles.transport.borrow();
            transport.peer_id().to_string()
        };
        let url = self.handles.phone_url.borrow().clone();
        let next = if connected {
            "Connected — live view from phone.".to_string()
        } else if url.is_empty() {
            format!("Waiting for server… (peer {peer})")
        } else {
            format!("Scan to connect a phone  ·  peer {peer}")
        };
        *self.status.borrow_mut() = next;
    }
}

impl Widget for ScreenShareView {
    fn type_name(&self) -> &'static str {
        "ScreenShareView"
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
        self.sync();
        if let Some(child) = self.children.first_mut() {
            let s = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
            self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
            s
        } else {
            self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
            available
        }
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Inner column paints through the framework's child traversal.
    }

    fn needs_draw(&self) -> bool {
        // While a phone is connected, keep repainting so new frames are pulled
        // promptly. `is_connected` also applies the stale-timeout, so once the
        // phone stops we fall idle and the QR reappears.
        self.handles.transport.borrow().is_connected()
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A single line of status text pulled live from a shared string. Sized to one
/// line so the QR / live-view area gets the rest of the window.
struct StatusText {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    font: Arc<Font>,
    text: Rc<RefCell<String>>,
}

impl StatusText {
    fn new(font: Arc<Font>, text: Rc<RefCell<String>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            text,
        }
    }
}

impl Widget for StatusText {
    fn type_name(&self) -> &'static str {
        "StatusText"
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
        let h = 20.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(13.0);
        ctx.set_fill_color(v.text_color);
        let text = self.text.borrow();
        if let Some(m) = ctx.measure_text(&text) {
            let ty = self.bounds.height * 0.5 - (m.ascent - m.descent) * 0.5;
            ctx.fill_text(&text, 0.0, ty.max(0.0));
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
