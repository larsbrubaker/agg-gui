//! The data side of a PulseAudio native-protocol connection, as the Linux capture
//! (`linux_capture`) reads it once its record stream is open: each frame is a 20-byte
//! big-endian descriptor (length, channel, 64-bit offset, flags) and a payload. Frames on the
//! command channel carry server commands (handed back whole for the caller to parse); frames on the
//! record stream's channel carry audio, here signed 16-bit little-endian mono, decoded to samples
//! with an odd byte carried into the next frame. Frames for any other channel are skipped. No
//! device or socket here, so it is tested (`pulse_frames_tests.rs`) on every platform.
//!
//! From mattercad-ai's `audio/pulse_frames.rs`, unchanged but for these notes.

use std::io::{self, Read};

/// A frame descriptor's size in bytes.
pub const DESCRIPTOR_SIZE: usize = 20;

/// The channel number of command frames (-1 on the wire).
pub const COMMAND_CHANNEL: u32 = u32::MAX;

// A frame larger than this is not audio a 16 kHz mono stream would send; reading it would only
// allocate whatever a corrupt length says.
const MAX_FRAME_LENGTH: u32 = 16 * 1024 * 1024;

/// What the next frame held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameEvent {
    /// Audio from the record stream.
    Samples(Vec<i16>),
    /// A server command's payload (a tagstruct starting with the command tag).
    Command(Vec<u8>),
}

/// Reads frames for one record stream from a connection.
pub struct RecordFrameReader<R> {
    reader: R,
    channel: u32,
    carry: Option<u8>,
}

impl<R: Read> RecordFrameReader<R> {
    /// Reads the frames of the record stream on `channel` (the create reply's channel).
    pub fn new(reader: R, channel: u32) -> Self {
        Self {
            reader,
            channel,
            carry: None,
        }
    }

    /// The next audio or command frame; frames for other channels are skipped.
    pub fn next_event(&mut self) -> io::Result<FrameEvent> {
        loop {
            let mut descriptor = [0u8; DESCRIPTOR_SIZE];
            self.reader.read_exact(&mut descriptor)?;
            let length =
                u32::from_be_bytes([descriptor[0], descriptor[1], descriptor[2], descriptor[3]]);
            let channel =
                u32::from_be_bytes([descriptor[4], descriptor[5], descriptor[6], descriptor[7]]);
            if length > MAX_FRAME_LENGTH {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("A sound server frame of {length} bytes is too large."),
                ));
            }
            let mut payload = vec![0u8; length as usize];
            self.reader.read_exact(&mut payload)?;
            if channel == COMMAND_CHANNEL {
                return Ok(FrameEvent::Command(payload));
            }
            if channel == self.channel {
                return Ok(FrameEvent::Samples(self.decode(&payload)));
            }
        }
    }

    // Signed 16-bit little-endian samples; a frame may end halfway through one.
    fn decode(&mut self, payload: &[u8]) -> Vec<i16> {
        let mut samples = Vec::with_capacity(payload.len() / 2 + 1);
        let mut bytes = payload;
        if let Some(low) = self.carry.take() {
            match bytes.split_first() {
                Some((&high, rest)) => {
                    samples.push(i16::from_le_bytes([low, high]));
                    bytes = rest;
                }
                None => {
                    self.carry = Some(low);
                    return samples;
                }
            }
        }
        let (pairs, remainder) = bytes.as_chunks::<2>();
        for pair in pairs {
            samples.push(i16::from_le_bytes(*pair));
        }
        if let [odd] = remainder {
            self.carry = Some(*odd);
        }
        samples
    }
}

#[cfg(test)]
#[path = "pulse_frames_tests.rs"]
mod tests;
