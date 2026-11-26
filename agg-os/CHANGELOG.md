# Changelog

All notable changes to `agg-os` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

## [0.1.0]

### Added

- `counting-alloc` feature: `alloc_count::CountingAlloc`, a global allocator that forwards every
  call to the system allocator and counts each allocation and reallocation, and
  `alloc_count::allocations()`, the count so far.
- `thread-id` feature: `thread_id::current_thread_os_id()`, the calling thread's id as a dump
  records it (its Mach port name on macOS, its kernel tid on Linux, 0 elsewhere).
- `desktop-settings` feature (Windows): `windows::desktop_settings`, one read of a desktop setting
  per function: `mouse_hover_time_ms()`, `double_click_time_ms()`, `double_click_rect_px()`,
  `cursor_pos()`, `font_smoothing_enabled()`, `font_smoothing_type()`,
  `font_smoothing_orientation()`, `is_remote_session()` and
  `primary_display_rotated_quarter_turn()`. Built on windows-sys 0.52.
- `recycle-bin` feature (Windows): `windows::recycle_bin`, with `recycle()` (moves an item to the
  Recycle Bin, failing with a `RecycleError`), `long_path_name()` and `recycle_bin_item_count()`.
  A path holding a NUL character is refused before any call. Built on windows-sys 0.61.
