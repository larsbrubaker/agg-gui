//! The microphone's audio brought to the model's 16 kHz for the engine
//! (`engine.rs`), which gets it in pushes at the device's rate (natively,
//! and on Firefox's device-rate path on the web).
//!
//! Resampling each push on its own would attenuate every boundary (a click
//! every push) and round each push's length, so [`ModelRate`] runs one
//! `whistle::audio::StreamResampler` per session: it keeps the input it
//! still needs across pushes, and the pushes' outputs joined are exactly
//! what whistle's one-shot `audio::resample` makes of the whole clip
//! (`resample_tests.rs`). Each output waits for the input half a kernel past
//! it (3 ms at 48 kHz); [`ModelRate::finish`] gives the last of it. What is
//! left here is the session's side: audio already at the model's rate passes
//! through, and a push at a new rate ends the old rate's audio first.
//!
//! The stateful resampler itself used to live here; whistle-rust took it
//! over as `StreamResampler` (ported from this file), so there is one copy.

use whistle::audio::StreamResampler;

/// The model's sample rate.
pub(crate) const MODEL_RATE: u32 = whistle::audio::SAMPLE_RATE;

/// A session's audio brought to [`MODEL_RATE`], whatever rate each push
/// comes at.
#[derive(Default)]
pub(crate) struct ModelRate {
    /// The conversion in progress and its source rate.
    resampler: Option<(u32, StreamResampler)>,
}

impl ModelRate {
    /// `samples` at `rate` Hz, as the model's samples ready so far; `None`
    /// for a rate that can't be converted, which takes nothing of the push
    /// and leaves the old rate's audio as it was. A push at a new rate ends
    /// the old rate's audio first (its tail leads the output).
    pub fn push(&mut self, rate: u32, samples: Vec<f32>) -> Option<Vec<f32>> {
        let current = self.resampler.as_ref().map(|(r, _)| *r);
        if current == Some(rate) {
            let (_, resampler) = self.resampler.as_mut()?;
            return Some(resampler.push(&samples));
        }
        // The new rate's conversion first: refused, the old rate's tail stays
        // for `finish`.
        let next = if rate == MODEL_RATE {
            None
        } else {
            Some(StreamResampler::new(rate, MODEL_RATE).ok()?)
        };
        let mut out = self.finish();
        match next {
            None => {
                if out.is_empty() {
                    return Some(samples);
                }
                out.extend(samples);
            }
            Some(mut resampler) => {
                out.extend(resampler.push(&samples));
                self.resampler = Some((rate, resampler));
            }
        }
        Some(out)
    }

    /// The end of the session's audio: what the conversion still owes.
    pub fn finish(&mut self) -> Vec<f32> {
        match self.resampler.take() {
            Some((_, mut resampler)) => resampler.finish(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "resample_tests.rs"]
pub(crate) mod tests;
