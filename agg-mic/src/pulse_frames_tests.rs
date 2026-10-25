//! Tests of `pulse_frames.rs`, the Linux capture's reading of a PulseAudio
//! native-protocol connection, over hand-recorded frames: audio for the record stream decodes to
//! samples (an odd byte carried across frames), command frames come back whole, frames for other
//! streams are skipped, and an oversized length is refused. From mattercad-ai's
//! `tests/ai/rust_only_pulse_frames_tests.rs`.

use std::io::{Cursor, ErrorKind};

use super::{FrameEvent, RecordFrameReader, COMMAND_CHANNEL};

// One frame as the server sends it: the big-endian descriptor, then the payload.
fn frame(channel: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&channel.to_be_bytes());
    bytes.extend_from_slice(&0u64.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn record_frames_decode_to_samples_across_odd_splits() {
    let mut recorded = Vec::new();
    // 1, -2 and 0x1234 little-endian, split so the last sample straddles two frames.
    recorded.extend(frame(7, &[0x01, 0x00, 0xFE, 0xFF, 0x34]));
    recorded.extend(frame(7, &[0x12]));
    let mut reader = RecordFrameReader::new(Cursor::new(recorded), 7);

    assert_eq!(
        reader.next_event().unwrap(),
        FrameEvent::Samples(vec![1, -2])
    );
    assert_eq!(
        reader.next_event().unwrap(),
        FrameEvent::Samples(vec![0x1234])
    );
    assert_eq!(
        reader.next_event().unwrap_err().kind(),
        ErrorKind::UnexpectedEof
    );
}

#[test]
fn commands_come_back_and_other_streams_are_skipped() {
    let mut recorded = Vec::new();
    recorded.extend(frame(3, &[0x10, 0x00]));
    recorded.extend(frame(COMMAND_CHANNEL, &[0x4C, 0, 0, 0, 65]));
    recorded.extend(frame(7, &[0x10, 0x00]));
    let mut reader = RecordFrameReader::new(Cursor::new(recorded), 7);

    assert_eq!(
        reader.next_event().unwrap(),
        FrameEvent::Command(vec![0x4C, 0, 0, 0, 65])
    );
    assert_eq!(reader.next_event().unwrap(), FrameEvent::Samples(vec![16]));
}

#[test]
fn an_oversized_frame_is_refused() {
    let mut recorded = Vec::new();
    recorded.extend_from_slice(&u32::MAX.to_be_bytes());
    recorded.extend_from_slice(&[0u8; 16]);
    let mut reader = RecordFrameReader::new(Cursor::new(recorded), 7);

    assert_eq!(
        reader.next_event().unwrap_err().kind(),
        ErrorKind::InvalidData
    );
}
