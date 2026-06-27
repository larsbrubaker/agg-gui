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
            peer_id,
        }
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
        // Before any frame arrives, trust the channel-open flag. Once frames
        // are flowing, require freshness so a stalled/dropped phone reverts to
        // the QR even while the socket is nominally still open.
        match self.last_seen.lock().ok().and_then(|seen| *seen) {
            None => true,
            Some(seen) => seen.elapsed().as_secs_f32() <= CONNECTION_STALE_SECS,
        }
    }

    fn peer_id(&self) -> &str {
        &self.peer_id
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
