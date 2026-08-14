//! Byte-exactness fixture generator for the agg-sharp LCD subpixel port.
//!
//! Dumps the reference implementation's own output for a fixed set of vector
//! paths so the C# port (`MatterHackers.Agg.LcdCoverage`) can assert
//! byte-for-byte equality against it.  Everything here calls the real
//! production pipeline — `LcdMaskBuilder` / `composite_lcd_mask` from
//! [`agg_gui::lcd_coverage`] — nothing is reimplemented locally.
//!
//! Run:
//!
//! ```text
//! cargo run -p agg-gui --example lcd_reference_fixture -- <out_dir> [<agg-gui sha>]
//! ```
//!
//! `<out_dir>` defaults to `target/lcd_reference_fixture`.  Output is one
//! raw binary blob per case per stage plus a line-oriented `manifest.txt`
//! that carries every parameter the C# side needs to rebuild the case.
//!
//! **Provenance.**  The manifest header records which reference produced it:
//! the agg-gui commit and the resolved `agg-rust` version from the workspace
//! `Cargo.lock`.  These are comment lines - informational only, never parsed
//! for behaviour - because a checkout with local edits, or a shallow/export
//! copy with no git metadata, must still be able to regenerate the fixture.
//! Pass the sha explicitly as the second argument when `git rev-parse` is not
//! available or not the right answer.
//!
//! **Why the manifest carries the geometry.**  The paths, the transform
//! matrices and the clip rects are written out as exact IEEE-754 bit
//! patterns (`<decimal>#<16 hex digits>`) rather than being re-typed as
//! literals on the C# side.  Duplicated literals could drift; bits cannot.
//! The transform is dumped as its six affine components, so no rotation or
//! scale composition (and no `sin`/`cos` call) happens on the consuming
//! side at all.
//!
//! **Determinism**: no clock, no RNG, no font/glyph shaping (pure vector
//! paths, so the fixture does not depend on font-stack parity), and the
//! two typography globals the filter reads are asserted to be at their
//! defaults so the byte-exact integer filter path is the one exercised.
//!
//! **Stages dumped per case** (`.lcd`, `.gray`, optional `.dst`, optional
//! `.bufcolor`/`.bufalpha`): the LCD mask, the gray-collapse mask, the stage-3
//! composite over a known destination, and the two planes of an [`LcdBuffer`]
//! painted through `LcdBuffer::fill_path`.  The gray-collapse dump is the stage
//! separator: it is a pure function of the 3x gray raster buffer, so if `.gray`
//! matches and `.lcd` does not, the divergence is in the 5-tap filter; if
//! `.gray` itself differs, the divergence is in the raster stage.
//!
//! The buffer dump is the only stage that goes through `fill_path`, so it is
//! also the only one that pins the bbox-sized mask and the integer origin
//! `build_bounded_mask` computes internally - a mask dump cannot reach either,
//! because the harness hands the mask builder its size explicitly.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use agg_gui::color::Color;
use agg_gui::draw_ctx::FillRule;
use agg_gui::lcd_coverage::{composite_lcd_mask, LcdBuffer, LcdMask, LcdMaskBuilder};
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::path_storage::PathStorage;
use agg_rust::trans_affine::TransAffine;

/// Cosine of 30 degrees, written as an exact literal.  The rotation matrices
/// below must be bit-identical to the ones the C# fixture test uses, and the
/// only way to guarantee that across two language runtimes is to never call a
/// trig function on either side: this literal is the shortest round-trip
/// decimal for the f64 nearest cos(pi/6).
const COS30: f64 = 0.8660254037844387;

/// Sine of 30 degrees - exactly representable, no rounding concern.
const SIN30: f64 = 0.5;

/// Circle-from-four-cubics control point factor, 4/3 * tan(pi/8).
const KAPPA: f64 = 0.5522847498307933;

/// The workspace lock file, embedded at compile time so the dependency version
/// stamped into the manifest is the one this binary was actually built against -
/// reading the lock from disk at run time could report a resolution that has
/// since moved on.
const CARGO_LOCK: &str = include_str!("../../Cargo.lock");

/// One path command.  Deliberately the smallest set that covers the fixture:
/// straight edges plus cubic Beziers (the curve flattening canary).
#[derive(Clone, Copy)]
enum Cmd {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    /// Cubic Bezier: two control points then the end point.
    Curve4(f64, f64, f64, f64, f64, f64),
    Close,
}

/// How a composite case pre-fills its destination.  Both variants are trivial
/// to reproduce exactly on the consuming side.
#[derive(Clone, Copy)]
enum DstFill {
    /// One RGBA colour everywhere.
    Solid([u8; 4]),
    /// Two RGBA colours split at an x column (`x < split` takes the first) -
    /// a non-uniform destination, so the composite cannot accidentally pass
    /// by ignoring the destination read.
    Halves(i32, [u8; 4], [u8; 4]),
}

/// Stage 3 parameters: the mask is composited onto a known destination in a
/// known colour at a known **integer** origin (sub-pixel placement would smear
/// the per-channel phase pattern, so the reference rounds and so do we).
#[derive(Clone, Copy)]
struct CompositeCase {
    dst_w: u32,
    dst_h: u32,
    fill: DstFill,
    /// Source colour as bytes; converted with `byte as f32 / 255.0`, which is
    /// exactly what the C# `Color` -> float conversion does.
    src: [u8; 4],
    origin: (i32, i32),
}

/// Stage-4 parameters: paint the case's paths into an [`LcdBuffer`] through
/// `fill_path` and dump both planes.  Unlike the mask stages, `fill_path`
/// computes its own mask size and integer origin from the transformed path
/// bbox, so this is what pins `build_bounded_mask`'s placement end to end.
///
/// Each path is a separate `fill_path` call (its own bounded mask), which is
/// how a caller paints unrelated fills; the mask stages instead accumulate
/// every path into one gray buffer.  Single-path cases make the two identical.
#[derive(Clone, Copy)]
struct BufferCase {
    buf_w: u32,
    buf_h: u32,
    /// Background the buffer is cleared to first, as bytes.  A semi-transparent
    /// background is deliberate: it leaves both planes non-trivial, so the
    /// `(1 - eff_a)` accumulate terms of the per-channel composite are pinned
    /// rather than multiplied by a zero destination.
    clear: [u8; 4],
    /// Fill colour as bytes; its alpha scales every channel's coverage.
    src: [u8; 4],
}

struct Case {
    name: &'static str,
    mask_w: u32,
    mask_h: u32,
    fill_rule: FillRule,
    /// Clip rect as the reference takes it: `(x, y, w, h)` in mask pixels.
    clip: Option<(f64, f64, f64, f64)>,
    xform: TransAffine,
    /// One entry per `add(path)` call - several entries exercise the
    /// accumulate-into-one-gray-buffer behaviour.
    paths: Vec<Vec<Cmd>>,
    composite: Option<CompositeCase>,
    buffer: Option<BufferCase>,
}

fn identity() -> TransAffine {
    TransAffine {
        sx: 1.0,
        shy: 0.0,
        shx: 0.0,
        sy: 1.0,
        tx: 0.0,
        ty: 0.0,
    }
}

fn translation(tx: f64, ty: f64) -> TransAffine {
    TransAffine {
        sx: 1.0,
        shy: 0.0,
        shx: 0.0,
        sy: 1.0,
        tx,
        ty,
    }
}

fn scale_translate(scale: f64, tx: f64, ty: f64) -> TransAffine {
    TransAffine {
        sx: scale,
        shy: 0.0,
        shx: 0.0,
        sy: scale,
        tx,
        ty,
    }
}

/// Rotate by 30 degrees (from the literals above), uniformly scale, then
/// translate.  Written out component-wise rather than composed through
/// `TransAffine::multiply` so the matrix is obviously what it looks like.
fn rot30_scale_translate(scale: f64, tx: f64, ty: f64) -> TransAffine {
    TransAffine {
        sx: COS30 * scale,
        shy: SIN30 * scale,
        shx: -SIN30 * scale,
        sy: COS30 * scale,
        tx,
        ty,
    }
}

fn rect(x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<Cmd> {
    vec![
        Cmd::MoveTo(x1, y1),
        Cmd::LineTo(x2, y1),
        Cmd::LineTo(x2, y2),
        Cmd::LineTo(x1, y2),
        Cmd::Close,
    ]
}

/// Five-point pentagram wound as a single self-intersecting contour, so the
/// centre pentagon is winding 2 (filled under non-zero) but parity 0 (a hole
/// under even-odd).  Coordinates are plain literals - the exact values reach
/// the C# side through the manifest, not through re-typing.
fn star() -> Vec<Cmd> {
    vec![
        Cmd::MoveTo(20.0, 35.5),
        Cmd::LineTo(26.25, 12.5),
        Cmd::LineTo(4.75, 26.75),
        Cmd::LineTo(35.25, 26.75),
        Cmd::LineTo(13.75, 12.5),
        Cmd::Close,
    ]
}

/// Circle of `r` about `(cx, cy)` as four cubic Beziers - the flattening
/// canary (Rust `ConvCurve` vs C# `FlattenCurves`, both curve_div at
/// approximation scale 1.0).
fn circle(cx: f64, cy: f64, r: f64) -> Vec<Cmd> {
    let k = r * KAPPA;
    vec![
        Cmd::MoveTo(cx + r, cy),
        Cmd::Curve4(cx + r, cy + k, cx + k, cy + r, cx, cy + r),
        Cmd::Curve4(cx - k, cy + r, cx - r, cy + k, cx - r, cy),
        Cmd::Curve4(cx - r, cy - k, cx - k, cy - r, cx, cy - r),
        Cmd::Curve4(cx + k, cy - r, cx + r, cy - k, cx + r, cy),
        Cmd::Close,
    ]
}

fn cases() -> Vec<Case> {
    let composite_over_white = CompositeCase {
        dst_w: 48,
        dst_h: 30,
        fill: DstFill::Solid([255, 255, 255, 255]),
        src: [0, 0, 0, 255],
        origin: (4, 3),
    };
    // Light-on-dark, half-opacity source, non-uniform destination, and an
    // origin that hangs the mask off the bottom-left corner so the composite's
    // clipping is pinned too.
    let composite_light_on_dark = CompositeCase {
        dst_w: 44,
        dst_h: 28,
        fill: DstFill::Halves(22, [32, 32, 40, 255], [200, 60, 10, 255]),
        src: [255, 255, 255, 128],
        origin: (-3, -2),
    };

    // One buffer case is enough to pin the stage: the two planes are a
    // per-pixel function of the mask (already pinned per case) and the
    // composite, so what a second case would add is more geometry, not more
    // arithmetic.  It goes on the star because that case has AA on every edge,
    // a bbox strictly inside the buffer (so a wrong `fill_path` origin shows up
    // as displaced ink rather than as clipping) and a fill rule that matters.
    let buffer_over_translucent_slate = BufferCase {
        buf_w: 40,
        buf_h: 40,
        clear: [40, 60, 90, 128],
        src: [230, 220, 255, 200],
    };

    vec![
        // ── Transform variants over one axis-aligned fractional rect ──────
        Case {
            name: "rect_frac_identity",
            mask_w: 48,
            mask_h: 30,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: identity(),
            paths: vec![rect(5.25, 4.75, 28.5, 16.25)],
            composite: Some(composite_over_white),
            buffer: None,
        },
        Case {
            name: "rect_frac_translate",
            mask_w: 48,
            mask_h: 30,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: translation(0.25, 0.75),
            paths: vec![rect(5.25, 4.75, 28.5, 16.25)],
            composite: None,
            buffer: None,
        },
        Case {
            name: "rect_rot30",
            mask_w: 48,
            mask_h: 40,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: rot30_scale_translate(1.0, 24.0, 20.0),
            paths: vec![rect(-8.0, -5.0, 8.0, 5.0)],
            composite: None,
            buffer: None,
        },
        Case {
            // 3.75x rather than 3.7x: exactly representable in binary, so the
            // scale literal itself can never be a source of divergence.
            name: "rect_scale375",
            mask_w: 48,
            mask_h: 36,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: scale_translate(3.75, 2.0, 2.0),
            paths: vec![rect(1.25, 1.5, 9.75, 7.25)],
            composite: None,
            buffer: None,
        },
        // ── Sub-pixel features ────────────────────────────────────────────
        Case {
            // Sliver: under a pixel wide at the top, so most columns carry
            // partial coverage in a single subpixel only.
            name: "sliver_triangle",
            mask_w: 24,
            mask_h: 28,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: identity(),
            paths: vec![vec![
                Cmd::MoveTo(6.5, 3.25),
                Cmd::LineTo(7.125, 24.5),
                Cmd::LineTo(8.25, 3.25),
                Cmd::Close,
            ]],
            composite: None,
            buffer: None,
        },
        Case {
            name: "sliver_triangle_rot30",
            mask_w: 32,
            mask_h: 32,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: rot30_scale_translate(1.0, 10.0, 4.0),
            paths: vec![vec![
                Cmd::MoveTo(6.5, 3.25),
                Cmd::LineTo(7.125, 24.5),
                Cmd::LineTo(8.25, 3.25),
                Cmd::Close,
            ]],
            composite: None,
            buffer: None,
        },
        Case {
            // Shallow wedge fully inside the buffer: the two long edges are
            // nearly parallel, so the AA coverage ramp is spread over many
            // columns - the most sensitive raster case in the set.
            name: "shallow_wedge",
            mask_w: 48,
            mask_h: 20,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: identity(),
            paths: vec![vec![
                Cmd::MoveTo(3.5, 5.0),
                Cmd::LineTo(44.5, 12.25),
                Cmd::LineTo(44.5, 10.75),
                Cmd::Close,
            ]],
            composite: None,
            buffer: None,
        },
        // ── Multi-path accumulation into one gray buffer ──────────────────
        Case {
            name: "two_overlap_nonzero",
            mask_w: 40,
            mask_h: 30,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: identity(),
            paths: vec![
                rect(4.5, 4.25, 24.75, 18.5),
                vec![
                    Cmd::MoveTo(14.25, 9.75),
                    Cmd::LineTo(34.5, 6.5),
                    Cmd::LineTo(30.75, 25.25),
                    Cmd::LineTo(12.5, 20.5),
                    Cmd::Close,
                ],
            ],
            composite: None,
            buffer: None,
        },
        // ── Fill-rule sensitivity ─────────────────────────────────────────
        Case {
            name: "star_nonzero",
            mask_w: 40,
            mask_h: 40,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: identity(),
            paths: vec![star()],
            composite: Some(composite_light_on_dark),
            buffer: Some(buffer_over_translucent_slate),
        },
        Case {
            name: "star_evenodd",
            mask_w: 40,
            mask_h: 40,
            fill_rule: FillRule::EvenOdd,
            clip: None,
            xform: identity(),
            paths: vec![star()],
            composite: None,
            buffer: None,
        },
        // ── Clip rect at fractional coordinates ───────────────────────────
        Case {
            name: "clip_frac",
            mask_w: 48,
            mask_h: 30,
            fill_rule: FillRule::NonZero,
            clip: Some((7.25, 5.5, 15.5, 9.25)),
            xform: identity(),
            paths: vec![rect(5.25, 4.75, 40.5, 24.25)],
            composite: None,
            buffer: None,
        },
        // ── Curve flattening canary ───────────────────────────────────────
        Case {
            name: "circle_cubic",
            mask_w: 40,
            mask_h: 40,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: identity(),
            paths: vec![circle(20.0, 20.0, 15.5)],
            composite: None,
            buffer: None,
        },
        Case {
            // Same curve under rotation + non-integer scale: flattening runs
            // in path space (ConvCurve before ConvTransform on both sides),
            // so this also pins that ordering.
            name: "circle_cubic_rot30_scale",
            mask_w: 56,
            mask_h: 56,
            fill_rule: FillRule::NonZero,
            clip: None,
            xform: rot30_scale_translate(1.75, 28.0, 26.0),
            paths: vec![circle(0.0, 0.0, 12.25)],
            composite: None,
            buffer: None,
        },
    ]
}

fn to_path_storage(cmds: &[Cmd]) -> PathStorage {
    let mut path = PathStorage::new();
    for cmd in cmds {
        match *cmd {
            Cmd::MoveTo(x, y) => path.move_to(x, y),
            Cmd::LineTo(x, y) => path.line_to(x, y),
            Cmd::Curve4(x1, y1, x2, y2, x3, y3) => path.curve4(x1, y1, x2, y2, x3, y3),
            Cmd::Close => path.close_polygon(PATH_FLAGS_NONE),
        }
    }
    path
}

/// Rasterize one case through the production builder.  `gray` selects the
/// finalize stage; the raster stage is identical either way, which is what
/// makes the two dumps a stage separator.
fn build_mask(case: &Case, gray: bool) -> LcdMask {
    let mut builder = LcdMaskBuilder::new(case.mask_w, case.mask_h)
        .with_clip(case.clip)
        .with_fill_rule(case.fill_rule);
    let mut paths: Vec<PathStorage> = case.paths.iter().map(|p| to_path_storage(p)).collect();
    builder.with_paths(&case.xform, |add| {
        for path in paths.iter_mut() {
            add(path);
        }
    });
    if gray {
        builder.finalize_gray()
    } else {
        builder.finalize()
    }
}

fn make_dst(spec: &CompositeCase) -> Vec<u8> {
    let mut dst = vec![0u8; (spec.dst_w as usize) * (spec.dst_h as usize) * 4];
    for y in 0..spec.dst_h as i32 {
        for x in 0..spec.dst_w as i32 {
            let rgba = match spec.fill {
                DstFill::Solid(c) => c,
                DstFill::Halves(split, a, b) => {
                    if x < split {
                        a
                    } else {
                        b
                    }
                }
            };
            let i = ((y as usize) * (spec.dst_w as usize) + (x as usize)) * 4;
            dst[i..i + 4].copy_from_slice(&rgba);
        }
    }
    dst
}

fn byte_color(rgba: [u8; 4]) -> Color {
    Color::rgba(
        rgba[0] as f32 / 255.0,
        rgba[1] as f32 / 255.0,
        rgba[2] as f32 / 255.0,
        rgba[3] as f32 / 255.0,
    )
}

/// `<shortest round-trip decimal>#<raw f64 bits>`.  The decimal half is for
/// human eyes; the consuming side parses the hex bits, so the value crossing
/// the language boundary is exact by construction.
fn num(v: f64) -> String {
    format!("{}#{:016x}", v, v.to_bits())
}

fn fill_rule_name(rule: FillRule) -> &'static str {
    match rule {
        FillRule::NonZero => "nonzero",
        FillRule::EvenOdd => "evenodd",
    }
}

fn path_line(cmds: &[Cmd]) -> String {
    let mut out = String::from("path");
    for cmd in cmds {
        match *cmd {
            Cmd::MoveTo(x, y) => {
                out.push_str(&format!(" m {} {}", num(x), num(y)));
            }
            Cmd::LineTo(x, y) => {
                out.push_str(&format!(" l {} {}", num(x), num(y)));
            }
            Cmd::Curve4(x1, y1, x2, y2, x3, y3) => {
                out.push_str(&format!(
                    " c {} {} {} {} {} {}",
                    num(x1),
                    num(y1),
                    num(x2),
                    num(y2),
                    num(x3),
                    num(y3)
                ));
            }
            Cmd::Close => out.push_str(" z"),
        }
    }
    out
}

/// The `agg-rust` version `Cargo.lock` resolved to, or `"unknown"` if the lock
/// layout ever changes.  Provenance only - nothing branches on this.
fn agg_rust_version() -> &'static str {
    let mut lines = CARGO_LOCK.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "name = \"agg-rust\"" {
            continue;
        }
        // Stay inside this `[[package]]` block: blank line ends it.
        for following in lines.by_ref() {
            let following = following.trim();
            if following.is_empty() {
                return "unknown";
            }
            if let Some(rest) = following.strip_prefix("version = \"") {
                return rest.trim_end_matches('"');
            }
        }
    }
    "unknown"
}

/// Runs git inside the agg-gui crate directory, or `None` if git is unavailable
/// or the command failed (an exported copy with no `.git` is a normal case).
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_string())
}

/// Which agg-gui revision produced this fixture: the explicitly passed sha if
/// given, else `git rev-parse HEAD`, else `"unknown"`.  A dirty working tree is
/// flagged, because a bare commit id would then imply the recorded revision
/// fully describes the code that generated these bytes when it does not.
fn agg_gui_commit(explicit: Option<String>) -> String {
    if let Some(sha) = explicit {
        return sha;
    }

    match git(&["rev-parse", "HEAD"]) {
        Some(sha) if !sha.is_empty() => {
            let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
            if dirty {
                format!("{sha}-dirty")
            } else {
                sha
            }
        }
        _ => "unknown".to_string(),
    }
}

fn write_blob(dir: &Path, name: &str, bytes: &[u8]) {
    let path: PathBuf = dir.join(name);
    fs::write(&path, bytes).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/lcd_reference_fixture".to_string()),
    );
    let commit = agg_gui_commit(args.next());
    fs::create_dir_all(&out_dir).expect("creating output directory");

    // The filter reads these two typography globals and only takes the
    // byte-exact integer path while they sit at their defaults.  The fixture
    // pins that path, so assert rather than assume - and record the values in
    // the manifest so a future divergence can be attributed.
    let primary = agg_gui::font_settings::current_primary_weight();
    let gamma = agg_gui::font_settings::current_gamma();
    assert!(
        (primary - 1.0 / 3.0).abs() < 1e-12 && (gamma - 1.0).abs() < 1e-12,
        "fixture requires the default filter parameters (primary 1/3, gamma 1), \
         got primary={primary} gamma={gamma}"
    );

    let mut manifest = String::new();
    manifest.push_str("# agg-gui LCD coverage reference fixture\n");
    manifest.push_str("# Generated by agg-gui/examples/lcd_reference_fixture.rs\n");
    manifest.push_str("# Numbers are <decimal>#<raw f64 bits, 16 hex digits>; parse the bits.\n");
    manifest.push_str("# Clip is left/bottom/right/top in mask pixels (right = x + w).\n");
    manifest.push_str("# Blob layouts: .lcd/.gray = width*height*3 packed R,G,B coverage, rows\n");
    manifest.push_str("# Y-up (row 0 = bottom).  .dst = dst_w*dst_h*4 straight RGBA, also Y-up.\n");
    manifest.push_str("# .bufcolor/.bufalpha = LcdBuffer planes, w*h*3 packed R,G,B, Y-up:\n");
    manifest.push_str("# premultiplied per-channel colour and per-channel alpha.\n");
    // Provenance, informational only: the consuming side skips comment lines. It
    // records which reference these bytes came from so a future divergence can be
    // bisected against the reference's history instead of guessed at.
    manifest.push_str(&format!("# Reference: agg-gui commit {commit}\n"));
    manifest.push_str(&format!(
        "# Reference: agg-rust {} (workspace Cargo.lock)\n",
        agg_rust_version()
    ));
    manifest.push_str("version 1\n");
    manifest.push_str(&format!(
        "filter primary {} gamma {}\n",
        num(primary),
        num(gamma)
    ));

    let all = cases();
    for case in &all {
        let lcd = build_mask(case, false);
        let gray = build_mask(case, true);
        assert_eq!(lcd.width, case.mask_w);
        assert_eq!(lcd.height, case.mask_h);

        let lcd_name = format!("{}.lcd", case.name);
        let gray_name = format!("{}.gray", case.name);
        write_blob(&out_dir, &lcd_name, &lcd.data);
        write_blob(&out_dir, &gray_name, &gray.data);

        manifest.push_str(&format!("case {}\n", case.name));
        manifest.push_str(&format!("size {} {}\n", case.mask_w, case.mask_h));
        manifest.push_str(&format!("fill {}\n", fill_rule_name(case.fill_rule)));
        match case.clip {
            None => manifest.push_str("clip none\n"),
            Some((x, y, w, h)) => manifest.push_str(&format!(
                "clip {} {} {} {}\n",
                num(x),
                num(y),
                num(x + w),
                num(y + h)
            )),
        }
        manifest.push_str(&format!(
            "xform {} {} {} {} {} {}\n",
            num(case.xform.sx),
            num(case.xform.shy),
            num(case.xform.shx),
            num(case.xform.sy),
            num(case.xform.tx),
            num(case.xform.ty)
        ));
        for path in &case.paths {
            manifest.push_str(&path_line(path));
            manifest.push('\n');
        }
        manifest.push_str(&format!("lcd {} {}\n", lcd_name, lcd.data.len()));
        manifest.push_str(&format!("gray {} {}\n", gray_name, gray.data.len()));

        if let Some(spec) = case.composite {
            let mut dst = make_dst(&spec);
            composite_lcd_mask(
                &mut dst,
                spec.dst_w,
                spec.dst_h,
                &lcd,
                byte_color(spec.src),
                spec.origin.0,
                spec.origin.1,
            );
            let dst_name = format!("{}.dst", case.name);
            write_blob(&out_dir, &dst_name, &dst);
            let fill = match spec.fill {
                DstFill::Solid(c) => {
                    format!("solid {} {} {} {}", c[0], c[1], c[2], c[3])
                }
                DstFill::Halves(split, a, b) => format!(
                    "halves {} {} {} {} {} {} {} {} {}",
                    split, a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]
                ),
            };
            manifest.push_str(&format!(
                "composite {} {} {} {} {} src {} {} {} {} dstfill {}\n",
                dst_name,
                spec.dst_w,
                spec.dst_h,
                spec.origin.0,
                spec.origin.1,
                spec.src[0],
                spec.src[1],
                spec.src[2],
                spec.src[3],
                fill
            ));
        }
        if let Some(spec) = case.buffer {
            let mut buffer = LcdBuffer::new(spec.buf_w, spec.buf_h);
            buffer.clear(byte_color(spec.clear));
            let mut paths: Vec<PathStorage> =
                case.paths.iter().map(|p| to_path_storage(p)).collect();
            for path in paths.iter_mut() {
                buffer.fill_path(
                    path,
                    byte_color(spec.src),
                    &case.xform,
                    case.clip,
                    case.fill_rule,
                );
            }
            let color_name = format!("{}.bufcolor", case.name);
            let alpha_name = format!("{}.bufalpha", case.name);
            write_blob(&out_dir, &color_name, buffer.color_plane());
            write_blob(&out_dir, &alpha_name, buffer.alpha_plane());
            manifest.push_str(&format!(
                "buffer {} {} {} {} clear {} {} {} {} src {} {} {} {}\n",
                color_name,
                alpha_name,
                spec.buf_w,
                spec.buf_h,
                spec.clear[0],
                spec.clear[1],
                spec.clear[2],
                spec.clear[3],
                spec.src[0],
                spec.src[1],
                spec.src[2],
                spec.src[3],
            ));
        }
        manifest.push_str("end\n");
    }

    let manifest_path = out_dir.join("manifest.txt");
    fs::write(&manifest_path, manifest).expect("writing manifest");
    println!(
        "wrote {} cases to {}",
        all.len(),
        out_dir.canonicalize().unwrap_or(out_dir.clone()).display()
    );
}
