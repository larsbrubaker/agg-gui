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
    Button, Conditional, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, ImageView, Label,
    QrView, Rect, Size, Stack, Widget,
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

    // Stream On/Off — a segmented control the desktop owns. The transport's
    // streaming flag is the single source of truth: the buttons read it for
    // their highlight (`with_active_fn`) and flip it on click (`set_streaming`),
    // which also commands the phone over the data channel. Starts Off, so a
    // freshly-connected phone stays idle until the user turns streaming on.
    let stream_row = build_stream_controls(Arc::clone(&font), &handles);

    let inner = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .add(Box::new(StatusText::new(Arc::clone(&font), Rc::clone(&status))))
        .add(Box::new(stream_row))
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

/// Build the `Stream  [On] [Off]` segmented control. Each segment is a subtle,
/// outlined `Button` whose highlight tracks the transport's streaming flag and
/// whose click drives [`ScreenShareTransport::set_streaming`], which both
/// updates that flag and tells the phone to start / stop capturing.
fn build_stream_controls(font: Arc<Font>, handles: &ScreenShareHandles) -> FlexRow {
    let label = Label::new("Stream", Arc::clone(&font)).with_font_size(13.0);

    let on_active = Rc::clone(&handles.transport);
    let on_click = Rc::clone(&handles.transport);
    let on_btn = Button::new("On", Arc::clone(&font))
        .with_font_size(13.0)
        .with_subtle()
        .with_outlined()
        .with_active_fn(move || on_active.borrow().is_streaming())
        .on_click(move || {
            on_click.borrow_mut().set_streaming(true);
            agg_gui::animation::request_draw();
        });

    let off_active = Rc::clone(&handles.transport);
    let off_click = Rc::clone(&handles.transport);
    let off_btn = Button::new("Off", Arc::clone(&font))
        .with_font_size(13.0)
        .with_subtle()
        .with_outlined()
        .with_active_fn(move || !off_active.borrow().is_streaming())
        .on_click(move || {
            off_click.borrow_mut().set_streaming(false);
            agg_gui::animation::request_draw();
        });

    FlexRow::new()
        .with_gap(6.0)
        .add(Box::new(label))
        .add(Box::new(on_btn))
        .add(Box::new(off_btn))
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
        let (connected, streaming) = {
            let mut transport = self.handles.transport.borrow_mut();
            if let Some(frame) = transport.take_latest_frame() {
                *self.handles.frame.borrow_mut() = Some(frame);
            }
            (transport.is_connected(), transport.is_streaming())
        };

        self.qr_visible.set(!connected);
        self.img_visible.set(connected);

        let peer = {
            let transport = self.handles.transport.borrow();
            transport.peer_id().to_string()
        };
        let url = self.handles.phone_url.borrow().clone();
        let next = if connected && streaming {
            "Connected — live view from phone.".to_string()
        } else if connected {
            "Connected — streaming paused (Stream is Off).".to_string()
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
        //
        // The QR↔live-view swap is performed by `sync()`, which runs from
        // `layout()` — and the host only re-runs layout when the invalidation
        // epoch changes. New frames bump that epoch on arrival, but a *stale*
        // timeout flips `is_connected` purely on elapsed time with no arriving
        // frame to trigger a bump. When that leaves the shown state out of sync,
        // force one relayout so the swap happens; `needs_draw` then reports no
        // further work and we fall idle again. This is the only invalidation we
        // raise here — we do NOT repaint continuously while connected.
        if self.handles.transport.borrow().is_connected() != self.img_visible.get() {
            agg_gui::animation::request_draw();
        }
    }

    fn needs_draw(&self) -> bool {
        // Event-driven: do NOT repaint continuously while connected. A redraw is
        // needed only when the shown QR/live state disagrees with the live
        // transport state — i.e. a connect / disconnect / stale transition is
        // pending. Arriving frames drive their own redraw by bumping the
        // invalidation epoch on arrival (see `push_screen_encoded` on wasm and
        // the wake `UserEvent` on native); this covers the staleness flip, which
        // has no incoming frame to trigger it.
        self.handles.transport.borrow().is_connected() != self.img_visible.get()
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        // While the live view is up, wake the loop once at the staleness
        // deadline so a silently-stalled phone reverts to the QR even though no
        // further frames arrive to drive a redraw. Re-armed every frame, so a
        // healthy stream keeps pushing the deadline forward and never fires.
        // (wasm's rAF loop polls `needs_draw` each tick, so it reverts without
        // this; native sleeps between frames and needs the scheduled wakeup.)
        if self.img_visible.get() {
            self.handles.transport.borrow().frame_stale_deadline()
        } else {
            None
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_share::QueuedScreenTransport;
    use std::sync::Mutex;

    const TEST_FONT: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

    /// The view requests a redraw only on a pending connect/disconnect
    /// transition — never continuously while connected. Frame delivery drives
    /// its own redraw via the shells' invalidation, so a steady connected view
    /// must report `needs_draw() == false`.
    #[test]
    fn needs_draw_only_on_transition_not_continuously() {
        let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
        let latest = Arc::new(Mutex::new(None));
        let connected = Arc::new(Mutex::new(true));
        let transport =
            QueuedScreenTransport::new(latest, connected.clone(), "ag-test".to_string());

        let handles = ScreenShareHandles::new();
        *handles.transport.borrow_mut() = Box::new(transport);

        let mut view = screen_share_demo(Arc::clone(&font), handles);
        // Lay out so `sync()` reconciles the shown state to "connected".
        view.layout(Size::new(360.0, 480.0));
        assert!(
            !view.needs_draw(),
            "a connected, reconciled view must not repaint continuously"
        );

        // A dropped connection is a transition the view hasn't shown yet.
        *connected.lock().unwrap() = false;
        assert!(
            view.needs_draw(),
            "a pending connect/disconnect transition must request a redraw"
        );

        // Re-laying out reconciles it (back to the QR) and it falls idle again.
        view.layout(Size::new(360.0, 480.0));
        assert!(
            !view.needs_draw(),
            "the view must fall idle once the transition is reconciled"
        );
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
