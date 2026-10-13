//! Bench for repeated text measurement of unchanged text.
//!
//! Times the paths that measure text on every UI frame when nothing has
//! changed, and counts the heap allocations each makes, through the real
//! production code (nothing is reimplemented here):
//!
//! - **tree layout**: `layout()` of a column of `Label`s and `Button`s;
//! - **direct measuring**: `text::measure_advance` / `measure_text_metrics`
//!   on the same strings, which is what apps' own widgets (inline edit
//!   fields, button text metrics) call from their layout;
//! - **ink overhang**: `Widget::backbuffer_ink_outset` of every label, which
//!   the backbuffered paint path asks every frame.
//!
//! Run in release; the shape cache is warm after the first pass, so every
//! timed pass measures the steady state:
//!
//! ```text
//! cargo run --release -p agg-gui --example text_measure_bench
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use agg_gui::text::{measure_advance, measure_text_metrics, Font};
use agg_gui::widgets::{Button, FlexColumn, Label};
use agg_gui::{Size, Widget};

/// Counts every allocation (and reallocation) the process makes.
struct CountingAlloc;

static ALLOCS: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards every call unchanged to the system allocator; the only
// addition is a relaxed counter increment, which cannot affect allocation.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/LiberationSans-Regular.ttf");
const ROWS: usize = 200;
const PASSES: usize = 400;
const ROUNDS: usize = 7;

/// Runs `f` for `ROUNDS` rounds of `PASSES` passes after one warm-up pass and
/// prints the fastest round's mean time and allocations per pass. The
/// fastest round is the least disturbed by other work on the machine.
fn time(name: &str, mut f: impl FnMut()) {
    f();
    let mut best = f64::INFINITY;
    let mut allocs = 0;
    for _ in 0..ROUNDS {
        let a0 = ALLOCS.load(Ordering::Relaxed);
        let t0 = Instant::now();
        for _ in 0..PASSES {
            f();
        }
        let us = t0.elapsed().as_secs_f64() * 1e6 / PASSES as f64;
        allocs = ALLOCS.load(Ordering::Relaxed) - a0;
        best = best.min(us);
    }
    println!(
        "{name:<34} {best:>9.2} us/pass  {:>8.1} allocs/pass",
        allocs as f64 / PASSES as f64
    );
}

fn main() {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("bench font"));
    let texts: Vec<String> = (0..ROWS)
        .map(|i| format!("Row {i}: Width {:.2} mm", i as f64 * 1.25))
        .collect();

    let mut column = FlexColumn::new();
    for (i, t) in texts.iter().enumerate() {
        column = if i % 2 == 0 {
            column.add(Box::new(Label::new(t.clone(), Arc::clone(&font))))
        } else {
            column.add(Box::new(Button::new(t.clone(), Arc::clone(&font))))
        };
    }
    let available = Size::new(800.0, 100_000.0);
    println!("{ROWS} rows, best of {ROUNDS} rounds of {PASSES} passes, per pass:");
    time("tree layout (labels + buttons)", || {
        std::hint::black_box(column.layout(available));
    });
    time("measure_advance x rows", || {
        for t in &texts {
            std::hint::black_box(measure_advance(&font, t, 14.0));
        }
    });
    time("measure_text_metrics x rows", || {
        for t in &texts {
            std::hint::black_box(measure_text_metrics(&font, t, 14.0));
        }
    });
    let mut labels: Vec<Label> = texts
        .iter()
        .map(|t| Label::new(t.clone(), Arc::clone(&font)))
        .collect();
    for l in &mut labels {
        l.layout(available);
    }
    time("label ink overhang x rows", || {
        for l in &labels {
            std::hint::black_box(l.backbuffer_ink_outset());
        }
    });
}
