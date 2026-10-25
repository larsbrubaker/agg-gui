//! Streaming resampling of the microphone's audio to the model's 16 kHz,
//! for the engine (`engine.rs`), which gets it in ~250 ms pushes at the
//! device's rate (native, and Firefox's device-rate path on the web).
//!
//! whistle's `audio::resample` converts a whole clip: samples outside the
//! slice count as zero and its phase starts at the slice, so resampling
//! each push on its own attenuates every chunk boundary (a click every
//! 250 ms) and rounds each push's length. [`Resampler`] keeps the input it
//! still needs and the position of its next output across pushes, so audio
//! cut into pushes of any size comes out exactly as the whole clip does.
//! The filter is whistle's (`whistle-rust/crates/whistle-rust/src/audio/
//! resample.rs`, private there): a Kaiser-windowed sinc with the same taps,
//! cutoff and β, so the output matches its one-shot resampler
//! (`resample_tests.rs`). Each output waits for the input half a kernel past
//! it (3 ms at 48 kHz); [`Resampler::finish`] gives the last of it, the
//! input after the end counting as zero, as whistle's does.

/// The model's sample rate.
pub(crate) const MODEL_RATE: u32 = 16_000;

/// Zero crossings of the low-pass kernel on each side, in samples of the
/// lower rate (whistle's `HALF_TAPS`).
const HALF_TAPS: f64 = 48.0;
/// The kernel's cutoff as a fraction of the lower rate's Nyquist.
const CUTOFF: f64 = 0.94;
/// Kaiser β, for about 85 dB of stop-band attenuation.
const BETA: f64 = 8.6;
/// Above this many output phases the kernel is evaluated per output sample
/// instead of tabulated.
const MAX_PHASES: u64 = 1024;

/// The zeroth-order modified Bessel function of the first kind.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0f64, 1.0f64, x * x / 4.0);
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

struct Kernel {
    /// Cutoff in cycles per input sample.
    fc: f64,
    /// Half-width in input samples.
    half_width: f64,
    i0_beta: f64,
}

impl Kernel {
    /// The `len` weights for input samples `-reach..` around fractional
    /// input position `x` (0..1), normalised to unit sum.
    fn weights(&self, x: f64, reach: i64, len: usize, out: &mut Vec<f64>) {
        out.clear();
        for j in 0..len {
            let d = x - (j as i64 - reach) as f64;
            let u = d / self.half_width;
            let w = if u.abs() >= 1.0 {
                0.0
            } else {
                let arg = 2.0 * self.fc * d;
                let sinc = if arg == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
                };
                sinc * bessel_i0(BETA * (1.0 - u * u).sqrt()) / self.i0_beta
            };
            out.push(w);
        }
        let sum: f64 = out.iter().sum();
        if sum != 0.0 {
            out.iter_mut().for_each(|w| *w /= sum);
        }
    }
}

/// One rate conversion, fed in pushes (see the module docs).
pub(crate) struct Resampler {
    from: u64,
    to: u64,
    kernel: Kernel,
    /// Input samples on each side of an output's position.
    reach: i64,
    /// The weights per output phase, when there are few enough phases.
    table: Option<Vec<Vec<f64>>>,
    phases: u64,
    scratch: Vec<f64>,
    /// The input still needed; `held[0]` is input sample `base`.
    held: Vec<f32>,
    base: u64,
    /// Input samples pushed in all.
    received: u64,
    /// The next output sample's index.
    next: u64,
}

impl Resampler {
    /// A conversion from `from` to `to` Hz; `None` for a rate outside
    /// whistle's `SUPPORTED_RATES` or equal rates (nothing to convert).
    pub fn new(from: u32, to: u32) -> Option<Resampler> {
        let rates = whistle::audio::SUPPORTED_RATES;
        if from == to || !rates.contains(&from) || !rates.contains(&to) {
            return None;
        }
        let (from, to) = (u64::from(from), u64::from(to));
        let lower = from.min(to) as f64;
        let kernel = Kernel {
            fc: CUTOFF * lower / 2.0 / from as f64,
            half_width: HALF_TAPS * from as f64 / lower,
            i0_beta: bessel_i0(BETA),
        };
        let reach = kernel.half_width.ceil() as i64;
        let len = (2 * reach + 1) as usize;
        let phases = to / gcd(from, to);
        let table = (phases <= MAX_PHASES).then(|| {
            (0..phases)
                .map(|p| {
                    let mut w = Vec::new();
                    kernel.weights((p * from % to) as f64 / to as f64, reach, len, &mut w);
                    w
                })
                .collect()
        });
        Some(Resampler {
            from,
            to,
            kernel,
            reach,
            table,
            phases,
            scratch: Vec::new(),
            held: Vec::new(),
            base: 0,
            received: 0,
            next: 0,
        })
    }

    /// The input sample output `i` is centred on.
    fn centre(&self, i: u64) -> i64 {
        (i * self.from / self.to) as i64
    }

    /// Output `i`, from the held input (zero outside what was pushed).
    fn output(&mut self, i: u64) -> f32 {
        let len = (2 * self.reach + 1) as usize;
        if self.table.is_none() {
            let x = (i * self.from % self.to) as f64 / self.to as f64;
            self.kernel.weights(x, self.reach, len, &mut self.scratch);
        }
        let weights = match &self.table {
            Some(table) => &table[(i % self.phases) as usize],
            None => &self.scratch,
        };
        let first = self.centre(i) - self.reach;
        let mut acc = 0.0f64;
        for (j, &w) in weights.iter().enumerate() {
            let at = first + j as i64;
            if at >= self.base as i64 {
                if let Some(&s) = self.held.get((at - self.base as i64) as usize) {
                    acc += f64::from(s) * w;
                }
            }
        }
        acc as f32
    }

    /// Drop the input no output from `next` on needs.
    fn trim(&mut self) {
        let needed = (self.centre(self.next) - self.reach).max(0) as u64;
        if needed > self.base {
            let drop = ((needed - self.base) as usize).min(self.held.len());
            self.held.drain(..drop);
            self.base += drop as u64;
        }
    }

    /// Take `samples` and return every output whose input is all in.
    pub fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        self.held.extend_from_slice(samples);
        self.received += samples.len() as u64;
        let mut out = Vec::new();
        while self.centre(self.next) + self.reach < self.received as i64 {
            out.push(self.output(self.next));
            self.next += 1;
        }
        self.trim();
        out
    }

    /// The end of the audio: the outputs still owed, `round(input * to /
    /// from)` in all, as whistle's one-shot resampler makes. The resampler
    /// is empty afterwards.
    pub fn finish(&mut self) -> Vec<f32> {
        let total = (self.received * self.to + self.from / 2) / self.from;
        let mut out = Vec::new();
        while self.next < total {
            out.push(self.output(self.next));
            self.next += 1;
        }
        self.held.clear();
        self.base = self.received;
        out
    }
}

/// A session's audio brought to [`MODEL_RATE`], whatever rate each push
/// comes at.
#[derive(Default)]
pub(crate) struct ModelRate {
    /// The conversion in progress and its source rate.
    resampler: Option<(u32, Resampler)>,
}

impl ModelRate {
    /// `samples` at `rate` Hz, as the model's samples ready so far; `None`
    /// for a rate that can't be converted. A push at a new rate ends the
    /// old rate's audio first (its tail leads the output).
    pub fn push(&mut self, rate: u32, samples: Vec<f32>) -> Option<Vec<f32>> {
        let current = self.resampler.as_ref().map(|(r, _)| *r);
        let mut out = Vec::new();
        if current.is_some() && current != Some(rate) {
            out = self.finish();
        }
        if rate == MODEL_RATE {
            if out.is_empty() {
                return Some(samples);
            }
            out.extend(samples);
            return Some(out);
        }
        if self.resampler.is_none() {
            self.resampler = Some((rate, Resampler::new(rate, MODEL_RATE)?));
        }
        if let Some((_, resampler)) = self.resampler.as_mut() {
            out.extend(resampler.push(&samples));
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
