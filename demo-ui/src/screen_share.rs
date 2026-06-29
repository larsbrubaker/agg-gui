//! Screen-share transport — the wasm-clean seam between the demo widget and
//! the platform shells that actually move bytes over WebRTC.
//!
//! Mirrors the Marbles `net::transport` pattern: demo-ui defines a trait and a
//! queue-backed bridge; the shells (`demo-native` with webrtc-rs, `demo-wasm`
//! with PeerJS) inject a concrete transport and push decoded frames in from
//! their async/JS callbacks. No tokio / webrtc / winit here so the module
//! compiles for both native and wasm.
//!
//! A "frame" is a fully-decoded top-down RGBA8 buffer plus dimensions — the
//! exact shape [`agg_gui::ImageView`] paints. Decoding (JPEG → RGBA) happens
//! in the shells; this layer only shuttles the newest frame to the widget.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Top-down RGBA8 pixels + width + height.
pub type ScreenFrame = (Vec<u8>, u32, u32);

/// How long an open channel may go without a fresh frame before we treat the
/// phone as gone and fall back to the QR code.
const CONNECTION_STALE_SECS: f32 = 2.5;

/// Pulled once per paint by the screen-share widget. Implementations must be
/// cheap and non-blocking.
pub trait ScreenShareTransport: 'static {
    /// The newest frame received since the last call, if any. Older frames are
    /// dropped — for a live view only the latest matters.
    fn take_latest_frame(&mut self) -> Option<ScreenFrame>;
    /// Whether a phone is currently connected and sending fresh frames.
    fn is_connected(&self) -> bool;
    /// Short peer id advertised to the phone (encoded into the QR URL).
    fn peer_id(&self) -> &str;
    /// Tell the connected phone whether to actively capture, encode, and
    /// transmit its screen. The desktop owns this switch (the Stream On/Off
    /// control); when off the phone stays connected but does no capture work.
    /// Default no-op so the null transport ignores it.
    fn set_streaming(&mut self, _on: bool) {}
    /// Whether the desktop currently wants the phone to stream. Drives the
    /// Stream On/Off control's highlighted segment. Default `false`.
    fn is_streaming(&self) -> bool {
        false
    }
    /// While actively streaming, the instant after which a silent gap in frames
    /// means the phone is gone and the live view should fall back to the QR.
    /// `None` when not streaming or no frame has been seen yet. The widget uses
    /// this to schedule a single wakeup for that revert (via
    /// [`Widget::next_draw_deadline`]) instead of repainting continuously — so
    /// the host loop can idle between frames. Default `None`.
    fn frame_stale_deadline(&self) -> Option<web_time::Instant> {
        None
    }
}

/// Default transport used before a shell injects a real one (and in tests).
pub struct NullScreenTransport {
    peer_id: String,
}

impl NullScreenTransport {
    pub fn new() -> Self {
        Self {
            peer_id: "offline".to_string(),
        }
    }
}

impl Default for NullScreenTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenShareTransport for NullScreenTransport {
    fn take_latest_frame(&mut self) -> Option<ScreenFrame> {
        None
    }
    fn is_connected(&self) -> bool {
        false
    }
    fn peer_id(&self) -> &str {
        &self.peer_id
    }
    // `set_streaming` / `is_streaming` use the trait defaults: an offline
    // transport has no phone to command and is never streaming.
}

/// A [`ScreenShareTransport`] backed by shared cells the shell writes into from
/// its WebRTC callbacks: a single-slot "latest frame" mailbox plus a connected
/// flag. Both shells use this as the bridge between event-driven networking and
/// the synchronous paint thread.
pub struct QueuedScreenTransport {
    /// Single-slot mailbox: the shell overwrites with each new decoded frame.
    latest: Arc<Mutex<Option<ScreenFrame>>>,
    /// Set true when the data channel opens, false when it closes.
    connected: Arc<Mutex<bool>>,
    /// When the last frame was observed by the widget.
    last_seen: Arc<Mutex<Option<web_time::Instant>>>,
    /// Desktop's Stream On/Off switch. Shared with the shell so it can read the
    /// current intent when a phone connects (to send the initial command) and
    /// so `is_connected` can relax the frame-freshness check while off.
    streaming: Arc<Mutex<bool>>,
    /// Shell-supplied sink that transmits a streaming on/off command to the
    /// phone over the live data channel. `None` until a shell injects one.
    control: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    peer_id: String,
}

impl QueuedScreenTransport {
    pub fn new(
        latest: Arc<Mutex<Option<ScreenFrame>>>,
        connected: Arc<Mutex<bool>>,
        peer_id: String,
    ) -> Self {
        Self {
            latest,
            connected,
            last_seen: Arc::new(Mutex::new(None)),
            streaming: Arc::new(Mutex::new(false)),
            control: None,
            peer_id,
        }
    }

    /// Share the desktop's Stream On/Off flag with the shell. The shell reads
    /// it when a phone connects so a freshly-joined phone immediately respects
    /// the current switch. Pass the same `Arc` the shell's control sink writes.
    pub fn with_streaming_flag(mut self, streaming: Arc<Mutex<bool>>) -> Self {
        self.streaming = streaming;
        self
    }

    /// Install the sink that pushes a streaming command to the phone. Called by
    /// each shell with platform-specific transport (data-channel send on native,
    /// a JS-drained mailbox on wasm).
    pub fn with_control(mut self, control: Arc<dyn Fn(bool) + Send + Sync>) -> Self {
        self.control = Some(control);
        self
    }
}

impl ScreenShareTransport for QueuedScreenTransport {
    fn take_latest_frame(&mut self) -> Option<ScreenFrame> {
        let frame = self.latest.lock().ok().and_then(|mut slot| slot.take());
        if frame.is_some() {
            if let Ok(mut seen) = self.last_seen.lock() {
                *seen = Some(web_time::Instant::now());
            }
        }
        frame
    }

    fn is_connected(&self) -> bool {
        let Ok(connected) = self.connected.lock() else {
            return false;
        };
        if !*connected {
            return false;
        }
        // With streaming off the phone is deliberately idle — no frames are
        // expected, so the freshness check would wrongly drop a healthy
        // connection back to the QR. Trust the channel-open flag instead
        // (closed channels still clear `connected`).
        if !self.streaming.lock().map(|s| *s).unwrap_or(false) {
            return true;
        }
        // Streaming on: before any frame arrives, trust the channel-open flag.
        // Once frames are flowing, require freshness so a stalled/dropped phone
        // reverts to the QR even while the socket is nominally still open.
        match self.last_seen.lock().ok().and_then(|seen| *seen) {
            None => true,
            Some(seen) => seen.elapsed().as_secs_f32() <= CONNECTION_STALE_SECS,
        }
    }

    fn peer_id(&self) -> &str {
        &self.peer_id
    }

    fn set_streaming(&mut self, on: bool) {
        if let Ok(mut s) = self.streaming.lock() {
            *s = on;
        }
        // Clear the staleness clock when turning on so a stream resumed after a
        // pause isn't immediately judged stale by a frame timestamp from before.
        if on {
            if let Ok(mut seen) = self.last_seen.lock() {
                *seen = None;
            }
        }
        if let Some(control) = &self.control {
            control(on);
        }
    }

    fn is_streaming(&self) -> bool {
        self.streaming.lock().map(|s| *s).unwrap_or(false)
    }

    fn frame_stale_deadline(&self) -> Option<web_time::Instant> {
        // Paused phones are deliberately idle — no freshness requirement, so no
        // revert deadline. Mirrors the streaming-off branch of `is_connected`.
        if !self.streaming.lock().map(|s| *s).unwrap_or(false) {
            return None;
        }
        self.last_seen
            .lock()
            .ok()
            .and_then(|seen| *seen)
            .map(|seen| seen + Duration::from_secs_f32(CONNECTION_STALE_SECS))
    }
}

/// Shared cells the screen-share demo widget reads and the platform shells
/// populate. Created in `build_demo_ui`, returned in `DemoHandles`, and handed
/// to the widget builder.
#[derive(Clone)]
pub struct ScreenShareHandles {
    /// Injected transport. Starts as [`NullScreenTransport`]; a shell swaps in a
    /// [`QueuedScreenTransport`] once its networking is up.
    pub transport: Rc<RefCell<Box<dyn ScreenShareTransport>>>,
    /// The URL the QR encodes (e.g. `http://<lan-ip>:<port>/phone.html?host=<id>`).
    /// Filled in by the shell after it knows its peer id / server address.
    pub phone_url: Rc<RefCell<String>>,
    /// Latest decoded frame, shared with the widget's `ImageView` source.
    pub frame: Rc<RefCell<Option<ScreenFrame>>>,
}

impl ScreenShareHandles {
    pub fn new() -> Self {
        Self {
            transport: Rc::new(RefCell::new(Box::new(NullScreenTransport::new()))),
            phone_url: Rc::new(RefCell::new(String::new())),
            frame: Rc::new(RefCell::new(None)),
        }
    }
}

impl Default for ScreenShareHandles {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_transport_is_offline() {
        let mut t = NullScreenTransport::new();
        assert!(!t.is_connected());
        assert!(t.take_latest_frame().is_none());
        assert_eq!(t.peer_id(), "offline");
    }

    #[test]
    fn connected_flag_gates_before_any_frame() {
        let latest = Arc::new(Mutex::new(None));
        let connected = Arc::new(Mutex::new(false));
        let mut t =
            QueuedScreenTransport::new(latest.clone(), connected.clone(), "ag-test".to_string());

        // No open channel → not connected.
        assert!(!t.is_connected());

        // Channel open but no frame yet → trust the open flag.
        *connected.lock().unwrap() = true;
        assert!(t.is_connected());
        assert_eq!(t.peer_id(), "ag-test");

        // Streaming defaults off; the switch flips it (no control sink needed).
        assert!(!t.is_streaming());
        t.set_streaming(true);
        assert!(t.is_streaming());
        // Still connected: no frame has arrived, so the freshness check is
        // moot whether streaming is on or off.
        assert!(t.is_connected());
        t.set_streaming(false);
        assert!(!t.is_streaming());
    }

    #[test]
    fn frame_stale_deadline_only_while_streaming_after_a_frame() {
        let latest = Arc::new(Mutex::new(None));
        let connected = Arc::new(Mutex::new(true));
        let mut t =
            QueuedScreenTransport::new(latest.clone(), connected, "ag-test".to_string());

        // Not streaming → no revert deadline (paused phones are idle).
        assert!(t.frame_stale_deadline().is_none());
        t.set_streaming(true);
        // Streaming but no frame seen yet → still none.
        assert!(t.frame_stale_deadline().is_none());

        // Once a frame has been observed, a revert deadline exists so the view
        // can schedule its fall-back-to-QR wakeup.
        *latest.lock().unwrap() = Some((vec![0, 0, 0, 0], 1, 1));
        let _ = t.take_latest_frame();
        assert!(t.frame_stale_deadline().is_some());
    }

    #[test]
    fn takes_newest_frame_then_empties() {
        let latest = Arc::new(Mutex::new(None));
        let connected = Arc::new(Mutex::new(true));
        let mut t =
            QueuedScreenTransport::new(latest.clone(), connected.clone(), "ag-test".to_string());

        // Shell pushes a frame; widget takes it exactly once.
        *latest.lock().unwrap() = Some((vec![1, 2, 3, 4], 1, 1));
        let f = t.take_latest_frame().expect("frame present");
        assert_eq!(f, (vec![1, 2, 3, 4], 1, 1));
        assert!(t.take_latest_frame().is_none());

        // Having seen a frame keeps it connected (fresh).
        assert!(t.is_connected());
    }
}
