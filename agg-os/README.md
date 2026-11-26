# agg-os

The operating system calls and the other unsafe code of the
[agg-gui](https://github.com/larsbrubaker/agg-gui) crates, each behind a safe function or type.

This crate holds the agg-gui workspace's unsafe code. Every other member of the workspace sets
`unsafe_code = "forbid"` in its manifest, so it cannot contain any, and a test in this crate
checks that every member but this one does so.
Everything public here is safe to call: a caller that uses only safe Rust cannot
cause undefined behavior through it, so no function takes a raw pointer, a raw handle or a size
that the unsafe code then trusts.

Each area is a Cargo feature, none on by default, so a consumer compiles only the bindings it
uses. No build script, and no dependency on the other agg-gui crates.

| Feature | Module | What it is |
|---|---|---|
| `counting-alloc` | `alloc_count` | `CountingAlloc`, a global allocator that forwards to the system allocator and counts allocations; `allocations()` reads the count |
| `thread-id` | `thread_id` | `current_thread_os_id()`, the calling thread's Mach port name on macOS, kernel tid on Linux, 0 elsewhere |
| `desktop-settings` | `windows::desktop_settings` | Windows only: one read of a desktop setting per function (mouse hover time, double-click time and rectangle, cursor position, font smoothing on/type/orientation, remote session, primary display rotation); windows-sys 0.52 |
| `recycle-bin` | `windows::recycle_bin` | Windows only: `recycle()` moves an item to the Recycle Bin, `long_path_name()` spells out a path's 8.3 components, `recycle_bin_item_count()` reads a drive's bin; windows-sys 0.61 |
| `open-files` | `macos::open_files` | macOS only: `install()` makes the application delegate answer `application:openFiles:` (Finder double-click, Open With, a drop on the Dock icon), handing each batch of paths to a callback and replying success to AppKit; objc2 0.6 |
| `app-menu` | `macos::menu` | macOS only: `MenuTarget`, the delegate of the `NSMenu`s and the target of the `NSMenuItem`s it makes, calling a `MenuHandler` (menu about to open, item picked, key equivalent claimed and fired); `AppAction` items for Hide, Hide Others and Show All; objc2 0.6 |
| `microphone` | `macos::audio_input` | macOS only: `audio_authorization()` and `request_audio_access()` (the microphone permission), `default_audio_input()` (its name, whether built in), and `InputEngine`, an AVAudioEngine whose input feeds an AVAudioSinkNode, handing each I/O cycle's float samples to a closure as slices (`InputCycle`) and each configuration change of the engine (a device unplugged or switched) to another, with `unobserve()` and a timed `shut_down()`; a panic in either closure is caught; for agg-mic; objc2 0.6, block2 0.6, objc2-foundation, objc2-av-foundation, objc2-avf-audio and objc2-core-audio-types 0.3 |
| `microphone` | `windows::audio_capture` | Windows only: `ComApartment` (COM on the calling thread, `CoInitializeEx` multithreaded and `CoUninitialize` on drop) and `CaptureStream`, a started WASAPI shared-mode capture on the default endpoint that borrows it: opened as 16-bit mono PCM the engine converts to, or, when the caller falls back, in the device's mix format if the caller accepts it (`MixFormat`); `next_packet()` appends a packet's bytes (`Packet`: frames, silent) and the drop stops it; errors are `CaptureError` (HRESULT and message) and `OpenError`; for agg-mic; windows 0.62 |
| `fork-minidump` | `linux::fork_minidump` | Linux only: `write_minidump_from_fork()` writes a minidump of this process from a forked child that ptraces it (named its tracer with `PR_SET_PTRACER` for Yama), killed at a timeout, failing with a `ForkDumpError`; libc and minidump-writer 0.13 |
