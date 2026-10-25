//! Tests for `resample.rs`: audio cut into pushes of any size comes out the
//! same as the whole clip resampled at once, and the whole clip comes out as
//! whistle's own one-shot resampler makes it (so chunk boundaries add no
//! clicks), whatever rate changes the session's pushes bring. No model is
//! needed.

use super::*;

/// A deterministic test signal at `rate`: two tones and a little noise,
/// `seconds` long.
fn signal(rate: u32, seconds: f32) -> Vec<f32> {
    let n = (rate as f32 * seconds) as usize;
    let mut seed = 0x2545_f491_u32;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32 - 0.5) * 0.05;
            0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                + 0.3 * (2.0 * std::f32::consts::PI * 3_100.0 * t).sin()
                + noise
        })
        .collect()
}

/// `samples` through a fresh [`ModelRate`] in pushes of `chunk`, then its
/// tail.
fn chunked(samples: &[f32], rate: u32, chunk: usize) -> Vec<f32> {
    let mut model_rate = ModelRate::default();
    let mut out = Vec::new();
    for piece in samples.chunks(chunk) {
        out.extend(model_rate.push(rate, piece.to_vec()).expect("supported"));
    }
    out.extend(model_rate.finish());
    out
}

/// `samples` at `rate` resampled in one push, with its tail.
pub(crate) fn whole(samples: &[f32], rate: u32) -> Vec<f32> {
    chunked(samples, rate, samples.len().max(1))
}

fn max_difference(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "lengths differ");
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

#[test]
fn chunked_audio_resamples_as_the_whole_clip_does() {
    for rate in [48_000, 44_100, 22_050, 8_000] {
        let clip = signal(rate, 1.3);
        let whole = whole(&clip, rate);
        // 128 is a render quantum, 4096 a ScriptProcessor buffer, 12 000 a
        // 250 ms push at 48 kHz; 1 and 997 are worst cases.
        for chunk in [1, 128, 997, 4_096, 12_000] {
            let pieces = chunked(&clip, rate, chunk);
            let diff = max_difference(&pieces, &whole);
            assert!(diff <= 1e-6, "{rate} Hz in {chunk}s: off by {diff}");
        }
    }
}

#[test]
fn the_whole_clip_matches_whistles_resampler() {
    for rate in [48_000, 44_100, 22_050, 8_000] {
        let clip = signal(rate, 1.3);
        let ours = chunked(&clip, rate, 4_000);
        let theirs = whistle::audio::resample(&clip, rate, MODEL_RATE);
        let diff = max_difference(&ours, &theirs);
        assert!(diff <= 1e-5, "{rate} Hz: off by {diff}");
    }
}

#[test]
fn audio_at_the_model_rate_passes_through_untouched() {
    let clip = signal(MODEL_RATE, 0.5);
    let mut model_rate = ModelRate::default();
    assert_eq!(model_rate.push(MODEL_RATE, clip.clone()), Some(clip));
    assert!(model_rate.finish().is_empty());
}

#[test]
fn a_rate_whistle_cannot_convert_is_refused() {
    let mut model_rate = ModelRate::default();
    assert_eq!(model_rate.push(0, vec![0.0; 10]), None);
    assert_eq!(model_rate.push(10_000_000, vec![0.0; 10]), None);
}

#[test]
fn a_change_of_rate_ends_the_old_rates_audio_first() {
    let first = signal(48_000, 0.2);
    let second = signal(MODEL_RATE, 0.2);
    let mut model_rate = ModelRate::default();
    let mut out = model_rate.push(48_000, first.clone()).expect("supported");
    out.extend(
        model_rate
            .push(MODEL_RATE, second.clone())
            .expect("supported"),
    );
    out.extend(model_rate.finish());
    let mut expected = whistle::audio::resample(&first, 48_000, MODEL_RATE);
    expected.extend(&second);
    assert!(max_difference(&out, &expected) <= 1e-5);
}

#[test]
fn nothing_pushed_has_no_tail() {
    let mut model_rate = ModelRate::default();
    assert!(model_rate.finish().is_empty());
    let mut resampler =
        whistle::audio::StreamResampler::new(48_000, MODEL_RATE).expect("supported");
    assert!(resampler.push(&[]).is_empty());
    assert!(resampler.finish().is_empty());
}

#[test]
fn a_push_at_a_rate_that_cannot_be_converted_keeps_the_old_rates_tail() {
    // Refused, it takes nothing of the push, and the old rate's audio still ends as a whole clip
    // of it would.
    let clip = signal(48_000, 0.2);
    let mut model_rate = ModelRate::default();
    let mut out = model_rate.push(48_000, clip.clone()).expect("supported");
    assert_eq!(model_rate.push(0, vec![0.0; 10]), None);
    out.extend(model_rate.finish());
    let expected = whistle::audio::resample(&clip, 48_000, MODEL_RATE);
    assert!(max_difference(&out, &expected) <= 1e-5);
}
