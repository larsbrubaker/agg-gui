# Third-party notices: agg-gui-whistle

agg-gui-whistle (MIT, `LICENSE`) depends on whistle-rust and downloads the Whistle model at run
time. Both are under the Apache License 2.0, whose text is `LICENSE-APACHE` beside this file. An
application that ships agg-gui-whistle, or lets it download `whistle.cact`, includes
`LICENSE-APACHE` and this notice.

- **whistle-rust** (github.com/larsbrubaker/whistle-rust), Copyright Lars Brubaker, Apache-2.0:
  the speech-to-text engine, linked into the transcription worker (and the native provider).
- **Whistle weights**, `whistle.cact` (huggingface.co/Cactus-Compute/whistle), Copyright Cactus
  Compute, Inc., Apache-2.0: downloaded and cached at run time, never stored in this repository.

With the `native-mic` feature it also links agg-mic, this workspace's microphone, whose platform
bindings (objc2 on macOS, windows on Windows, pulseaudio on Linux) are listed in
`agg-mic/THIRD_PARTY_NOTICES.md`; a native build with the feature includes that notice too.

whistle-rust's own notices follow, verbatim (`whistle-rust/THIRD_PARTY_NOTICES.md`).

---

# Third-party notices

## Whistle model code (needle)

whistle-rust is a Rust translation of the Whistle model definition, quantization and archive
format in github.com/cactus-compute/needle (`needle/model/whistle.py`, `architecture.py`,
`quantize.py`, `export.py`), pinned in `REFERENCE`.

Copyright Cactus Compute, Inc. Licensed under the Apache License, Version 2.0
(http://www.apache.org/licenses/LICENSE-2.0). Changes: re-expressed in Rust as an inference-only
library; training, fine-tuning and export code are not included.

The oracle scripts in `oracle/` import that code (fetched into `needle-reference/`, not
redistributed here) to generate test fixtures.

## Whistle weights

`whistle.cact` (huggingface.co/Cactus-Compute/whistle, revision pinned in `REFERENCE`) is
Copyright Cactus Compute, Inc., Apache License 2.0. It is not stored in this repository;
`scripts/fetch-weights.sh` downloads it. Applications that ship it must include the Apache-2.0
license text and this notice.

Citation: "Whistle: Speech Recognition for Tiny Devices" (2026), Cactus Compute, Inc.

## Test fixtures

`test-data/fixtures/` holds tensors computed by the needle model code from the Whistle weights
(Apache-2.0, as above). `test-data/clips/` and `test-data/clips-extra/` are synthesized speech
(macOS `say`) generated for this project. `test-data/engine/`, `test-data/engine-extra/` and
`test-data/engine-stream/` hold transcripts produced by the Needle 3 engine binary
(huggingface.co/Cactus-Compute/needle3, Apache-2.0 model card), used only as a black-box
reference (`engine-extra/` with and without its `--audio-keywords` option, `engine-stream/`
the updates of its `--audio-stream` mode).

## Deterministic math (libm / musl / fdlibm)

`crates/whistle-rust/src/math/` (`expf`, `logf`, `sinf`, `cosf`, `powf`, `exp`, `log`,
`log10`, `sin`, `cos`, `pow` and their argument reductions) is ported from the Rust `libm` crate
0.2.16 (now maintained in github.com/rust-lang/compiler-builtins, `libm/`), a Rust port of musl
libm (musl.libc.org), whose routines derive from FreeBSD msun and Sun fdlibm; `scalbnf` and
`scalbn` in `math.rs` follow musl's. Changes: x87-only paths, exception-flag forcing and
unchecked indexing removed; the 2/pi table trimmed to the length f32 and f64 inputs need.

The libm crate:

    Copyright (c) 2018 Jorge Aparicio

musl libm:

    Copyright © 2005-2020 Rich Felker, et al.

Both are available under the MIT license:

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

The underlying fdlibm code carries, in each ported file:

    Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
    Developed at SunPro, a Sun Microsystems, Inc. business. Permission to use, copy, modify,
    and distribute this software is freely granted, provided that this notice is preserved.
