//! Frame chunking protocol for screen-share frames.
//!
//! Full-resolution encoded frames (JPEG/PNG) routinely exceed a WebRTC data
//! channel's per-message size limit, so the phone splits each frame into
//! ordered chunks and the receiver reassembles them. The channel is reliable +
//! ordered, so chunks arrive in order and a frame is complete once its last
//! chunk lands.
//!
//! Wire format per chunk — a 12-byte little-endian header then the payload:
//! ```text
//!   offset 0  u32  frame_seq    monotonically increasing per frame
//!   offset 4  u16  chunk_index  0-based
//!   offset 6  u16  chunk_count  total chunks in this frame
//!   offset 8  u32  total_len    total encoded image length (bytes)
//!   offset 12 ..   payload      this chunk's slice of the encoded image
//! ```
//! The TypeScript sender in `demo/src/app.ts` must match this layout exactly.

/// Bytes of header preceding each chunk's payload.
pub const HEADER_LEN: usize = 12;

/// Reassembles ordered chunks into complete encoded-image buffers.
#[derive(Default)]
pub struct Reassembler {
    seq: u32,
    count: u16,
    received: u16,
    buf: Vec<u8>,
    active: bool,
}

impl Reassembler {
    /// Feed one received chunk. Returns the full encoded image when its final
    /// chunk completes the frame, otherwise `None`.
    pub fn push(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < HEADER_LEN {
            return None;
        }
        let seq = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let idx = u16::from_le_bytes([data[4], data[5]]);
        let count = u16::from_le_bytes([data[6], data[7]]);
        let total = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let payload = &data[HEADER_LEN..];

        if idx == 0 {
            // Start of a new frame.
            self.seq = seq;
            self.count = count;
            self.received = 0;
            self.buf = Vec::with_capacity(total);
            self.active = true;
        }

        // Ignore stray chunks that don't belong to the frame we're building
        // (e.g. we connected mid-frame and haven't seen an idx==0 yet).
        if !self.active || seq != self.seq {
            return None;
        }

        self.buf.extend_from_slice(payload);
        self.received += 1;

        if self.count == 0 || self.received >= self.count {
            self.active = false;
            return Some(std::mem::take(&mut self.buf));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u32, idx: u16, count: u16, total: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&seq.to_le_bytes());
        v.extend_from_slice(&idx.to_le_bytes());
        v.extend_from_slice(&count.to_le_bytes());
        v.extend_from_slice(&total.to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn reassembles_in_order() {
        let mut r = Reassembler::default();
        assert!(r.push(&frame(1, 0, 3, 6, &[1, 2])).is_none());
        assert!(r.push(&frame(1, 1, 3, 6, &[3, 4])).is_none());
        let out = r.push(&frame(1, 2, 3, 6, &[5, 6])).expect("complete");
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn single_chunk_frame_completes_immediately() {
        let mut r = Reassembler::default();
        let out = r.push(&frame(7, 0, 1, 3, &[9, 9, 9])).expect("complete");
        assert_eq!(out, vec![9, 9, 9]);
    }

    #[test]
    fn ignores_chunks_before_first_frame_start() {
        let mut r = Reassembler::default();
        // Joining mid-frame: a non-zero index with no prior start is dropped.
        assert!(r.push(&frame(2, 1, 2, 4, &[1, 2])).is_none());
        // Next full frame still reassembles cleanly.
        assert!(r.push(&frame(3, 0, 2, 4, &[1, 2])).is_none());
        let out = r.push(&frame(3, 1, 2, 4, &[3, 4])).expect("complete");
        assert_eq!(out, vec![1, 2, 3, 4]);
    }
}
