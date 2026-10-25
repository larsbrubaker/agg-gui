# Third-party notices: agg-mic

agg-mic (MIT, `LICENSE`) reaches each platform's microphone through the crates below, each on its
own platform only. All are pure Rust with no build script that compiles C. Each is used under the
MIT license (where a crate offers a choice, MIT is the one taken), whose terms ask that its
copyright notice and permission notice go with every copy: an application that ships agg-mic
(agg-gui-whistle's `native-mic` feature, for one), as source or as a binary, includes this file.
Add an entry here in the same change that adds a dependency from a new upstream.

The capture code itself is the author's own: it was first written for mattercad-rust's
`mattercad-ai` (`src/audio/`, Lars Brubaker) and moved here.

Procedural-macro crates (thiserror-impl, enum-primitive-derive, futures-macro, windows-implement,
windows-interface, and syn, quote, proc-macro2 and unicode-ident under them) run only while
compiling and leave no code in a binary, so they are not listed.

## macOS

### objc2, objc2-encode, block2 and objc2-foundation (MIT); objc2-av-foundation, objc2-avf-audio and objc2-core-audio-types (Zlib OR Apache-2.0 OR MIT)

Source: https://github.com/madsmtm/objc2. These are crates.io dependencies of `agg-mic`, used for
the microphone capture (`mac_capture`, its sink node's audio types, and `mac_engine_change`,
which hears a lost device). They are pure-Rust bindings to the system's Objective-C runtime and
its Foundation, AVFoundation, AVFAudio and CoreAudio types, with no build script. Only the
classes the capture uses are enabled. objc2, block2 and objc2-foundation were already in
agg-gui's build through winit.

    Copyright 2026 Mads Marquart

(the copyright line of the repository's `LICENSE-MIT.txt`; its `LICENSE.md` says how the crates
are licensed and that they are derived from Apple's SDK headers.)

### bitflags (MIT OR Apache-2.0)

Source: https://github.com/bitflags/bitflags. It is a dependency of objc2-core-audio-types, for
its flag types. It is also in the Linux build, under pulseaudio.

    Copyright (c) 2014 The Rust Project Developers

## Windows

### windows and its crates (MIT OR Apache-2.0)

Source: https://github.com/microsoft/windows-rs. It is a crates.io dependency of `agg-mic` on
Windows only, used for the microphone capture (`windows_capture`). These are pure-Rust bindings
to COM and WASAPI, linked through windows-link with no build script. Only the APIs the capture
calls are enabled. Its own crates come from the same repository under the same license and
notice: windows-core, windows-link, windows-result, windows-strings, windows-collections,
windows-future, windows-numerics and windows-threading. It was already in agg-gui's build
through wgpu.

    Copyright (c) Microsoft Corporation.

## Linux

### pulseaudio (MIT)

Source: https://github.com/colinmarc/pulseaudio-rs. It is a crates.io dependency of `agg-mic` on
Linux only, used for the microphone capture (`linux_capture`, with `pulse_frames`). It speaks the
PulseAudio native protocol in pure Rust, with no libpulse, no ALSA and no build script. That
protocol is spoken by PulseAudio and by PipeWire's pulse server.

    Copyright 2023 Colin Marc <hi@colinmarc.com>

### pulseaudio's dependencies

All are used under MIT; where a crate offers a choice, the alternatives are Apache-2.0 or the
Unlicense.

| Crate | Copyright |
|---|---|
| mio (MIT) | Copyright (c) 2014 Carl Lerche and other MIO contributors |
| slab (MIT) | Copyright (c) 2019 Carl Lerche |
| futures, futures-channel, futures-core, futures-executor, futures-io, futures-sink, futures-task, futures-util (MIT OR Apache-2.0) | Copyright (c) 2016 Alex Crichton |
| bitflags (MIT OR Apache-2.0) | Copyright (c) 2014 The Rust Project Developers |
| num-traits, log (MIT OR Apache-2.0) | Copyright (c) 2014 The Rust Project Developers |
| libc (MIT OR Apache-2.0) | Copyright (c) The Rust Project Developers |
| byteorder, memchr (Unlicense OR MIT) | Copyright (c) 2015 Andrew Gallant |
| thiserror (MIT OR Apache-2.0) | Copyright (c) David Tolnay (its license file carries no copyright line; David Tolnay is its author) |
| pin-project-lite (Apache-2.0 OR MIT) | The pin-project-lite authors (its license file carries no copyright line) |

## The MIT license

Each copyright notice above is given with this permission notice:

    Permission is hereby granted, free of charge, to any person obtaining a copy of this
    software and associated documentation files (the "Software"), to deal in the Software
    without restriction, including without limitation the rights to use, copy, modify, merge,
    publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
    to whom the Software is furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in all copies or
    substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
    INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
    PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
    FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
    OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
    DEALINGS IN THE SOFTWARE.
