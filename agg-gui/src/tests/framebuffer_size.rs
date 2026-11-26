//! `Framebuffer` sizes that cannot be drawn into must be refused, not wrapped.
//!
//! `Framebuffer::new` and `Framebuffer::resize` compute the RGBA8 byte length
//! through one helper in `framebuffer.rs` (`rgba8_byte_len`). A size whose bytes
//! do not fit an allocation (more than `isize::MAX`), or whose row does not fit
//! the `i32` stride the renderer attaches every framebuffer with (whatever the
//! height), panics at once with a message naming the size. These tests never
//! allocate a large buffer: they reach the helper directly for the sizes that
//! would succeed with gigabytes, and `rgba8_byte_len_within` for the limit a
//! 32-bit target such as wasm32 meets.

use super::*;
use crate::framebuffer::{rgba8_byte_len, rgba8_byte_len_within};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[test]
#[should_panic(expected = "framebuffer of 4294967295 x 4294967295 pixels is too large")]
fn new_refuses_a_size_whose_bytes_overflow() {
    let _ = Framebuffer::new(u32::MAX, u32::MAX);
}

#[test]
#[should_panic(expected = "framebuffer of 4294967295 x 4294967295 pixels is too large")]
fn resize_refuses_a_size_whose_bytes_overflow() {
    let mut fb = Framebuffer::new(3, 2);
    fb.resize(u32::MAX, u32::MAX);
}

#[test]
fn resize_that_panics_leaves_the_framebuffer_unchanged() {
    let mut fb = Framebuffer::new(3, 2);
    let result = catch_unwind(AssertUnwindSafe(|| fb.resize(u32::MAX, u32::MAX)));
    assert!(result.is_err(), "resize to an impossible size must panic");
    assert_eq!(fb.width(), 3);
    assert_eq!(fb.height(), 2);
    assert_eq!(fb.pixels().len(), 24);
}

#[test]
fn byte_len_of_an_empty_size_is_zero() {
    assert_eq!(rgba8_byte_len(0, 5), Some(0));
    assert_eq!(rgba8_byte_len(5, 0), Some(0));
}

#[test]
fn byte_len_of_a_small_size_is_exact() {
    assert_eq!(rgba8_byte_len(3, 2), Some(24));
}

#[test]
fn byte_len_reports_overflow() {
    assert_eq!(rgba8_byte_len(u32::MAX, u32::MAX), None);
}

#[test]
fn byte_len_refuses_a_row_that_does_not_fit_the_i32_stride() {
    // One row of 2^29 pixels is 2^31 bytes, one more than `i32::MAX`.
    assert_eq!(rgba8_byte_len(1 << 29, 1), None);
    assert_eq!(
        rgba8_byte_len((1 << 29) - 1, 1),
        Some(((1usize << 29) - 1) * 4)
    );
}

#[cfg(target_pointer_width = "64")]
#[test]
fn byte_len_of_32768_squared_does_not_wrap() {
    // `32768 * 32768 * 4` in `u32` wrapped to zero.
    assert_eq!(rgba8_byte_len(1 << 15, 1 << 15), Some(1usize << 32));
}

#[test]
#[should_panic(expected = "framebuffer of 536870912 x 0 pixels is too large")]
fn new_refuses_a_row_too_wide_for_the_i32_stride_even_with_no_rows() {
    // Allocates nothing: the height is zero, but every `Framebuffer` must keep
    // `width * 4 <= i32::MAX`, which `pixels_flipped` and every stride rely on.
    let _ = Framebuffer::new(1 << 29, 0);
}

#[test]
fn new_accepts_empty_sizes() {
    for (w, h) in [(0, 5), (5, 0), (0, 0)] {
        let fb = Framebuffer::new(w, h);
        assert_eq!(fb.pixels().len(), 0, "{w} x {h}");
        assert_eq!(fb.width(), w);
        assert_eq!(fb.height(), h);
    }
}

#[test]
fn byte_len_refuses_more_than_a_32_bit_target_can_allocate() {
    // wasm32 allows at most `i32::MAX` bytes in one allocation (`isize::MAX`).
    let max_len = i32::MAX as usize;
    assert_eq!(rgba8_byte_len_within(1 << 15, 1 << 15, max_len), None);
    assert_eq!(
        rgba8_byte_len_within(1 << 14, 1 << 14, max_len),
        Some(1 << 30)
    );
}

#[test]
fn byte_len_accepts_exactly_the_allocation_limit() {
    assert_eq!(rgba8_byte_len_within(3, 2, 24), Some(24));
    assert_eq!(rgba8_byte_len_within(3, 2, 23), None);
}
