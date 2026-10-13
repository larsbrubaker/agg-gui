//! When the Arc-keyed texture caches sweep their dead entries.
//!
//! `arc_texture_cache` (a `Label`'s bitmap, one entry per label) and `lcd_arc_texture_cache`
//! (cached text masks and LCD backbuffer planes) drop an entry once the `Arc` it was made from is
//! gone. Each draw used to look for such entries across the whole cache, so a page of N labels
//! cost N sweeps of N entries a frame: 165 ms of a 180 ms paint for ~3,200 labels. A dead entry
//! only holds a texture, and lookups already check the `Weak` they find (a recycled address never
//! returns a stale texture), so sweeping once a frame frees the same memory one frame sooner than
//! never and keeps the cost O(entries) per frame.

/// Per cache, the frame its last sweep ran in.
#[derive(Debug)]
pub(crate) struct DeadEntrySweeps {
    frame: u64,
    arc: u64,
    lcd: u64,
}

impl Default for DeadEntrySweeps {
    /// Nothing swept yet: the first draw of a context sweeps (a context used without frames,
    /// as the cache tests do, still sweeps once).
    fn default() -> Self {
        Self {
            frame: 0,
            arc: u64::MAX,
            lcd: u64::MAX,
        }
    }
}

impl DeadEntrySweeps {
    /// A new frame begins: each cache sweeps again at its next draw.
    pub(crate) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Whether `arc_texture_cache` has not been swept this frame (and is now marked swept).
    pub(crate) fn arc_due(&mut self) -> bool {
        std::mem::replace(&mut self.arc, self.frame) != self.frame
    }

    /// Whether `lcd_arc_texture_cache` has not been swept this frame (and is now marked swept).
    pub(crate) fn lcd_due(&mut self) -> bool {
        std::mem::replace(&mut self.lcd, self.frame) != self.frame
    }
}

#[cfg(test)]
mod tests {
    use super::DeadEntrySweeps;

    #[test]
    fn each_cache_sweeps_once_per_frame_however_many_draws() {
        let mut sweeps = DeadEntrySweeps::default();
        sweeps.begin_frame();
        let arc_runs = (0..1000).filter(|_| sweeps.arc_due()).count();
        let lcd_runs = (0..1000).filter(|_| sweeps.lcd_due()).count();
        assert_eq!((arc_runs, lcd_runs), (1, 1));
        sweeps.begin_frame();
        assert!(
            sweeps.arc_due() && sweeps.lcd_due(),
            "a new frame sweeps again"
        );
        assert!(!sweeps.arc_due() && !sweeps.lcd_due());
    }

    #[test]
    fn a_context_with_no_frames_sweeps_on_its_first_draw() {
        let mut sweeps = DeadEntrySweeps::default();
        assert!(sweeps.arc_due() && sweeps.lcd_due());
        assert!(!sweeps.arc_due() && !sweeps.lcd_due());
    }
}
