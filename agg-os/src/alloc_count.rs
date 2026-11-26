//! A global allocator that counts allocations (feature `counting-alloc`).
//!
//! [`CountingAlloc`] forwards every call to [`std::alloc::System`] and counts
//! each `alloc` and `realloc`; [`allocations`] reads the count. A bench
//! installs it as the process's `#[global_allocator]` and reports the
//! allocations a pass makes (agg-gui's `examples/text_measure_bench.rs`).
//! Installing it is safe code: the `unsafe impl` lives here.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

/// Counts every allocation (and reallocation) the process makes, once it is
/// installed with `#[global_allocator] static GLOBAL: CountingAlloc = CountingAlloc;`.
pub struct CountingAlloc;

static ALLOCS: AtomicU64 = AtomicU64::new(0);

/// How many times the process has allocated or reallocated through
/// [`CountingAlloc`] so far. Every thread's allocations count; the number
/// never goes down.
pub fn allocations() -> u64 {
    ALLOCS.load(Ordering::Relaxed)
}

// SAFETY: forwards every call unchanged to the system allocator, so it keeps
// every guarantee `GlobalAlloc` asks of an allocator exactly as `System` does;
// the only addition is a relaxed counter increment, which neither allocates
// nor affects the allocation.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for
        // `layout` (a non-zero size), which is `System.alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller guarantees `ptr` came from this allocator with
        // `layout`; every block this allocator hands out is `System`'s.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller guarantees `ptr` came from this allocator with
        // `layout` and that `new_size` is valid for it; every block this
        // allocator hands out is `System`'s.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
