//! Tests for `mixdown.rs` on every platform: buffers (planar or interleaved, as an
//! AudioBufferList holds them) average to mono, and a WASAPI mix format's bytes (float, 16-, 24-
//! and 32-bit integer, any channel count) decode and average to mono -1..1, with the format told
//! from its tag, bit depth and extensible sub-format.

use super::*;

#[test]
fn planar_buffers_average_to_mono() {
    let left = [1.0, 0.5, -1.0];
    let right = [0.0, 0.5, 1.0];
    assert_eq!(
        mono_from_buffers(&[(&left, 1), (&right, 1)], 3),
        vec![0.5, 0.5, 0.0]
    );
}

#[test]
fn an_interleaved_buffer_averages_to_mono() {
    let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
    assert_eq!(mono_from_buffers(&[(&stereo, 2)], 3), vec![0.5, 0.5, 0.0]);
}

#[test]
fn a_short_buffer_is_read_only_as_far_as_it_goes() {
    // A buffer whose byte size says fewer frames than the call: its frames count as silence
    // past its end rather than being read out of bounds.
    let full = [1.0, 1.0];
    let short = [1.0];
    assert_eq!(
        mono_from_buffers(&[(&full, 1), (&short, 1)], 2),
        vec![1.0, 0.5]
    );
    assert!(mono_from_buffers(&[], 4).iter().all(|&s| s == 0.0));
}

#[test]
fn float_stereo_bytes_average_to_mono() {
    let mut bytes = Vec::new();
    for sample in [1.0f32, 0.0, -0.5, -0.5] {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    assert_eq!(
        mono_from_bytes(&bytes, SampleKind::Float32, 2),
        vec![0.5, -0.5]
    );
}

#[test]
fn integer_bytes_scale_to_minus_one_to_one() {
    let i16_bytes: Vec<u8> = [i16::MIN, 16_384]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    assert_eq!(
        mono_from_bytes(&i16_bytes, SampleKind::Int16, 1),
        vec![-1.0, 0.5]
    );
    // 24-bit packed: -8388608 and 4194304.
    let i24_bytes = [0x00, 0x00, 0x80, 0x00, 0x00, 0x40];
    assert_eq!(
        mono_from_bytes(&i24_bytes, SampleKind::Int24, 1),
        vec![-1.0, 0.5]
    );
    let i32_bytes: Vec<u8> = [i32::MIN, 1 << 30]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    assert_eq!(
        mono_from_bytes(&i32_bytes, SampleKind::Int32, 1),
        vec![-1.0, 0.5]
    );
}

#[test]
fn a_partial_last_frame_is_dropped() {
    let bytes = [0u8; 2 * 2 * 3 + 1];
    assert_eq!(mono_from_bytes(&bytes, SampleKind::Int16, 2).len(), 3);
    assert!(mono_from_bytes(&bytes, SampleKind::Int16, 0).is_empty());
}

#[test]
fn the_sample_kind_comes_from_the_tag_and_bit_depth() {
    assert_eq!(
        sample_kind(WAVE_FORMAT_PCM, 16, None),
        Some(SampleKind::Int16)
    );
    assert_eq!(
        sample_kind(WAVE_FORMAT_PCM, 24, None),
        Some(SampleKind::Int24)
    );
    assert_eq!(
        sample_kind(WAVE_FORMAT_IEEE_FLOAT, 32, None),
        Some(SampleKind::Float32)
    );
    assert_eq!(
        sample_kind(WAVE_FORMAT_EXTENSIBLE, 32, Some(SUBTYPE_IEEE_FLOAT)),
        Some(SampleKind::Float32)
    );
    assert_eq!(
        sample_kind(WAVE_FORMAT_EXTENSIBLE, 32, Some(SUBTYPE_PCM)),
        Some(SampleKind::Int32)
    );
    // 8-bit, 64-bit float and an unknown sub-format are not decoded.
    assert_eq!(sample_kind(WAVE_FORMAT_PCM, 8, None), None);
    assert_eq!(sample_kind(WAVE_FORMAT_IEEE_FLOAT, 64, None), None);
    assert_eq!(sample_kind(WAVE_FORMAT_EXTENSIBLE, 32, Some([7; 16])), None);
    assert_eq!(sample_kind(WAVE_FORMAT_EXTENSIBLE, 32, None), None);
}
