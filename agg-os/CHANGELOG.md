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
- `open-files` feature (macOS): `macos::open_files::install()`, which adds
  `application:openFiles:` to the application delegate's class when it lacks it, sets the
  delegate again so AppKit sees the method, and hands each batch of paths to a `fn(Vec<PathBuf>)`
  callback, replying success to AppKit; it fails with an `InstallError` (no delegate, or the
  method could not be added). Built on objc2 0.6 and objc2-app-kit 0.3.
- `app-menu` feature (macOS): `macos::menu`, with `MenuTarget`, an Objective-C object that is the
  delegate of the `NSMenu`s it makes (`new_menu()`) and the target of the action items it makes
  (`new_action_item()`), and that calls a `MenuHandler` when a menu is about to open, an item is
  picked (by its tag) or a key equivalent is looked for (`KeyClaim`) and fired. `new_app_item()`
  makes the standard Hide, Hide Others and Show All items (`AppAction`) that `NSApplication`
  answers. An application makes one `MenuTarget`: its Objective-C object and its handler live for
  the rest of the process, so the menus and items, which hold it weakly, always have it, and the
  handler is never dropped. Built on objc2 0.6 and objc2-app-kit 0.3.
- `microphone` feature (macOS): `macos::audio_input`, agg-mic's macOS capture.
  `audio_authorization()` reads the microphone permission as an `AudioAuthorization` without
  prompting; `request_audio_access()` asks for it, handing the answer to a closure on
  AVFoundation's queue; `default_audio_input()` names the default input device (`AudioInputInfo`:
  its name, whether it is built in). `InputEngine::start()` builds and starts an AVAudioEngine
  whose input node feeds an AVAudioSinkNode: each I/O cycle's 32-bit float buffers reach a closure
  on the real-time audio thread as slices with their channel counts, frame count and sample rate
  (`InputCycle`), and each `AVAudioEngineConfigurationChangeNotification` of that engine reaches
  another closure; a panic in either is caught and drops that cycle or event. It fails with an
  `InputEngineError` (no input format, a format that is not 32-bit float, or the engine's start
  error). `unobserve()` removes the observer; `shut_down()`, or a drop, removes it, stops the
  engine and detaches the node, returning how long the stop and the detach took. An engine shut
  down or dropped from inside one of its own callbacks is left running and never released (hand it
  to another thread to stop it). Built on objc2 0.6, block2 0.6, objc2-foundation,
  objc2-av-foundation, objc2-avf-audio and objc2-core-audio-types 0.3, with no AppKit.
- `fork-minidump` feature (Linux): `linux::fork_minidump::write_minidump_from_fork()`, which writes
  a minidump of this process to a file from a forked child that ptraces it, the calling thread as
  the blamed thread. The parent names the child its tracer with `PR_SET_PTRACER` (for Yama's
  `ptrace_scope` 1) and lets it go through a pipe; the child is killed at the given timeout. What
  the child does is fixed, since only some work is sound in the child of a multi-threaded process.
  It fails with a `ForkDumpError` (this process's id, creating the file, a pipe or the child, the
  child's exit code and report, a signal, the wait, or the timeout). Built on libc and
  minidump-writer 0.13.
