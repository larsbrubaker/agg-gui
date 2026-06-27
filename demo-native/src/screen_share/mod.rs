//! Native Screen Share wiring: LAN phone server + WebRTC signaling bridge.
//!
//! `start` is called once from `main` after the demo UI is built. It mints a
//! peer id, brings up the LAN HTTP server, injects a real
//! `QueuedScreenTransport` into the demo's screen-share seam, fills in the QR
//! URL, and spawns the signaling bridge on the shared tokio runtime.
//!
//! Everything here is native-only (the WASM shell uses PeerJS in TypeScript).

mod bridge;
mod chunk;
mod peer_id;
mod phone_server;

use std::sync::{Arc, Mutex};

use tokio::runtime::Runtime;

use bridge::BridgeChannels;

/// Detached handle kept alive for the process lifetime (dropping it would not
/// stop the detached tasks, but holding it documents ownership).
pub struct ScreenShare {
    _signaling: tokio::task::JoinHandle<()>,
}

/// Bring up the screen-share transport and wire it into `handles`.
///
/// `wake` nudges the winit event loop so a connect / new frame repaints even
/// when the app is otherwise idle.
pub fn start(
    runtime: &Runtime,
    handles: &demo_ui::ScreenShareHandles,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> ScreenShare {
    let peer_id = peer_id::generate();

    // Bring up the LAN server first so the QR can encode its URL.
    let server_url = match runtime.block_on(phone_server::start()) {
        Ok(server) => server.url,
        Err(err) => {
            eprintln!("screen-share: phone server failed to start: {err}");
            String::new()
        }
    };
    let phone_url = if server_url.is_empty() {
        String::new()
    } else {
        format!("{server_url}?host={peer_id}")
    };
    if !phone_url.is_empty() {
        eprintln!("screen-share: phone URL {phone_url}");
    }

    let latest: Arc<Mutex<Option<demo_ui::ScreenFrame>>> = Arc::new(Mutex::new(None));
    let connected: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    *handles.transport.borrow_mut() = Box::new(demo_ui::QueuedScreenTransport::new(
        latest.clone(),
        connected.clone(),
        peer_id.clone(),
    ));
    *handles.phone_url.borrow_mut() = phone_url;

    let signaling = bridge::spawn(
        runtime,
        peer_id,
        BridgeChannels {
            latest,
            connected,
            wake,
        },
    );

    ScreenShare {
        _signaling: signaling,
    }
}
