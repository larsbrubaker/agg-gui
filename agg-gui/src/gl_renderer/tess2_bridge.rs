//! tess2 bridge — converts 2D polygon contours into GL triangle meshes.
//!
//! # Architecture (based on MatterCAD agg-sharp reference)
//!
//! Both fills and AA strokes follow the same pipeline:
//!
//! ```text
//! Contours (Vec<Vec<[f32;2]>>)
//!   │  (stroke: pre-expand via AGG Stroke first)
//!   ▼
//! tess2::Tessellator::add_contour()   ← one call per contour ring
//!   ▼
//! tess2::tessellate(NonZero, Polygons, 3, 2)
//!   ▼
//! out_vertices: flat [x,y, x,y, …]
//! out_elements: flat [i0,i1,i2, i0,i1,i2, …]   ← triangle indices
//!   ▼
//! (Vec<f32>, Vec<u32>)   ready for glBufferData
//! ```
//!
//! Anti-aliased edge expansion (for strokes) follows the approach in
//! `AARenderTesselator.cs`: each boundary edge is expanded by 1 px outward and
//! a coverage ramp (0 → alpha) is applied across the expansion quad. For now
//! we implement non-AA fill tessellation; AA strokes are planned as Phase D ext.

use crate::draw_ctx::FillRule;
use agg_rust::basics::{is_end_poly, is_move_to, is_stop, VertexSource};
use tess2_rust::{ElementType, Tessellator, WindingRule};

// ---------------------------------------------------------------------------
// Panic-isolation + repro capture
// ---------------------------------------------------------------------------
//
// tess2-rust still hits an unresolved porting bug on WASM where a sweep
// region keeps a reference to an edge whose origin vertex was wiped by
// `kill_vertex(_, INVALID)`, and the sweep then dereferences
// `mesh.verts[INVALID as usize]` and traps with "index out of bounds".
// We can't reproduce it natively even with the lion stress harness, so
// the goal of this layer is to capture **the exact input contour set**
// that triggers the panic on the deployed wasm build so we can paste it
// back into a tess2 regression test and fix the real algorithm bug.
//
// Two complementary mechanisms:
//
//   1. `try_tessellate` runs the pipeline inside `catch_unwind`.  On
//      native this catches the panic, dumps the input, and returns
//      `None` so the caller degrades gracefully.  On
//      `wasm32-unknown-unknown` `catch_unwind` is a no-op (no unwinder
//      runtime), so this branch only protects native callers.
//
//   2. While `try_tessellate` is running it stashes `(contours,
//      winding, label)` in a thread-local; if a panic fires before the
//      function returns, the panic **hook** (installed by
//      [`install_tess_panic_logger`]) reads that thread-local and
//      dumps the same repro info to `console.error` *before* the WASM
//      module aborts.  Panic hooks run regardless of unwind/abort, so
//      this is what actually surfaces the failing input on wasm.
//
// Once the underlying tess2 bug is gone this layer becomes harmless
// dead code; we keep it around because tess2 is a C-port-style library
// and any future contract violation would otherwise present as a
// generic `RuntimeError: unreachable` with no actionable input.

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
struct TessContext {
    contours: Vec<Vec<[f32; 2]>>,
    winding: WindingRule,
    label: &'static str,
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// Most recent `(contours, winding, label)` handed to
    /// [`try_tessellate`].  Cleared on a clean return; left intact when
    /// the call panics so the panic hook can dump it.  `RefCell` lets
    /// the hook borrow it without taking ownership.
    static LAST_TESS_CONTEXT: RefCell<Option<TessContext>> = const { RefCell::new(None) };
}

/// Best-effort dump of the contour set that triggered a tess2 panic.
///
/// On wasm32 this lands on `console.error` so it's visible to the user
/// and to anyone we ask to copy-paste the failing input.  On native it
/// goes to stderr so unit/integration tests still capture it.  The
/// emitted format is intentionally `f64` literal-friendly so we can drop
/// it straight into a Rust test as `&[(f64, f64)]`.
fn log_tess_repro(contours: &[Vec<[f32; 2]>], winding: WindingRule, label: &str) {
    use std::fmt::Write;
    let total: usize = contours.iter().map(|c| c.len()).sum();
    let mut buf = String::new();
    let _ = writeln!(
        buf,
        "tess2 repro for {label} — winding={winding:?}, contours={}, points={}:",
        contours.len(),
        total
    );
    for (i, c) in contours.iter().enumerate() {
        let _ = write!(buf, "  contour[{i}] (n={}) [", c.len());
        for (j, pt) in c.iter().enumerate() {
            if j > 0 {
                buf.push_str(", ");
            }
            let _ = write!(buf, "({:.6}, {:.6})", pt[0], pt[1]);
        }
        let _ = writeln!(buf, "]");
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&buf));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("{buf}");
    }
}

/// Install a panic hook that dumps the most recent tess2 input set
/// (recorded by [`try_tessellate`]) before delegating to whatever hook
/// was previously installed (typically `console_error_panic_hook`).
///
/// Call this **after** `console_error_panic_hook::set_once()` so that
/// our hook chains through the panic-info logger instead of replacing
/// it.  Safe to call multiple times — the previous hook is captured
/// each call, so duplicate installs just stack.  Native builds also
/// benefit (e.g. integration tests get the contour dump on `panic!`),
/// though the `catch_unwind` path in `try_tessellate` already covers
/// the common native case.
pub fn install_tess_panic_logger() {
    #[cfg(target_arch = "wasm32")]
    {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Read the stashed input.  Borrowing rather than taking so a
            // panic during repro logging doesn't lose the data on the
            // next failed frame.
            LAST_TESS_CONTEXT.with(|cell| {
                if let Ok(borrow) = cell.try_borrow() {
                    if let Some(ctx) = borrow.as_ref() {
                        log_tess_repro(&ctx.contours, ctx.winding, ctx.label);
                    }
                }
            });
            prev(info);
        }));
    }
}

/// Run the tess2 pipeline for `contours` under `winding`.
///
/// On native, the call is wrapped in `catch_unwind` so a tess2 panic
/// dumps the input and returns `None` instead of crashing the host.
/// On wasm the same dump is produced via the panic hook installed by
/// [`install_tess_panic_logger`] (catch_unwind can't unwind in
/// wasm32-unknown-unknown), and the call still aborts the page —
/// preventing that requires fixing the underlying tess2 bug.
///
/// `extract` produces the caller-shaped result from the tessellator
/// once it has finished without panicking; it never runs on the panic
/// path.
fn try_tessellate<T, F>(
    contours: &[Vec<[f32; 2]>],
    winding: WindingRule,
    label: &'static str,
    extract: F,
) -> Option<T>
where
    F: FnOnce(&Tessellator) -> Option<T>,
{
    use std::panic::{catch_unwind, AssertUnwindSafe};

    if contours.is_empty() {
        return None;
    }

    // Stash a copy of the input so the wasm panic hook can dump it if
    // the call below aborts.  Released on the clean-return paths and
    // on the catch_unwind error path.  The clone is only paid on wasm
    // (gating the cfg here keeps native zero-cost).
    #[cfg(target_arch = "wasm32")]
    LAST_TESS_CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(TessContext {
            contours: contours.to_vec(),
            winding,
            label,
        });
    });

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut tess = Tessellator::new();
        for c in contours {
            // Promote f32 → f64 at the boundary so the sweep's edge-sign
            // predicates have the full f64 margin (rotation-stable
            // topology).  See the Real-type comment in tess2-rust geom.rs.
            let flat: Vec<f64> = c.iter().flat_map(|v| [v[0] as f64, v[1] as f64]).collect();
            tess.add_contour(2, &flat);
        }
        let ok = tess.tessellate(winding, ElementType::Polygons, 3, 2, None);
        if !ok || tess.vertex_count() == 0 {
            return None;
        }
        extract(&tess)
    }));

    #[cfg(target_arch = "wasm32")]
    LAST_TESS_CONTEXT.with(|cell| {
        *cell.borrow_mut() = None;
    });

    match result {
        Ok(v) => v,
        Err(_payload) => {
            // Native catch_unwind hit — the panic hook also fires on
            // wasm, but on native there's no panic hook installed for
            // tess2, so log here too.
            log_tess_repro(contours, winding, label);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Universal AGG VertexSource → tess2 contours
// ---------------------------------------------------------------------------

/// Walk any AGG `VertexSource` (a `PathStorage`, a `ConvStroke<…>`, a
/// `ConvCurve<…>`, an `Ellipse`, etc.) and produce tess2-ready contours.
///
/// Protocol (mirrors MatterCAD's `VertexSourceToTesselator.SendShapeToTesselator`):
///   - `move_to` → start a new contour.
///   - `line_to` → append vertex to the current contour.
///   - `end_poly` / implicit close → finalise the current contour.
///   - `stop` → we're done.
///
/// Degenerate contours (< 3 vertices after de-duplication) are dropped, since
/// tess2 can't triangulate them and frequently panics on zero-length edges.
///
/// This is the ONE place in the codebase that does path → contour conversion.
/// Both fill and stroke rendering go through it so the two code paths share
/// the same battle-tested handling of close/end-poly semantics.
pub fn agg_path_to_contours<VS: VertexSource>(path: &mut VS) -> Vec<Vec<[f32; 2]>> {
    let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();

    path.rewind(0);
    loop {
        let (mut x, mut y) = (0.0, 0.0);
        let cmd = path.vertex(&mut x, &mut y);
        if is_stop(cmd) {
            break;
        }
        if is_move_to(cmd) {
            // Finish the previous contour before starting a new one.
            if !cur.is_empty() {
                push_contour(&mut out, std::mem::take(&mut cur));
            }
            cur.push([x as f32, y as f32]);
        } else if is_end_poly(cmd) {
            if !cur.is_empty() {
                push_contour(&mut out, std::mem::take(&mut cur));
            }
        } else {
            // line_to (drawing command): append to current contour.
            cur.push([x as f32, y as f32]);
        }
    }
    if !cur.is_empty() {
        push_contour(&mut out, cur);
    }
    out
}

fn push_contour(out: &mut Vec<Vec<[f32; 2]>>, mut contour: Vec<[f32; 2]>) {
    // De-duplicate consecutive identical vertices and strip a trailing
    // closing duplicate (first == last) — tess2 panics on zero-length
    // edges and on redundant closing vertices.
    contour = deduplicate_contour_v(&contour);
    if contour.len() < 3 {
        return;
    }
    if signed_area_2x(&contour).abs() < 1.0 {
        return;
    }
    out.push(contour);
}

/// Tessellate any AGG vertex source.  Convenience wrapper over
/// [`agg_path_to_contours`] + [`tessellate_fill`] — use this for every fill /
/// stroke rendering path so there's a single code-path from an AGG path to
/// GPU triangles.
pub fn tessellate_path<VS: VertexSource>(path: &mut VS) -> Option<(Vec<f32>, Vec<u32>)> {
    let contours = agg_path_to_contours(path);
    if contours.is_empty() {
        return None;
    }
    tessellate_fill(&contours)
}

// ---------------------------------------------------------------------------
// Analytic edge AA via tess2 edge flags
// ---------------------------------------------------------------------------

/// Tessellate `path` and produce **`(x, y, alpha)` vertices + triangle
/// indices** for analytic edge-AA rendering.
///
/// For every triangle-vertex tess2 emits, the matching
/// [`edge_flags`](tess2_rust::Tessellator::edge_flags) entry tells us
/// whether the edge starting at that vertex (going CCW around the same
/// triangle) is an **original polygon boundary edge**.  For each such
/// boundary edge we emit a 1-pixel-wide halo quad extending OUTWARD:
///   - inner vertices (on the polygon edge): alpha = 1.0
///   - outer vertices (one pixel outside):   alpha = 0.0
/// The GPU linearly interpolates alpha across the halo, giving a clean
/// analytic edge-coverage ramp at the silhouette — no hardware MSAA needed.
///
/// This mirrors the MatterCAD agg-sharp `AARenderTesselator` strategy, with
/// a shader-side alpha attribute instead of a texture-coord trick.
///
/// `halo_px` is the halo strip width in logical pixels; `1.0` is the
/// convention (gives one pixel of alpha falloff outward from the boundary).
/// Cached result of tessellating an AGG path **once** at load time.
///
/// Callers who want to rotate / scale / skew the same geometry across
/// many frames should use [`tessellate_interior`] to build one of these,
/// stash it, and call [`expand_aa_halo`] every frame with the transformed
/// vertex positions.  That keeps the triangle set and its edge flags
/// deterministic — the same polygons render the same way regardless of
/// the current transform, which matters because tess2 is not
/// numerically stable across all transforms (small precision changes in
/// input coords can drop or re-order whole polygons in its output).
///
/// The battle-tested MatterCAD agg-sharp `CachedTesselator` pattern: tess
/// once, apply the view transform afterwards.
#[derive(Clone, Debug)]
pub struct CachedTess {
    /// Interior triangle vertices in the caller's input coord system
    /// (flat `[x, y, x, y, …]`).
    pub vertices: Vec<f32>,
    /// Triangle vertex indices into `vertices` (each triple is one triangle).
    pub indices: Vec<u32>,
    /// Parallel to `indices`: `1` if the edge starting at this vertex
    /// (going CCW around the triangle) is an **original polygon boundary
    /// edge**, `0` if it's an interior edge added by the tessellation.
    /// Used by [`expand_aa_halo`] to build 1-pixel halo strips only along
    /// real silhouette edges.
    pub edge_flags: Vec<u8>,
}

/// Tessellate any AGG `VertexSource` ONCE into a cacheable triangle list
/// with edge flags.  Does not build halo strips — see [`expand_aa_halo`].
///
/// Returns `None` if the path yielded no usable triangles (empty path or
/// a degenerate shape tess2 can't handle).
pub fn tessellate_interior<VS: VertexSource>(path: &mut VS) -> Option<CachedTess> {
    let contours = agg_path_to_contours(path);
    try_tessellate(
        &contours,
        WindingRule::Odd,
        "tessellate_interior",
        |tess| {
            Some(CachedTess {
                vertices: tess.vertices().iter().map(|&v| v as f32).collect(),
                indices: tess.elements().to_vec(),
                edge_flags: tess.edge_flags().to_vec(),
            })
        },
    )
}

/// Given a pre-computed `CachedTess` whose vertices have already been
/// transformed into screen space, emit `(x, y, alpha)` vertices + triangle
/// indices for the halo-AA solid pipeline.  Interior triangles get
/// alpha = 1.0 on every vertex; every boundary edge additionally spawns a
/// `halo_px`-wide outward quad with the outer pair at alpha = 0.0, giving
/// analytic 1-pixel edge coverage.
///
/// The outward normal is taken in screen space (`(dy, -dx)` of each edge
/// direction, Y-up CCW → right = outside), so the halo is always exactly
/// `halo_px` logical pixels wide regardless of the transform applied to
/// the interior vertices.
pub fn expand_aa_halo(
    transformed_xy: &[f32],
    cached: &CachedTess,
    halo_px: f32,
) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    let n_interior = transformed_xy.len() / 2;
    let n_indices = cached.indices.len();
    if n_indices == 0 {
        return None;
    }

    let mut out_verts: Vec<[f32; 3]> = Vec::with_capacity(n_interior + n_indices * 4);
    let mut out_indices: Vec<u32> = Vec::with_capacity(n_indices + n_indices * 2);

    for i in 0..n_interior {
        out_verts.push([transformed_xy[i * 2], transformed_xy[i * 2 + 1], 1.0]);
    }
    out_indices.extend_from_slice(&cached.indices);

    let n_tris = n_indices / 3;
    for t in 0..n_tris {
        let ia = cached.indices[t * 3] as usize;
        let ib = cached.indices[t * 3 + 1] as usize;
        let ic = cached.indices[t * 3 + 2] as usize;
        if ia >= n_interior || ib >= n_interior || ic >= n_interior {
            continue;
        }
        let p = [
            [transformed_xy[ia * 2], transformed_xy[ia * 2 + 1]],
            [transformed_xy[ib * 2], transformed_xy[ib * 2 + 1]],
            [transformed_xy[ic * 2], transformed_xy[ic * 2 + 1]],
        ];
        let flag = [
            cached.edge_flags.get(t * 3).copied().unwrap_or(0),
            cached.edge_flags.get(t * 3 + 1).copied().unwrap_or(0),
            cached.edge_flags.get(t * 3 + 2).copied().unwrap_or(0),
        ];
        for k in 0..3 {
            if flag[k] == 0 {
                continue;
            }
            let a = p[k];
            let b = p[(k + 1) % 3];
            let c = p[(k + 2) % 3];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                continue;
            }
            // Right-hand perpendicular, flipped if it points into the
            // triangle (toward `c`).  See `tessellate_path_aa` for the
            // full explanation.
            let mut nx = dy / len * halo_px;
            let mut ny = -dx / len * halo_px;
            let dot_c = nx * (c[0] - a[0]) + ny * (c[1] - a[1]);
            if dot_c > 0.0 {
                nx = -nx;
                ny = -ny;
            }
            let base = out_verts.len() as u32;
            out_verts.push([a[0], a[1], 1.0]);
            out_verts.push([b[0], b[1], 1.0]);
            out_verts.push([a[0] + nx, a[1] + ny, 0.0]);
            out_verts.push([b[0] + nx, b[1] + ny, 0.0]);
            out_indices.extend_from_slice(&[
                base,
                base + 1,
                base + 2,
                base + 1,
                base + 3,
                base + 2,
            ]);
        }
    }

    Some((out_verts, out_indices))
}

pub fn tessellate_path_aa<VS: VertexSource>(
    path: &mut VS,
    halo_px: f32,
    fill_rule: FillRule,
) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    let contours = agg_path_to_contours(path);
    if contours.is_empty() {
        return None;
    }

    struct TessOut {
        verts: Vec<f32>,
        indices: Vec<u32>,
        flags: Vec<u8>,
        vcount: usize,
    }
    let out = try_tessellate(
        &contours,
        to_tess_winding_rule(fill_rule),
        "tessellate_path_aa",
        |tess| {
            Some(TessOut {
                verts: tess.vertices().iter().map(|&v| v as f32).collect(),
                indices: tess.elements().to_vec(),
                flags: tess.edge_flags().to_vec(),
                vcount: tess.vertex_count(),
            })
        },
    )?;

    let in_verts: &[f32] = &out.verts; // flat [x, y, x, y, …]
    let in_indices: &[u32] = &out.indices; // [i0, i1, i2, …]
    let edge_flags: &[u8] = &out.flags; // parallel to in_indices

    let n_interior = out.vcount;
    let n_indices = in_indices.len();
    if n_indices == 0 {
        return None;
    }

    let mut out_verts: Vec<[f32; 3]> = Vec::with_capacity(n_interior + n_indices * 4);
    let mut out_indices: Vec<u32> = Vec::with_capacity(n_indices + n_indices * 2);

    // Interior triangles — alpha 1.0 everywhere.
    for i in 0..n_interior {
        out_verts.push([in_verts[i * 2], in_verts[i * 2 + 1], 1.0]);
    }
    out_indices.extend_from_slice(in_indices);

    // Halo strips — one quad per boundary edge, always extruded AWAY from
    // the triangle's third vertex.
    //
    // Mirrors MatterCAD agg-sharp `AARenderTesselator.Draw1EdgeTriangle`:
    // compute the right-hand perpendicular of the edge, then flip its sign
    // if it points toward the third (non-edge) vertex.  A single winding
    // assumption isn't reliable — tess2 can emit CW triangles for CW-input
    // polygons or for internal regions of self-intersecting inputs, and
    // those would have gotten their halo pushed INWARD (invisible), which
    // is what produced the jagged lion silhouette edges.
    let n_tris = n_indices / 3;
    for t in 0..n_tris {
        let ia = in_indices[t * 3] as usize;
        let ib = in_indices[t * 3 + 1] as usize;
        let ic = in_indices[t * 3 + 2] as usize;
        if ia >= n_interior || ib >= n_interior || ic >= n_interior {
            continue;
        }
        let p = [
            [in_verts[ia * 2], in_verts[ia * 2 + 1]],
            [in_verts[ib * 2], in_verts[ib * 2 + 1]],
            [in_verts[ic * 2], in_verts[ic * 2 + 1]],
        ];
        let flag = [
            edge_flags.get(t * 3).copied().unwrap_or(0),
            edge_flags.get(t * 3 + 1).copied().unwrap_or(0),
            edge_flags.get(t * 3 + 2).copied().unwrap_or(0),
        ];
        for k in 0..3 {
            if flag[k] == 0 {
                continue;
            }
            let a = p[k];
            let b = p[(k + 1) % 3];
            let c = p[(k + 2) % 3]; // third vertex — the "nonAaPoint"
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                continue;
            }
            // Right-hand perpendicular of (dx, dy).  Sign is flipped below
            // if it ends up pointing toward `c` (i.e. into the triangle).
            let mut nx = dy / len * halo_px;
            let mut ny = -dx / len * halo_px;
            let dot_c = nx * (c[0] - a[0]) + ny * (c[1] - a[1]);
            if dot_c > 0.0 {
                nx = -nx;
                ny = -ny;
            }

            let base = out_verts.len() as u32;
            out_verts.push([a[0], a[1], 1.0]); // 0: inner a
            out_verts.push([b[0], b[1], 1.0]); // 1: inner b
            out_verts.push([a[0] + nx, a[1] + ny, 0.0]); // 2: outer a
            out_verts.push([b[0] + nx, b[1] + ny, 0.0]); // 3: outer b
            out_indices.extend_from_slice(&[
                base,
                base + 1,
                base + 2,
                base + 1,
                base + 3,
                base + 2,
            ]);
        }
    }

    Some((out_verts, out_indices))
}

fn to_tess_winding_rule(fill_rule: FillRule) -> WindingRule {
    match fill_rule {
        FillRule::NonZero => WindingRule::NonZero,
        FillRule::EvenOdd => WindingRule::Odd,
    }
}

/// Tessellate a filled polygon described by one or more contour rings.
///
/// Each contour is a list of 2-D vertices in Y-up order.  Outer contours
/// should be wound counter-clockwise; holes clockwise (standard even-odd /
/// non-zero fill rules).
///
/// Returns `(flat_xy_vertices, triangle_indices)` where:
/// - `flat_xy_vertices`: interleaved `[x0, y0, x1, y1, …]` as `f32`
/// - `triangle_indices`: triples referencing vertex positions (indices into the
///   vertex array — divide by 2 to get the vertex number)
///
/// Returns `None` if tessellation fails or produces no output.
pub fn tessellate_fill(contours: &[Vec<[f32; 2]>]) -> Option<(Vec<f32>, Vec<u32>)> {
    if contours.is_empty() {
        return None;
    }

    // Pre-clean every contour the same way `try_tessellate` would
    // forward it to tess2: drop short / zero-area / dup-vertex rings so
    // the tessellator never sees the obviously-degenerate cases that
    // tend to surface latent porting bugs.
    let cleaned: Vec<Vec<[f32; 2]>> = contours
        .iter()
        .filter_map(|contour| {
            if contour.len() < 3 {
                return None;
            }
            let c = deduplicate_contour(contour);
            if c.len() < 3 {
                return None;
            }
            // Any polygon with area < 0.5 px² is invisible anyway and
            // tess2 panics on collinear faces instead of returning an
            // error, so filter them out at the boundary.
            if signed_area_2x(&c).abs() < 1.0 {
                return None;
            }
            Some(c)
        })
        .collect();

    try_tessellate(
        &cleaned,
        // EvenOdd — NonZero panics in tess2-rust on some inputs.
        WindingRule::Odd,
        "tessellate_fill",
        |tess| {
            Some((
                tess.vertices().iter().map(|&v| v as f32).collect(),
                tess.elements().to_vec(),
            ))
        },
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 2× the signed area of a polygon (positive = CCW in Y-up).
/// If |result| < 1.0 the polygon covers less than 0.5 px² and is invisible.
fn signed_area_2x(pts: &[[f32; 2]]) -> f32 {
    let n = pts.len();
    let mut a = 0.0f32;
    for i in 0..n {
        let j = (i + 1) % n;
        a += pts[i][0] * pts[j][1] - pts[j][0] * pts[i][1];
    }
    a
}

/// Remove consecutive duplicate vertices and strip the closing duplicate if
/// the contour ends with a copy of its first vertex.
fn deduplicate_contour(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    deduplicate_contour_v(pts)
}

/// Same as `deduplicate_contour` but with a shorter name suitable for
/// internal re-use — kept as a separate symbol to avoid churning the public
/// one while the path converter lands.
fn deduplicate_contour_v(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(pts.len());
    for &pt in pts {
        match out.last() {
            Some(&prev) if prev == pt => {}
            _ => out.push(pt),
        }
    }
    if out.len() >= 2 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// Convert an axis-aligned rectangle into a single contour and tessellate it.
/// Useful for solid-colour fills without a full path builder.
pub fn tessellate_rect(x: f32, y: f32, w: f32, h: f32) -> Option<(Vec<f32>, Vec<u32>)> {
    let contour = vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    tessellate_fill(&[contour])
}

/// Convert a rounded rectangle into a contour (approximated as a polygon with
/// `segments` points per quarter-circle arc) and tessellate.
pub fn tessellate_rounded_rect(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    segments: usize,
) -> Option<(Vec<f32>, Vec<u32>)> {
    let r = r.min(w * 0.5).min(h * 0.5);
    let seg = segments.max(3);
    let mut contour: Vec<[f32; 2]> = Vec::with_capacity(seg * 4 + 4);

    use std::f32::consts::PI;

    // Four arc centres (inner rect corners), CCW starting bottom-right.
    let corners = [
        (x + w - r, y + r, -PI * 0.5, 0.0),    // bottom-right
        (x + w - r, y + h - r, 0.0, PI * 0.5), // top-right
        (x + r, y + h - r, PI * 0.5, PI),      // top-left
        (x + r, y + r, PI, PI * 1.5),          // bottom-left
    ];

    for &(cx, cy, start, end) in &corners {
        for i in 0..=seg {
            let t = i as f32 / seg as f32;
            let angle = start + t * (end - start);
            contour.push([cx + angle.cos() * r, cy + angle.sin() * r]);
        }
    }

    tessellate_fill(&[contour])
}

/// Build a circle contour and tessellate it.
pub fn tessellate_circle(
    cx: f32,
    cy: f32,
    r: f32,
    segments: usize,
) -> Option<(Vec<f32>, Vec<u32>)> {
    let seg = segments.max(8);
    use std::f32::consts::TAU;
    let contour: Vec<[f32; 2]> = (0..seg)
        .map(|i| {
            let angle = i as f32 / seg as f32 * TAU;
            [cx + angle.cos() * r, cy + angle.sin() * r]
        })
        .collect();
    tessellate_fill(&[contour])
}
