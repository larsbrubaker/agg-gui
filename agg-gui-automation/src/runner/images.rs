//! The runner's image searches — the port of `GetCurrentScreen`,
//! `ImageExists` and `WaitForImage` (agg-sharp
//! `GuiAutomation/AutomationRunner.cs`).
//!
//! A search looks for a needle image in a [`SearchRegion`]'s image with
//! [`find_least_squares_match`] under the runner's
//! [`match_limit`](super::AutomationConfig::match_limit). A region carries
//! its own image (captured once, on first use); with no region every look
//! takes a fresh capture of the whole window, so a wait sees the UI change.
//! Like the name waits in `named.rs`, the answer is the last look, not the
//! clock: a zero-second wait is a single look.
//!
//! The image gestures (`ClickImage`, `DragImage`, `DropImage`,
//! `DragDropImage`) and needles loaded by file name from an images folder
//! are not ported yet (`docs/design/gui-automation.md`, section 9).

use agg_gui::clock;
use agg_gui::Framebuffer;

use super::AutomationRunner;
use crate::driver::UiDriver;
use crate::image_match::find_least_squares_match;
use crate::search_region::{ScreenRectangle, SearchRegion};

/// How long an image wait lets the UI run between looks (C# `Delay(.05)`).
const IMAGE_POLL_SECONDS: f64 = 0.05;

/// A copy of `image`'s pixels (a [`Framebuffer`] is not `Clone`).
fn copy_framebuffer(image: &Framebuffer) -> Framebuffer {
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
                let haystack = region.image(|| self.get_current_screen());
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
