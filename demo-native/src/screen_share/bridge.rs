//! WebRTC bridge — peerjs signaling client + webrtc-rs `RTCPeerConnection`.
//!
//! Adapted from Marbles' `webrtc_bridge`. The desktop is the *answerer*: it
//! registers a peer id with peerjs's cloud (`wss://0.peerjs.com/peerjs`), waits
//! for the phone's `OFFER`, answers, exchanges ICE, and then receives the
//! phone's screen as **binary JPEG frames** on the data channel. Each frame is
//! decoded to RGBA8 and dropped into the single-slot mailbox the widget reads.
//!
//! See <https://github.com/peers/peerjs-server> for the envelope grammar.

use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use super::chunk;

/// Top-down RGBA8 + width + height. Matches `demo_ui::ScreenFrame`.
type ScreenFrame = (Vec<u8>, u32, u32);

/// Shared cells the bridge writes into; the widget reads them via
/// `QueuedScreenTransport`.
pub struct BridgeChannels {
    pub latest: Arc<Mutex<Option<ScreenFrame>>>,
    pub connected: Arc<Mutex<bool>>,
    /// Nudges the winit event loop so a connect / new frame is painted even
    /// when the app is otherwise idle.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl Clone for BridgeChannels {
    fn clone(&self) -> Self {
        Self {
            latest: self.latest.clone(),
            connected: self.connected.clone(),
            wake: self.wake.clone(),
        }
    }
}

/// Spawn the signaling bridge on the shared runtime. Detached; lives for the
/// process lifetime.
pub fn spawn(runtime: &Runtime, peer_id: String, channels: BridgeChannels) -> JoinHandle<()> {
    runtime.spawn(async move {
        if let Err(err) = run_signaling(peer_id, channels).await {
            eprintln!("screen-share signaling exited: {err}");
        }
    })
}

async fn run_signaling(peer_id: String, channels: BridgeChannels) -> Result<(), String> {
    use rand::Rng;

    let token: String = {
        let mut rng = rand::thread_rng();
        (0..16)
            .map(|_| {
                let n: u8 = rng.gen_range(0..36);
                if n < 10 {
                    (b'0' + n) as char
                } else {
                    (b'a' + (n - 10)) as char
                }
            })
            .collect()
    };

    let url =
        format!("wss://0.peerjs.com/peerjs?key=peerjs&id={peer_id}&token={token}&version=1.5.4");
    eprintln!("screen-share signaling: opening {url}");

    let (ws_stream, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("ws connect: {e}"))?;
    let (mut ws_write, mut ws_read) = ws_stream.split();

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let writer_task = tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            if let Err(err) = ws_write.send(Message::Text(text)).await {
                eprintln!("screen-share signaling: ws send failed: {err}");
                break;
            }
        }
    });

    // peerjs evicts peers that stop heartbeating (~30s); its own client sends
    // one every 5s, so we mirror that.
    let hb_tx = out_tx.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            tick.tick().await;
            if hb_tx.send(r#"{"type":"HEARTBEAT"}"#.to_string()).is_err() {
                break;
            }
        }
    });

    let active_pc: Arc<Mutex<Option<Arc<RTCPeerConnection>>>> = Arc::new(Mutex::new(None));

    while let Some(msg) = ws_read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Err(err) =
                    handle_envelope(text.as_ref(), &peer_id, &out_tx, &active_pc, &channels).await
                {
                    eprintln!("screen-share signaling: dispatch error: {err}");
                }
            }
            Ok(Message::Close(_)) => break,
            Err(err) => return Err(format!("ws read: {err}")),
            _ => {}
        }
    }

    drop(out_tx);
    heartbeat_task.abort();
    let _ = writer_task.await;
    Ok(())
}

async fn handle_envelope(
    text: &str,
    self_id: &str,
    out: &mpsc::UnboundedSender<String>,
    active_pc: &Arc<Mutex<Option<Arc<RTCPeerConnection>>>>,
    channels: &BridgeChannels,
) -> Result<(), String> {
    let env: Value = serde_json::from_str(text).map_err(|e| format!("json: {e}"))?;
    let kind = env.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "OPEN" => {
            eprintln!("screen-share signaling: registered as {self_id}");
            Ok(())
        }
        "OFFER" => {
            let src = env
                .get("src")
                .and_then(Value::as_str)
                .ok_or("OFFER missing src")?
                .to_string();
            let payload = env.get("payload").ok_or("OFFER missing payload")?;
            let sdp = payload
                .get("sdp")
                .and_then(|s| s.get("sdp"))
                .and_then(Value::as_str)
                .ok_or("OFFER missing payload.sdp.sdp")?
                .to_string();
            let connection_id = payload
                .get("connectionId")
                .and_then(Value::as_str)
                .ok_or("OFFER missing connectionId")?
                .to_string();

            eprintln!("screen-share signaling: OFFER from {src} (cid {connection_id})");
            let pc =
                build_peer_connection(src.clone(), connection_id.clone(), out.clone(), channels)
                    .await?;

            let offer = RTCSessionDescription::offer(sdp).map_err(|e| format!("offer parse: {e}"))?;
            pc.set_remote_description(offer)
                .await
                .map_err(|e| format!("set_remote: {e}"))?;
            let answer = pc
                .create_answer(None)
                .await
                .map_err(|e| format!("create_answer: {e}"))?;
            pc.set_local_description(answer.clone())
                .await
                .map_err(|e| format!("set_local: {e}"))?;

            let envelope = json!({
                "type": "ANSWER",
                "dst": src,
                "payload": {
                    "sdp": { "sdp": answer.sdp, "type": "answer" },
                    "type": "data",
                    "connectionId": connection_id,
                    "browser": "agg-gui-rust",
                },
            });
            out.send(envelope.to_string())
                .map_err(|e| format!("send answer: {e}"))?;
            eprintln!("screen-share signaling: ANSWER sent to {src}");

            *active_pc.lock().unwrap() = Some(pc);
            Ok(())
        }
        "CANDIDATE" => {
            let payload = env.get("payload").ok_or("CANDIDATE missing payload")?;
            let cand = payload.get("candidate").ok_or("CANDIDATE missing inner")?;
            let candidate = cand
                .get("candidate")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let sdp_mid = cand.get("sdpMid").and_then(Value::as_str).map(String::from);
            let sdp_mline_index = cand
                .get("sdpMLineIndex")
                .and_then(Value::as_u64)
                .map(|n| n as u16);
            let init = RTCIceCandidateInit {
                candidate,
                sdp_mid,
                sdp_mline_index,
                username_fragment: cand
                    .get("usernameFragment")
                    .and_then(Value::as_str)
                    .map(String::from),
            };
            let pc_opt = active_pc.lock().unwrap().clone();
            if let Some(pc) = pc_opt {
                pc.add_ice_candidate(init)
                    .await
                    .map_err(|e| format!("add_ice: {e}"))?;
            } else {
                eprintln!("screen-share signaling: CANDIDATE dropped — no active pc yet");
            }
            Ok(())
        }
        "LEAVE" | "EXPIRE" => {
            *active_pc.lock().unwrap() = None;
            *channels.connected.lock().unwrap() = false;
            (channels.wake)();
            Ok(())
        }
        other => {
            eprintln!("screen-share signaling: ignoring {other}");
            Ok(())
        }
    }
}

async fn build_peer_connection(
    remote: String,
    connection_id: String,
    out: mpsc::UnboundedSender<String>,
    channels: &BridgeChannels,
) -> Result<Arc<RTCPeerConnection>, String> {
    let api = APIBuilder::new().build();
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:global.stun.twilio.com:3478".to_string(),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let pc = Arc::new(
        api.new_peer_connection(config)
            .await
            .map_err(|e| format!("new_peer_connection: {e}"))?,
    );

    // Connection / ICE state tracing so we can see exactly where a failing
    // handshake stalls (the phone only reports its own conn-open=false).
    pc.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        eprintln!("screen-share signaling: pc state → {s}");
        Box::pin(async {})
    }));
    pc.on_ice_connection_state_change(Box::new(move |s: RTCIceConnectionState| {
        eprintln!("screen-share signaling: ice state → {s}");
        Box::pin(async {})
    }));

    // Our own ICE candidates → peerjs CANDIDATE envelopes back to the phone.
    let out_for_ice = out.clone();
    let remote_for_ice = remote.clone();
    let cid_for_ice = connection_id.clone();
    pc.on_ice_candidate(Box::new(move |c| {
        let out = out_for_ice.clone();
        let remote = remote_for_ice.clone();
        let cid = cid_for_ice.clone();
        Box::pin(async move {
            let Some(c) = c else {
                return;
            };
            let init = match c.to_json() {
                Ok(i) => i,
                Err(err) => {
                    eprintln!("screen-share signaling: ice to_json: {err}");
                    return;
                }
            };
            let envelope = json!({
                "type": "CANDIDATE",
                "dst": remote,
                "payload": {
                    "candidate": {
                        "candidate": init.candidate,
                        "sdpMid": init.sdp_mid,
                        "sdpMLineIndex": init.sdp_mline_index,
                        "usernameFragment": init.username_fragment,
                    },
                    "type": "data",
                    "connectionId": cid,
                },
            });
            let _ = out.send(envelope.to_string());
        })
    }));

    // Inbound data channel: the phone is the offerer and creates the channel.
    let channels_for_dc = channels.clone();
    pc.on_data_channel(Box::new(move |dc| {
        let channels = channels_for_dc.clone();
        Box::pin(async move {
            eprintln!("screen-share signaling: data channel opening ({})", dc.label());

            let on_open = channels.clone();
            dc.on_open(Box::new(move || {
                let channels = on_open.clone();
                Box::pin(async move {
                    *channels.connected.lock().unwrap() = true;
                    (channels.wake)();
                    eprintln!("screen-share signaling: data channel open");
                })
            }));

            let on_close = channels.clone();
            dc.on_close(Box::new(move || {
                let channels = on_close.clone();
                Box::pin(async move {
                    *channels.connected.lock().unwrap() = false;
                    (channels.wake)();
                    eprintln!("screen-share signaling: data channel closed");
                })
            }));

            // Full-resolution frames exceed the data channel's per-message
            // limit, so the phone splits each codec packet into ordered chunks
            // (see `chunk`). Reassemble, then run the frame-diff decoder.
            let on_msg = channels.clone();
            let reasm = Arc::new(Mutex::new(chunk::Reassembler::default()));
            let decoder = Arc::new(Mutex::new(demo_ui::FrameDecoder::new()));
            dc.on_message(Box::new(move |msg: DataChannelMessage| {
                let channels = on_msg.clone();
                let reasm = reasm.clone();
                let decoder = decoder.clone();
                Box::pin(async move {
                    let complete = reasm.lock().ok().and_then(|mut r| r.push(&msg.data));
                    // Any inbound chunk means the phone is alive — keep
                    // connected so the stale-timeout doesn't trip mid-frame.
                    *channels.connected.lock().unwrap() = true;
                    if let Some(packet) = complete {
                        let decoded = decoder.lock().ok().and_then(|mut d| d.decode(&packet));
                        if let Some((rgba, w, h)) = decoded {
                            if let Ok(mut slot) = channels.latest.lock() {
                                *slot = Some((rgba, w, h));
                            }
                            (channels.wake)();
                        }
                        // `None` = a delta that arrived before its keyframe
                        // (e.g. we joined mid-stream); wait for the next key.
                    }
                })
            }));
        })
    }));

    Ok(pc)
}
