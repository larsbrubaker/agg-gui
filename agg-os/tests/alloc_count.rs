//! `agg_os::alloc_count` installed as this test binary's global allocator:
//! the count goes up when the test allocates and never goes down. Other
//! threads of the harness allocate too, so the checks are "at least".

#![cfg(feature = "counting-alloc")]

use agg_os::alloc_count::{allocations, CountingAlloc};

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

#[test]
fn a_box_counts_one_allocation_at_least() {
    let before = allocations();
    let boxed = std::hint::black_box(Box::new([7u8; 64]));
    let after = allocations();
    assert!(after > before, "{before} -> {after}");
    drop(boxed);
    assert!(allocations() >= after, "the count went down after a free");
}

#[test]
fn a_growing_vec_counts_its_reallocations() {
    let mut v: Vec<u64> = Vec::with_capacity(1);
    v.push(1);
    let before = allocations();
    // Growing from capacity 1 to 1024 grows the buffer several times, each a
    // realloc (how many is std's business).
    for i in 0..1024 {
        v.push(std::hint::black_box(i));
    }
    let after = allocations();
    assert!(after > before, "{before} -> {after}");
    drop(v);
    assert!(allocations() >= after, "the count went down after a free");
}
