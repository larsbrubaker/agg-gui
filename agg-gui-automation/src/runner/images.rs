//! The runner's image searches — the port of `GetCurrentScreen`,
//! `ImageExists` and `WaitForImage` (agg-sharp
//! `GuiAutomation/AutomationRunner.cs`).
//!
//! A search looks for a needle image in a [`SearchRegion`]'s image with
//! [`find_least_squares_match`] under the runner's
//! [`match_limit`](super::AutomationConfig::match_limit). A region carries
//! its own image, covering its rectangle (captured once, on first use, as
//! the current screen cropped to the region); with no region every look
//! takes a fresh capture of the whole window, so a wait sees the UI change.
//! Like the name waits in `named.rs`, the answer is the last look, not the
//! clock: a zero-second wait is a single look.
//!
//! C#'s region captures the *whole* screen on first use, so a region did not
//! limit an image search, and the image gestures, which place the match
//! inside the region, could not know where it was. The crop fixes both.
//! The gestures themselves are `image_gestures.rs`.

use agg_gui::clock;
use agg_gui::Framebuffer;

use super::AutomationRunner;
use crate::driver::UiDriver;
use crate::image_match::find_least_squares_match;
use crate::search_region::{ScreenRectangle, SearchRegion};

/// How long an image wait lets the UI run between looks (C# `Delay(.05)`).
const IMAGE_POLL_SECONDS: f64 = 0.05;

/// A copy of `image`'s pixels (a [`Framebuffer`] is not `Clone`).
pub(super) fn copy_framebuffer(image: &Framebuffer) -> Framebuffer {
    let mut copy = Framebuffer::new(image.width(), image.height());
    copy.pixels_mut().copy_from_slice(image.pixels());
    copy
}

impl AutomationRunner {
    /// C# `GetCurrentScreen`: a copy of the window's last painted frame
    /// (RGBA8, row 0 at the bottom).
    pub fn get_current_screen(&self) -> Framebuffer {
        self.check_not_timed_out();
        copy_framebuffer(self.driver.current_screen())
    }

    /// C# `GetScreenRegion`: the whole window, over a fresh capture.
    pub fn get_screen_region(&self) -> SearchRegion {
        let screen_image = self.get_current_screen();
        let bounds = ScreenRectangle::new(
            0,
            0,
            screen_image.width() as i32,
            screen_image.height() as i32,
        );
        SearchRegion::with_image(screen_image, bounds)
    }

    /// The part of the current screen inside `rect` (Y-down pixels), clipped
    /// to the window; an empty image when they do not overlap.
    pub fn capture_region(&self, rect: ScreenRectangle) -> Framebuffer {
        let screen = self.get_current_screen();
        let (width, height) = (screen.width() as i32, screen.height() as i32);
        let Some(clip) =
            ScreenRectangle::intersection(rect, ScreenRectangle::new(0, 0, width, height))
        else {
            return Framebuffer::new(0, 0);
        };
        let crop_width = (clip.right - clip.left) as usize;
        let crop_height = (clip.bottom - clip.top) as usize;
        let mut crop = Framebuffer::new(crop_width as u32, crop_height as u32);
        // Rows are stored bottom-up: the crop's row 0 is the screen's row
        // `height - clip.bottom`.
        let first_row = (height - clip.bottom) as usize;
        let screen_row_bytes = width as usize * 4;
        let crop_row_bytes = crop_width * 4;
        for row in 0..crop_height {
            let start = (first_row + row) * screen_row_bytes + clip.left as usize * 4;
            crop.pixels_mut()[row * crop_row_bytes..(row + 1) * crop_row_bytes]
                .copy_from_slice(&screen.pixels()[start..start + crop_row_bytes]);
        }
        crop
    }

    /// C# `ImageFoundNow`: one look for the image, with no waiting. A region
    /// is searched in its own image (captured now if it has none yet); no
    /// region means a fresh capture of the whole window.
    fn image_found_now(
        &self,
        image_needle: &Framebuffer,
        search_region: Option<&SearchRegion>,
    ) -> bool {
        let match_limit = self.config.match_limit as f64;
        match search_region {
            Some(region) => {
                let haystack = region.image(|| self.capture_region(region.screen_rect));
                find_least_squares_match(haystack, image_needle, match_limit).found
            }
            None => {
                let screen = self.get_current_screen();
                find_least_squares_match(&screen, image_needle, match_limit).found
            }
        }
    }

    /// C# `WaitForImage`: look for `image_needle` in `search_region` (the
    /// whole window when `None`); while it is missing and less than
    /// `secs_to_wait` of UI time has passed, let the UI run for 50 ms and
    /// look again. The answer is the last look.
    pub fn wait_for_image(
        &mut self,
        image_needle: &Framebuffer,
        secs_to_wait: f64,
        search_region: Option<&SearchRegion>,
    ) -> bool {
        self.check_not_timed_out();
        let time_waited = clock::now();
        loop {
            let found = self.image_found_now(image_needle, search_region);
            if found || clock::since(time_waited).as_secs_f64() >= secs_to_wait {
                return found;
            }
            self.delay(IMAGE_POLL_SECONDS);
        }
    }

    /// C# `ImageExists`: [`wait_for_image`](Self::wait_for_image) by another
    /// name. It always takes at least one look, so zero seconds is a single
    /// look.
    pub fn image_exists(
        &mut self,
        image_needle: &Framebuffer,
        secs_to_wait: f64,
        search_region: Option<&SearchRegion>,
    ) -> bool {
        self.wait_for_image(image_needle, secs_to_wait, search_region)
    }
}
