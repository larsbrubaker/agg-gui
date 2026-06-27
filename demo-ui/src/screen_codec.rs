//! Frame-difference codec for the Screen Share demo.
//!
//! A compact, fast, TIFF-ish scheme tuned for streaming a mostly-static desktop:
//! each frame is diffed against the previous one, only changed pixels travel,
//! and those are squeezed hard. Shared by every shell (wasm-clean): the wasm
//! sender encodes, the native + wasm receivers decode.
//!
//! Encode pipeline (per frame):
//!   1. Pixel compare vs. the previous frame → a per-pixel changed/unchanged
//!      mask. A *keyframe* (first frame, a resize, or every `KEY_INTERVAL`
//!      frames) marks every pixel changed and needs no previous frame.
//!   2. RLE the mask as alternating unchanged/changed run lengths (varint).
//!   3. For the changed pixels: separate channels (planar R,G,B,A), delta-encode
//!      each plane, then LZW-compress it.
//!
//! Decode reverses it, applying the changed pixels over a copy of the previous
//! frame. The channel is reliable + ordered, so a keyframe followed by deltas
//! stays in lock-step; a receiver that joins mid-stream simply ignores deltas
//! until the next keyframe.
//!
//! Wire format (one frame, before any data-channel chunking):
//! ```text
//!   u8   version (= 1)
//!   u8   flags        bit0 = keyframe
//!   u32  width  (LE)
//!   u32  height (LE)
//!   u32  mask_len (LE)
//!   ..   mask: varint run lengths, alternating, first run = UNCHANGED count
//!   for each channel R,G,B,A:
//!     u32 plane_len (LE)
//!     ..  LZW( delta( changed bytes of this channel, in scan order ) )
//! ```

const VERSION: u8 = 1;
const FLAG_KEYFRAME: u8 = 0b0000_0001;
/// Force a keyframe at least this often so late joiners recover and drift can't
/// accumulate.
const KEY_INTERVAL: u32 = 60;

// ── Public encoder / decoder ───────────────────────────────────────────────

/// Stateful per-stream encoder. Holds the previous frame to diff against.
pub struct FrameEncoder {
    prev: Vec<u8>,
    dims: (u32, u32),
    have_prev: bool,
    since_key: u32,
}

impl Default for FrameEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameEncoder {
    pub fn new() -> Self {
        Self {
            prev: Vec::new(),
            dims: (0, 0),
            have_prev: false,
            since_key: 0,
        }
    }

    /// Encode `rgba` (top-down RGBA8, `w*h*4` bytes) into one frame packet.
    pub fn encode(&mut self, rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
        let dims_changed = !self.have_prev || self.dims != (w, h);
        let keyframe = dims_changed || self.since_key >= KEY_INTERVAL;
        let packet = if keyframe {
            encode_frame(None, rgba, w, h)
        } else {
            encode_frame(Some(&self.prev), rgba, w, h)
        };
        self.prev.clear();
        self.prev.extend_from_slice(rgba);
        self.dims = (w, h);
        self.have_prev = true;
        self.since_key = if keyframe { 0 } else { self.since_key + 1 };
        packet
    }
}

/// Stateful per-stream decoder. Holds the previous frame to apply deltas over.
pub struct FrameDecoder {
    prev: Vec<u8>,
    dims: (u32, u32),
    have_prev: bool,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self {
            prev: Vec::new(),
            dims: (0, 0),
            have_prev: false,
        }
    }

    /// Decode one frame packet to top-down RGBA8. Returns `None` for a delta
    /// that arrives before a matching keyframe (e.g. joined mid-stream) or a
    /// malformed packet.
    pub fn decode(&mut self, packet: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
        let prev = if self.have_prev {
            Some((self.prev.as_slice(), self.dims))
        } else {
            None
        };
        let (rgba, w, h) = decode_frame(packet, prev)?;
        self.prev.clear();
        self.prev.extend_from_slice(&rgba);
        self.dims = (w, h);
        self.have_prev = true;
        Some((rgba, w, h))
    }
}

// ── Frame encode / decode ──────────────────────────────────────────────────

fn encode_frame(prev: Option<&[u8]>, cur: &[u8], w: u32, h: u32) -> Vec<u8> {
    let px = (w as usize) * (h as usize);
    let keyframe = prev.is_none();

    // 1. changed mask + 2. RLE (alternating runs, first = unchanged).
    let mut mask = Vec::new();
    let mut changed_idx: Vec<usize> = Vec::new();
    {
        let mut run_unchanged = true; // current run kind; first run is unchanged
        let mut run_len: u64 = 0;
        for i in 0..px {
            let changed = match prev {
                None => true,
                Some(p) => cur[i * 4..i * 4 + 4] != p[i * 4..i * 4 + 4],
            };
            if changed {
                changed_idx.push(i);
            }
            let is_unchanged = !changed;
            if is_unchanged == run_unchanged {
                run_len += 1;
            } else {
                write_varint(&mut mask, run_len);
                run_unchanged = !run_unchanged;
                run_len = 1;
            }
        }
        write_varint(&mut mask, run_len);
    }

    // 3. planar channel split → delta → LZW.
    let n = changed_idx.len();
    let mut planes: [Vec<u8>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for c in 0..4 {
        let plane = &mut planes[c];
        plane.reserve(n);
        for &i in &changed_idx {
            plane.push(cur[i * 4 + c]);
        }
        delta_encode(plane);
    }

    let mut out = Vec::new();
    out.push(VERSION);
    out.push(if keyframe { FLAG_KEYFRAME } else { 0 });
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&(mask.len() as u32).to_le_bytes());
    out.extend_from_slice(&mask);
    for plane in &planes {
        let comp = lzw_compress(plane);
        out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        out.extend_from_slice(&comp);
    }
    out
}

fn decode_frame(packet: &[u8], prev: Option<(&[u8], (u32, u32))>) -> Option<(Vec<u8>, u32, u32)> {
    let mut r = Reader::new(packet);
    if r.u8()? != VERSION {
        return None;
    }
    let flags = r.u8()?;
    let keyframe = flags & FLAG_KEYFRAME != 0;
    let w = r.u32()?;
    let h = r.u32()?;
    let px = (w as usize).checked_mul(h as usize)?;
    let bytes = px.checked_mul(4)?;

    // Base buffer the changed pixels are written over.
    let mut out = if keyframe {
        vec![0u8; bytes]
    } else {
        let (p, dims) = prev?;
        if dims != (w, h) || p.len() != bytes {
            return None; // can't apply a delta without the matching previous frame
        }
        p.to_vec()
    };

    // Mask → ordered list of changed pixel indices.
    let mask_len = r.u32()? as usize;
    let mask = r.take(mask_len)?;
    let mut changed_idx: Vec<usize> = Vec::new();
    {
        let mut mr = Reader::new(mask);
        let mut idx: usize = 0;
        let mut unchanged = true;
        while idx < px {
            let run = mr.varint()? as usize;
            if !unchanged {
                for k in 0..run {
                    changed_idx.push(idx + k);
                }
            }
            idx += run;
            unchanged = !unchanged;
        }
        if idx != px {
            return None;
        }
    }
    let n = changed_idx.len();

    // Per-channel: LZW decompress → un-delta → scatter into `out`.
    for c in 0..4 {
        let plane_len = r.u32()? as usize;
        let comp = r.take(plane_len)?;
        let mut plane = lzw_decompress(comp);
        if plane.len() != n {
            return None;
        }
        delta_decode(&mut plane);
        for (k, &i) in changed_idx.iter().enumerate() {
            out[i * 4 + c] = plane[k];
        }
    }

    Some((out, w, h))
}

// ── Delta coding (within a plane, in scan order) ───────────────────────────

fn delta_encode(plane: &mut [u8]) {
    for i in (1..plane.len()).rev() {
        plane[i] = plane[i].wrapping_sub(plane[i - 1]);
    }
}

fn delta_decode(plane: &mut [u8]) {
    for i in 1..plane.len() {
        plane[i] = plane[i].wrapping_add(plane[i - 1]);
    }
}

// ── LZW (fixed 16-bit codes; dictionary stops growing when full) ───────────

fn lzw_compress(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    use std::collections::HashMap;
    let mut dict: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next: u32 = 256;
    let mut codes: Vec<u16> = Vec::new();
    let mut cur = data[0] as u16;
    for &b in &data[1..] {
        if let Some(&code) = dict.get(&(cur, b)) {
            cur = code;
        } else {
            codes.push(cur);
            if next < 65536 {
                dict.insert((cur, b), next as u16);
                next += 1;
            }
            cur = b as u16;
        }
    }
    codes.push(cur);

    let mut out = Vec::with_capacity(codes.len() * 2);
    for code in codes {
        out.extend_from_slice(&code.to_le_bytes());
    }
    out
}

fn lzw_decompress(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() < 2 {
        return Vec::new();
    }
    let mut codes = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        codes.push(u16::from_le_bytes([pair[0], pair[1]]));
    }

    // dict[code] = byte sequence; codes 0..256 are the single bytes.
    let mut dict: Vec<Vec<u8>> = (0..256).map(|b| vec![b as u8]).collect();
    let mut out: Vec<u8> = Vec::new();
    let mut prev = codes[0] as usize;
    if prev >= dict.len() {
        return Vec::new();
    }
    out.extend_from_slice(&dict[prev]);
    for &code in &codes[1..] {
        let code = code as usize;
        let entry = if code < dict.len() {
            dict[code].clone()
        } else if code == dict.len() {
            // Special LZW case: code not yet in the dictionary.
            let mut e = dict[prev].clone();
            e.push(dict[prev][0]);
            e
        } else {
            return out; // corrupt stream; salvage what we have
        };
        out.extend_from_slice(&entry);
        if dict.len() < 65536 {
            let mut newe = dict[prev].clone();
            newe.push(entry[0]);
            dict.push(newe);
        }
        prev = code;
    }
    out
}

// ── varint + reader helpers ────────────────────────────────────────────────

fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }
    fn u32(&mut self) -> Option<u32> {
        let end = self.pos.checked_add(4)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(len)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }
    fn varint(&mut self) -> Option<u64> {
        let mut v: u64 = 0;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            v.extend_from_slice(&rgba);
        }
        v
    }

    #[test]
    fn lzw_roundtrips() {
        for case in [
            vec![],
            vec![0u8; 1000],
            (0..=255u8).cycle().take(5000).collect::<Vec<_>>(),
            b"TOBEORNOTTOBEORTOBEORNOT".to_vec(),
        ] {
            let comp = lzw_compress(&case);
            assert_eq!(lzw_decompress(&comp), case, "lzw mismatch for len {}", case.len());
        }
    }

    #[test]
    fn delta_roundtrips() {
        let mut v: Vec<u8> = (0..200).map(|i| (i * 7 % 256) as u8).collect();
        let orig = v.clone();
        delta_encode(&mut v);
        delta_decode(&mut v);
        assert_eq!(v, orig);
    }

    #[test]
    fn keyframe_then_identity_delta() {
        let mut enc = FrameEncoder::new();
        let mut dec = FrameDecoder::new();
        let frame = solid(8, 6, [10, 20, 30, 255]);

        let k = enc.encode(&frame, 8, 6);
        let (img, w, h) = dec.decode(&k).expect("keyframe decodes");
        assert_eq!((w, h), (8, 6));
        assert_eq!(img, frame);

        // Identical next frame → tiny delta, decodes to the same image.
        let d = enc.encode(&frame, 8, 6);
        assert!(d.len() < k.len(), "identity delta should be smaller than keyframe");
        let (img2, _, _) = dec.decode(&d).expect("delta decodes");
        assert_eq!(img2, frame);
    }

    #[test]
    fn partial_change_delta_reconstructs_exactly() {
        let mut enc = FrameEncoder::new();
        let mut dec = FrameDecoder::new();
        let w = 16;
        let h = 16;
        let mut a = solid(w, h, [0, 0, 0, 255]);
        let _ = dec.decode(&enc.encode(&a, w, h)).unwrap();

        // Change a scattered handful of pixels.
        for &i in &[0usize, 5, 33, 100, 200, 255] {
            a[i * 4] = 200;
            a[i * 4 + 1] = 150;
            a[i * 4 + 2] = 50;
        }
        let d = enc.encode(&a, w, h);
        let (img, _, _) = dec.decode(&d).expect("delta decodes");
        assert_eq!(img, a);
    }

    #[test]
    fn resize_forces_keyframe_and_decodes() {
        let mut enc = FrameEncoder::new();
        let mut dec = FrameDecoder::new();
        let _ = dec.decode(&enc.encode(&solid(4, 4, [1, 2, 3, 4]), 4, 4)).unwrap();
        // Different dimensions must be sent as a keyframe and still decode.
        let big = solid(10, 7, [9, 8, 7, 255]);
        let p = enc.encode(&big, 10, 7);
        let (img, w, h) = dec.decode(&p).expect("resize keyframe decodes");
        assert_eq!((w, h), (10, 7));
        assert_eq!(img, big);
    }

    #[test]
    fn delta_without_keyframe_is_rejected() {
        // A fresh decoder receiving a delta (no prior keyframe) returns None.
        let mut enc = FrameEncoder::new();
        let _key = enc.encode(&solid(4, 4, [0, 0, 0, 255]), 4, 4);
        let delta = enc.encode(&solid(4, 4, [0, 0, 1, 255]), 4, 4);
        assert!(delta[1] & FLAG_KEYFRAME == 0, "second frame is a delta");
        let mut fresh = FrameDecoder::new();
        assert!(fresh.decode(&delta).is_none());
    }
}
