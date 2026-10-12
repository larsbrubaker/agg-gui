//! Where the runner's image needles come from — the port of agg-sharp
//! `AutomationRunner.LoadImageFromSourceFolder` and the `imagesDirectory`
//! argument of `ShowWindowAndExecuteTests` (`GuiAutomation/AutomationRunner.cs`).
//!
//! An image gesture (`runner/image_gestures.rs`) takes its needle either as
//! pixels ([`ImageNeedle::Buffer`]) or by file name ([`ImageNeedle::Name`]).
//! A name is resolved by the runner's [`ImageSource`]: by default a
//! [`DirectoryImageSource`] over `RunOptions::images_directory` (C# reads the
//! file under `imagesDirectory`, which is the current directory when the
//! argument is left empty); an application installs its own loader (its
//! static data) with `AutomationRunner::set_image_source`. A name that does
//! not load is a gesture that finds nothing, as in C#.

use std::path::{Path, PathBuf};

use agg_gui::Framebuffer;

/// The needle an image gesture looks for.
#[derive(Clone, Copy)]
pub enum ImageNeedle<'a> {
    /// A file name, resolved by the runner's [`ImageSource`].
    Name(&'a str),
    /// The needle's pixels (RGBA8, row 0 at the bottom, as a screen capture).
    Buffer(&'a Framebuffer),
}

/// Loads needles by name for the runner's image gestures.
pub trait ImageSource {
    /// The image called `image_name` (RGBA8, row 0 at the bottom), or `None`
    /// when there is no such image or it does not decode.
    fn load(&self, image_name: &str) -> Option<Framebuffer>;
}

impl<F: Fn(&str) -> Option<Framebuffer>> ImageSource for F {
    fn load(&self, image_name: &str) -> Option<Framebuffer> {
        self(image_name)
    }
}

/// C#'s image folder: needles are files under one directory (an empty path
/// is the current directory, as C#'s `Path.Combine("", name)` is).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryImageSource {
    directory: PathBuf,
}

impl DirectoryImageSource {
    /// Needles are read from files under `directory`.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// The folder needles are read from.
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl ImageSource for DirectoryImageSource {
    fn load(&self, image_name: &str) -> Option<Framebuffer> {
        let bytes = std::fs::read(self.directory.join(image_name)).ok()?;
        decode_needle(&bytes)
    }
}

/// Decode an image file (any format the `image` crate reads, as C#'s
/// `ImageIO.LoadImageData` reads its formats) into a needle: RGBA8 with row
/// 0 at the bottom, the orientation of the runner's screen captures.
pub fn decode_needle(bytes: &[u8]) -> Option<Framebuffer> {
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    let top_down = image.into_raw();
    let mut needle = Framebuffer::new(width, height);
    let row_bytes = width as usize * 4;
    if row_bytes == 0 {
        return Some(needle);
    }
    for (row, source) in top_down.chunks_exact(row_bytes).enumerate() {
        let target_row = height as usize - 1 - row;
        let start = target_row * row_bytes;
        needle.pixels_mut()[start..start + row_bytes].copy_from_slice(source);
    }
    Some(needle)
}
