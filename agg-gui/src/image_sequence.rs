//! `ImageSequence` — the frames and per-frame times of an animated image.
//!
//! Port of agg-sharp's `agg/Image/ImageSequence.cs` (the frame list, its
//! timing, `GetImageIndexByTime`) plus the animated-GIF branch of
//! `DataConverters2D/ImageIO.LoadImageData(Stream, ImageSequence)`, which
//! reads each frame's delay. `MarkdownView` plays a sequence for animated
//! GIF images (`widgets/markdown/animated_image.rs`, the port of
//! `ResponsiveImageSequenceWidget`'s timing); hosts can also build one
//! themselves and hand it to [`crate::widgets::MarkdownView`] through
//! `with_image_sequence_provider`.
//!
//! Frames are tightly-packed RGBA8, top row first, the same layout the
//! markdown image provider uses. GIF decoding goes through the `image`
//! crate's `gif` codec (already an agg-gui dependency), which composites
//! every frame onto the full logical screen, so all frames share one size.

use std::sync::Arc;

/// One frame of a sequence: RGBA8 pixels, top row first.
#[derive(Clone, Debug)]
pub struct SequenceFrame {
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// An animated image: frames with a time in milliseconds each.
#[derive(Clone, Debug)]
pub struct ImageSequence {
    frames: Vec<SequenceFrame>,
    frame_times_ms: Vec<i32>,
    /// C#'s `SecondsPerFrame`: the animation tick, used when no frame
    /// times exist and as the playback widget's update interval.
    seconds_per_frame: f64,
}

impl Default for ImageSequence {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageSequence {
    /// An empty sequence ticking at C#'s default 30 frames per second.
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            frame_times_ms: Vec::new(),
            seconds_per_frame: 1.0 / 30.0,
        }
    }

    /// `AddImage`: appends a frame shown for `frame_time_ms`, clamped to at
    /// least 1 ms as C# does (a zero GIF delay becomes 1 ms).
    pub fn add_image(&mut self, data: Vec<u8>, width: u32, height: u32, frame_time_ms: i32) {
        self.frames.push(SequenceFrame {
            data: Arc::new(data),
            width,
            height,
        });
        self.frame_times_ms.push(frame_time_ms.max(1));
    }

    pub fn frames(&self) -> &[SequenceFrame] {
        &self.frames
    }

    pub fn frame_times_ms(&self) -> &[i32] {
        &self.frame_times_ms
    }

    pub fn num_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn seconds_per_frame(&self) -> f64 {
        self.seconds_per_frame
    }

    pub fn set_seconds_per_frame(&mut self, seconds: f64) {
        self.seconds_per_frame = seconds;
    }

    /// `FramesPerSecond` = 1 / `SecondsPerFrame`.
    pub fn frames_per_second(&self) -> f64 {
        1.0 / self.seconds_per_frame
    }

    /// Width of the union of every frame's bounds (C#'s `Width`).
    pub fn width(&self) -> u32 {
        self.frames.iter().map(|f| f.width).max().unwrap_or(0)
    }

    /// Height of the union of every frame's bounds (C#'s `Height`).
    pub fn height(&self) -> u32 {
        self.frames.iter().map(|f| f.height).max().unwrap_or(0)
    }

    /// Total length in seconds: the sum of the frame times, or
    /// `frames × SecondsPerFrame` when there are none.
    pub fn time(&self) -> f64 {
        if !self.frame_times_ms.is_empty() {
            let total: i32 = self.frame_times_ms.iter().sum();
            total as f64 / 1000.0
        } else {
            self.frames.len() as f64 * self.seconds_per_frame
        }
    }

    /// `GetImageIndexByTime`: the frame showing `seconds` into the sequence.
    /// Past the end it falls back to C#'s frames-per-second rounding,
    /// clamped to the last frame. `None` for an empty sequence.
    pub fn image_index_by_time(&self, seconds: f64) -> Option<usize> {
        if self.frames.is_empty() {
            return None;
        }
        let last = self.frames.len() - 1;
        if !self.frame_times_ms.is_empty() {
            // C#'s `(int)(numSeconds * 1000)` truncates toward zero.
            let time_ms = (seconds * 1000.0) as i64;
            let mut total = 0i64;
            for (index, time) in self.frame_times_ms.iter().enumerate() {
                total += i64::from(*time);
                if total > time_ms {
                    return Some(index.min(last));
                }
            }
        }
        // `Math.Round` is banker's rounding; a negative frame (negative time)
        // would index out of range in C#, so it clamps to the first frame.
        let frame = (seconds * self.frames_per_second()).round_ties_even();
        Some(if frame <= 0.0 {
            0
        } else {
            (frame as usize).min(last)
        })
    }

    /// The frame showing `seconds` into the sequence (`GetImageByTime`).
    pub fn image_by_time(&self, seconds: f64) -> Option<&SequenceFrame> {
        self.image_index_by_time(seconds).map(|i| &self.frames[i])
    }

    /// Decode a GIF the way `ImageIO.LoadImageData(Stream, ImageSequence)`
    /// does: an animated GIF keeps each frame with its delay
    /// (centiseconds × 10 ms) and ticks at the shortest delay, never faster
    /// than 10 ms; a single-frame GIF becomes one frame at the default
    /// 1/30 s. `None` when the bytes are not a decodable GIF.
    pub fn load_gif(bytes: &[u8]) -> Option<Self> {
        use image::AnimationDecoder;
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
        let frames = decoder.into_frames().collect_frames().ok()?;
        let mut sequence = Self::new();
        if frames.len() > 1 {
            let mut min_frame_time_ms = i32::MAX;
            for frame in frames {
                let (numer, denom) = frame.delay().numer_denom_ms();
                let frame_delay = i32::try_from(numer / denom.max(1)).unwrap_or(i32::MAX);
                let buffer = frame.into_buffer();
                let (width, height) = buffer.dimensions();
                sequence.add_image(buffer.into_raw(), width, height, frame_delay);
                min_frame_time_ms = 10.max(frame_delay.min(min_frame_time_ms));
            }
            sequence.seconds_per_frame = min_frame_time_ms as f64 / 1000.0;
        } else {
            let frame = frames.into_iter().next()?;
            let buffer = frame.into_buffer();
            let (width, height) = buffer.dimensions();
            sequence.add_image(buffer.into_raw(), width, height, 1000 / 30);
        }
        Some(sequence)
    }

    /// True when `bytes` start with a GIF signature.
    pub fn is_gif(bytes: &[u8]) -> bool {
        bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
    }
}

#[cfg(test)]
#[path = "image_sequence_tests.rs"]
pub(crate) mod tests;
