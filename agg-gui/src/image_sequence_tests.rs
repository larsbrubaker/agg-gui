//! Tests for `image_sequence.rs`: `ImageSequence`'s timing (C#'s
//! `GetImageIndexByTime`, `Time`, `AddImage`'s 1 ms floor) and the GIF
//! loader's frame delays (C#'s `ImageIO.LoadImageData(Stream, ImageSequence)`).

use super::*;

/// An RGBA GIF with one solid-coloured `w`×`h` frame per `(rgba, delay_cs)`,
/// encoded through the `image` crate's GIF encoder.
pub(crate) fn gif_bytes(w: u32, h: u32, frames: &[([u8; 4], u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut out);
        for (rgba, delay_cs) in frames {
            let buffer = image::RgbaImage::from_pixel(w, h, image::Rgba(*rgba));
            let delay = image::Delay::from_numer_denom_ms(u32::from(*delay_cs) * 10, 1);
            encoder
                .encode_frame(image::Frame::from_parts(buffer, 0, 0, delay))
                .expect("encode gif frame");
        }
    }
    out
}

#[test]
fn rust_only_load_gif_reads_each_frame_delay() {
    let bytes = gif_bytes(4, 3, &[([255, 0, 0, 255], 10), ([0, 0, 255, 255], 25)]);
    assert!(ImageSequence::is_gif(&bytes));
    let sequence = ImageSequence::load_gif(&bytes).expect("decodes");
    assert_eq!(sequence.num_frames(), 2);
    assert_eq!(sequence.frame_times_ms(), &[100, 250]);
    assert_eq!((sequence.width(), sequence.height()), (4, 3));
    assert_eq!(&sequence.frames()[0].data[..4], &[255, 0, 0, 255]);
    assert_eq!(&sequence.frames()[1].data[..4], &[0, 0, 255, 255]);
    // Ticks at the shortest delay.
    assert!((sequence.seconds_per_frame() - 0.1).abs() < 1e-12);
    assert!((sequence.time() - 0.35).abs() < 1e-12);
}

#[test]
fn rust_only_zero_gif_delay_is_one_ms_and_ticks_at_ten_ms() {
    let bytes = gif_bytes(2, 2, &[([255, 0, 0, 255], 0), ([0, 255, 0, 255], 0)]);
    let sequence = ImageSequence::load_gif(&bytes).expect("decodes");
    // AddImage's Math.Max(frameTimeMs, 1); the tick never under 10 ms.
    assert_eq!(sequence.frame_times_ms(), &[1, 1]);
    assert!((sequence.seconds_per_frame() - 0.010).abs() < 1e-12);
}

#[test]
fn rust_only_single_frame_gif_uses_default_frame_time() {
    let bytes = gif_bytes(2, 2, &[([9, 9, 9, 255], 50)]);
    let sequence = ImageSequence::load_gif(&bytes).expect("decodes");
    assert_eq!(sequence.num_frames(), 1);
    assert_eq!(sequence.frame_times_ms(), &[33]);
    assert!((sequence.seconds_per_frame() - 1.0 / 30.0).abs() < 1e-12);
}

#[test]
fn rust_only_not_a_gif_is_none() {
    assert!(!ImageSequence::is_gif(b"\x89PNG\r\n"));
    assert!(ImageSequence::load_gif(b"GIF89a broken").is_none());
}

#[test]
fn rust_only_image_index_by_time_walks_frame_times() {
    let mut sequence = ImageSequence::new();
    sequence.add_image(vec![0; 4], 1, 1, 100);
    sequence.add_image(vec![0; 4], 1, 1, 200);
    sequence.add_image(vec![0; 4], 1, 1, 0);
    assert_eq!(sequence.frame_times_ms(), &[100, 200, 1]);
    assert_eq!(sequence.image_index_by_time(0.0), Some(0));
    assert_eq!(sequence.image_index_by_time(0.0999), Some(0));
    assert_eq!(sequence.image_index_by_time(0.1), Some(1));
    assert_eq!(sequence.image_index_by_time(0.2999), Some(1));
    assert_eq!(sequence.image_index_by_time(0.3), Some(2));
    // Past the end: frames-per-second fallback, clamped to the last frame.
    assert_eq!(sequence.image_index_by_time(5.0), Some(2));
    assert_eq!(ImageSequence::new().image_index_by_time(0.0), None);
}
