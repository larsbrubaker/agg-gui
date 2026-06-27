//! Short, human-readable peer IDs for the QR-code handoff.
//!
//! Six base32-ish characters → ~30 bits of entropy; collisions against
//! peerjs's public cloud over a session are vanishingly unlikely. Kept short
//! so the QR stays small and easy to scan. Adapted from Marbles'
//! `net::peer_id`; the `ag-` prefix marks it as agg-gui in logs.

use rand::Rng;

const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// Generate a fresh peer ID like `ag-mqz4xp`.
pub fn generate() -> String {
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(9);
    id.push_str("ag-");
    for _ in 0..6 {
        let idx = rng.gen_range(0..ALPHABET.len());
        id.push(ALPHABET[idx] as char);
    }
    id
}
