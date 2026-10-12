//! Playback of animated (GIF / image-sequence) images in `MarkdownView`.
//!
//! The timing half of agg-sharp's `Gui/ResponsiveImageSequenceWidget.cs`,
//! which MarkdigAgg's `ImageLinkSimpleX` uses for every Markdown image:
//! the playback time wraps at the sequence's length, the shown frame is
//! `ImageSequence.GetImageIndexByTime`, and a draw is wanted only when that
//! frame index changes (C#'s `updateEvent.ShouldDraw`).
//!
//! C# defers its `UiThread.SetInterval` until `OnLoad` and disposes it in
//! `OnClosed`. agg-gui has no interval registry: a widget re-arms a
//! scheduled draw ([`crate::animation::request_draw_after`]) from `paint`,
//! as the spinner does. So the clock starts on the first paint after the
//! image has loaded, re-arms only while the image is painted inside the
//! clip, and a view removed from the tree is never painted again, so it
//! schedules nothing. `MarkdownView::needs_draw` / `next_draw_deadline`
//! report a due frame so retained parents (cached window layers) repaint.
//!
//! Sizing is unchanged from static images (`layout.rs`'s
//! `inline_image_size`): fit to the available width, never above natural
//! size, keeping the aspect ratio — C#'s responsive scale
//! `min(DeviceScale, width / imageWidth)` in agg-gui's logical units.

use std::cell::Cell;
use std::time::Duration;

use web_time::Instant;

use crate::image_sequence::ImageSequence;

/// One Markdown image's playback state; lives on its image-cache entry.
#[derive(Default)]
pub(super) struct AnimationClock {
    /// When playback started: the first paint after load. `None` = not yet.
    started: Cell<Option<Instant>>,
    /// The frame index last painted.
    shown: Cell<Option<usize>>,
}

impl AnimationClock {
    /// The frame index last painted.
    pub(super) fn shown_frame(&self) -> Option<usize> {
        self.shown.get()
    }

    /// Whether playback has started (the image has loaded and painted).
    pub(super) fn is_running(&self) -> bool {
        self.started.get().is_some()
    }

    /// Seconds into the sequence at `now`, wrapped like C#'s
    /// `while (Time > 0 && currentTime > Time) currentTime -= Time;`,
    /// which leaves the time in `(0, Time]`.
    fn playback_time(sequence: &ImageSequence, start: Instant, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(start).as_secs_f64();
        let total = sequence.time();
        if total > 0.0 && elapsed > total {
            let wrapped = elapsed % total;
            if wrapped == 0.0 {
                total
            } else {
                wrapped
            }
        } else {
            elapsed
        }
    }

    /// The frame to paint now. Starts the clock on the first call, records
    /// the frame as shown and, when `visible`, schedules the draw that shows
    /// the next frame.
    pub(super) fn frame_for_paint(&self, sequence: &ImageSequence, visible: bool) -> usize {
        let now = crate::clock::now();
        let start = match self.started.get() {
            Some(start) => start,
            None => {
                self.started.set(Some(now));
                now
            }
        };
        let t = Self::playback_time(sequence, start, now);
        let index = sequence.image_index_by_time(t).unwrap_or(0);
        self.shown.set(Some(index));
        if visible && sequence.num_frames() > 1 {
            crate::animation::request_draw_after_tagged(
                Self::delay_to_next_frame(sequence, t),
                "markdown.animated_image",
            );
        }
        index
    }

    /// True when playback has started and the frame due now differs from
    /// the one last painted.
    pub(super) fn frame_due(&self, sequence: &ImageSequence) -> bool {
        let (Some(start), Some(shown)) = (self.started.get(), self.shown.get()) else {
            return false;
        };
        let t = Self::playback_time(sequence, start, crate::clock::now());
        sequence.image_index_by_time(t) != Some(shown)
    }

    /// The instant the next frame is due, once playback has started.
    pub(super) fn next_deadline(&self, sequence: &ImageSequence) -> Option<Instant> {
        let start = self.started.get()?;
        if sequence.num_frames() < 2 {
            return None;
        }
        let now = crate::clock::now();
        let t = Self::playback_time(sequence, start, now);
        Some(now + Self::delay_to_next_frame(sequence, t))
    }

    /// Time from `t` to the next frame boundary (wrapping to the loop start
    /// after the last frame), never shorter than the sequence's tick —
    /// C#'s `Animation` updates at `FramesPerSecond`, so frames shorter than
    /// a tick (a 0 ms GIF delay, clamped to 1 ms) are stepped over as there.
    fn delay_to_next_frame(sequence: &ImageSequence, t: f64) -> Duration {
        let mut boundary_ms = 0i64;
        let mut next = None;
        let time_ms = (t * 1000.0) as i64;
        for time in sequence.frame_times_ms() {
            boundary_ms += i64::from(*time);
            if boundary_ms > time_ms {
                next = Some(boundary_ms as f64 / 1000.0);
                break;
            }
        }
        let until = match next {
            Some(boundary) => boundary - t,
            None => {
                let total = sequence.time();
                if total > t {
                    total - t
                } else {
                    sequence.seconds_per_frame()
                }
            }
        };
        Duration::from_secs_f64(until.max(sequence.seconds_per_frame()).max(0.0))
    }
}

impl super::ImageEntry {
    /// The entry's animated sequence, once it has loaded as one.
    pub(super) fn sequence(&self) -> Option<std::sync::Arc<ImageSequence>> {
        let state = self.state.lock().ok()?;
        match &*state {
            super::ImageState::Ready { image, .. } => image.sequence.clone(),
            _ => None,
        }
    }
}

impl super::MarkdownView {
    /// Supply an animated-image source, asked before
    /// [`with_image_provider`](Self::with_image_provider): it receives an
    /// image URL/path from the Markdown source and returns its frames
    /// (e.g. [`ImageSequence::load_gif`] over a local GIF), or `None` to fall
    /// back to the static provider. MatterCAD's
    /// `MarkdownWidget.RetrieveImageSquenceAsync` fills an `ImageSequence`
    /// the same way. Remote `.gif` images animate without a provider.
    pub fn with_image_sequence_provider(
        mut self,
        provider: impl Fn(&str) -> Option<ImageSequence> + 'static,
    ) -> Self {
        self.sequence_provider = Some(Box::new(provider));
        self
    }

    /// Whether the image at `url` is animating (loaded, painted, and has
    /// more than one frame).
    pub fn is_image_animating(&self, url: &str) -> bool {
        self.image_cache.iter().any(|entry| {
            entry.url == url
                && entry.animation.is_running()
                && entry.sequence().is_some_and(|s| s.num_frames() > 1)
        })
    }

    /// The image at `url`'s frame index as last painted, if it is animated.
    pub fn image_frame_index(&self, url: &str) -> Option<usize> {
        let entry = self.image_cache.iter().find(|entry| entry.url == url)?;
        entry.sequence()?;
        entry.animation.shown_frame()
    }

    /// The provided sequence for `url` as image pixels (first frame shown
    /// while not animating).
    pub(super) fn provided_sequence(&self, url: &str) -> Option<super::ImagePixels> {
        let sequence = self.sequence_provider.as_ref().and_then(|p| p(url))?;
        pixels_of_sequence(sequence)
    }

    /// True when an animated image's due frame differs from the painted one.
    pub(super) fn animation_frame_due(&self) -> bool {
        self.image_cache.iter().any(|entry| {
            entry
                .sequence()
                .is_some_and(|sequence| entry.animation.frame_due(&sequence))
        })
    }

    /// The earliest instant an animated image's next frame is due.
    pub(super) fn next_animation_deadline(&self) -> Option<Instant> {
        self.image_cache
            .iter()
            .filter_map(|entry| {
                let sequence = entry.sequence()?;
                entry.animation.next_deadline(&sequence)
            })
            .min()
    }
}

/// Image pixels for a sequence: its first frame, with the frames attached
/// when there is more than one (a one-frame sequence draws as a still).
pub(crate) fn pixels_of_sequence(sequence: ImageSequence) -> Option<super::ImagePixels> {
    let first = sequence.frames().first()?.clone();
    let animated = sequence.num_frames() > 1;
    Some(super::ImagePixels {
        data: first.data,
        width: first.width,
        height: first.height,
        sequence: animated.then(|| std::sync::Arc::new(sequence)),
    })
}
