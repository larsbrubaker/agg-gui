//! Tests for `mac_capture.rs` without a microphone: what each permission status becomes (a
//! policy block doesn't send the user to System Settings, which can't lift it), and a sink node
//! cycle, as agg-os hands over the AudioBufferList's buffers, delivered as mono (planar and
//! interleaved, a buffer shorter than the call or without data). How agg-os reads the
//! AudioBufferList into those buffers is tested beside it (agg-os's `macos/audio_input/tests.rs`).
//! That the engine delivers, and how close to a stop its last chunk lands, can only be seen live
//! (`tests/live_mic.rs`).

use std::sync::{Arc, Mutex};

use super::*;
use crate::{MAC_PERMISSION_DENIED_MESSAGE, MAC_RESTRICTED_MESSAGE};

#[test]
fn the_default_input_device_is_readable_without_prompting() {
    // A Mac without an input device has none; one with a microphone names it.
    if let Some(device) = default_input_device() {
        assert!(!device.name.is_empty());
    }
}

#[test]
fn an_authorized_microphone_is_not_refused() {
    assert_eq!(refusal(AUTHORIZATION_AUTHORIZED), None);
}

#[test]
fn a_user_refusal_says_where_to_turn_it_on() {
    assert_eq!(
        refusal(AUTHORIZATION_DENIED),
        Some(MicError::PermissionDenied(
            MAC_PERMISSION_DENIED_MESSAGE.to_owned()
        ))
    );
}

#[test]
fn a_policy_block_does_not_send_the_user_to_system_settings() {
    assert_eq!(
        refusal(AUTHORIZATION_RESTRICTED),
        Some(MicError::PermissionDenied(
            MAC_RESTRICTED_MESSAGE.to_owned()
        ))
    );
    assert!(!MAC_RESTRICTED_MESSAGE.contains("System Settings"));
}

/// Delivered chunks: each one's samples and rate.
type Chunks = Vec<(Vec<f32>, u32)>;

/// The chunks `buffers` (one sink node cycle of `frames` frames at 48 kHz) deliver.
fn read(buffers: &[(&[f32], usize)], frames: usize) -> Chunks {
    let chunks: Arc<Mutex<Chunks>> = Arc::default();
    let into = Arc::clone(&chunks);
    let delivery = Delivery::new(
        Box::new(move |samples: &[f32], rate: u32| {
            into.lock().unwrap().push((samples.to_vec(), rate))
        }),
        Box::new(|_| {}),
    );
    let cycle = InputCycle {
        buffers,
        frames,
        sample_rate: 48_000,
    };
    deliver_cycle(&delivery, cycle);
    let delivered = chunks.lock().unwrap().clone();
    delivered
}

#[test]
fn a_planar_buffer_list_averages_to_mono() {
    let (left, right) = ([1.0f32, 0.5], [0.0f32, -0.5]);
    assert_eq!(
        read(&[(&left, 1), (&right, 1)], 2),
        vec![(vec![0.5, 0.0], 48_000)]
    );
}

#[test]
fn an_interleaved_buffer_list_averages_to_mono() {
    let stereo = [1.0f32, 0.0, 0.5, 0.5];
    assert_eq!(read(&[(&stereo, 2)], 2), vec![(vec![0.5, 0.5], 48_000)]);
}

#[test]
fn a_buffer_is_read_no_further_than_its_size_says() {
    let (left, right) = ([1.0f32, 1.0], [1.0f32]);
    assert_eq!(
        read(&[(&left, 1), (&right, 1)], 2),
        vec![(vec![1.0, 0.5], 48_000)]
    );
    // A buffer without data (agg-os hands it over empty) reads as silence.
    assert_eq!(
        read(&[(&left, 1), (&[], 1)], 2),
        vec![(vec![0.5, 0.5], 48_000)]
    );
}
