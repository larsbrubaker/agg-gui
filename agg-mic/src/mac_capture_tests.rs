//! Tests for `mac_capture.rs` without a microphone: what each permission status becomes (a
//! policy block doesn't send the user to System Settings, which can't lift it), and the sink
//! node's AudioBufferList read to mono (planar and interleaved, a buffer shorter than the call or
//! without data). That the engine delivers, and how close to a stop its last chunk lands, can only
//! be seen live (`tests/live_mic.rs`).

use std::ffi::c_void;

use objc2_core_audio_types::{AudioBuffer, AudioBufferList};

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

/// An AudioBufferList with room for two buffers, as the engine hands a planar stereo input.
#[repr(C)]
struct TwoBuffers {
    count: u32,
    buffers: [AudioBuffer; 2],
}

fn buffer(channels: u32, samples: &mut [f32]) -> AudioBuffer {
    AudioBuffer {
        mNumberChannels: channels,
        mDataByteSize: std::mem::size_of_val(samples) as u32,
        mData: samples.as_mut_ptr() as *mut c_void,
    }
}

/// `list` read as the receiver block reads it.
fn read(list: &mut TwoBuffers, frames: u32) -> Vec<f32> {
    // SAFETY: TwoBuffers is laid out as an AudioBufferList of `count` (at most 2) buffers, each
    // pointing at live samples of the size it says.
    unsafe {
        mono_from_buffer_list(
            &*(list as *mut TwoBuffers as *const AudioBufferList),
            frames,
        )
    }
}

#[test]
fn a_planar_buffer_list_averages_to_mono() {
    let (mut left, mut right) = ([1.0f32, 0.5], [0.0f32, -0.5]);
    let mut list = TwoBuffers {
        count: 2,
        buffers: [buffer(1, &mut left), buffer(1, &mut right)],
    };
    assert_eq!(read(&mut list, 2), vec![0.5, 0.0]);
}

#[test]
fn an_interleaved_buffer_list_averages_to_mono() {
    let mut stereo = [1.0f32, 0.0, 0.5, 0.5];
    let mut unused = [0.0f32];
    let mut list = TwoBuffers {
        count: 1,
        buffers: [buffer(2, &mut stereo), buffer(1, &mut unused)],
    };
    assert_eq!(read(&mut list, 2), vec![0.5, 0.5]);
}

#[test]
fn a_buffer_is_read_no_further_than_its_size_says() {
    let (mut left, mut right) = ([1.0f32, 1.0], [1.0f32]);
    let mut list = TwoBuffers {
        count: 2,
        buffers: [buffer(1, &mut left), buffer(1, &mut right)],
    };
    assert_eq!(read(&mut list, 2), vec![1.0, 0.5]);
    // A buffer without data reads as silence.
    list.buffers[1].mData = std::ptr::null_mut();
    assert_eq!(read(&mut list, 2), vec![0.5, 0.5]);
}
