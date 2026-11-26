# agg-os

The operating system calls and the other unsafe code of the
[agg-gui](https://github.com/larsbrubaker/agg-gui) crates, each behind a safe function or type.

This crate holds the agg-gui workspace's unsafe code. A member whose manifest sets
`unsafe_code = "forbid"` cannot contain any, and a test in this crate checks that every member
does so except the ones it lists by name, which still call the operating system directly.
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
| `fork-minidump` | `linux::fork_minidump` | Linux only: `write_minidump_from_fork()` writes a minidump of this process from a forked child that ptraces it (named its tracer with `PR_SET_PTRACER` for Yama), killed at a timeout, failing with a `ForkDumpError`; libc and minidump-writer 0.13 |
