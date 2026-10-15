//! Tests for how the headless device is requested (`headless.rs`'s
//! [`HeadlessOptions`]): the defaults, the setters, that the shared device
//! is the one every option set with the same adapter choice reaches, and that
//! it was opened with the memory hints asked for — seen through the
//! allocator's own report where the backend gives one (Vulkan, DX12).
//!
//! The timing half (budgets counted from the creation's start, a late device
//! reaching later callers) is driven without a GPU in
//! `gpu_shared_startup_tests.rs`. GPU tests here skip (pass trivially) when
//! the machine has no adapter, as the crate's others do.

use std::time::Duration;

use crate::headless::{HeadlessGpu, HeadlessOptions};
use crate::{GpuMemoryHints, GPU_STARTUP_BUDGET};

/// The largest device-memory block wgpu 29's allocators reserve for
/// sub-allocation under `MemoryUsage` (`Performance` starts at 128 MiB).
const MEMORY_USAGE_MAX_BLOCK: u64 = 64 * 1024 * 1024;

#[test]
fn options_default_to_the_hardware_adapter_the_window_budget_and_memory_usage() {
    let options = HeadlessOptions::new();
    assert_eq!(options, HeadlessOptions::default());
    assert!(!options.force_fallback_adapter);
    assert_eq!(options.startup_budget, GPU_STARTUP_BUDGET);
    assert_eq!(options.memory_hints, GpuMemoryHints::MemoryUsage);

    let options = options
        .with_force_fallback_adapter(true)
        .with_startup_budget(Duration::from_secs(120))
        .with_memory_hints(GpuMemoryHints::Performance);
    assert!(options.force_fallback_adapter);
    assert_eq!(options.startup_budget, Duration::from_secs(120));
    assert_eq!(options.memory_hints, GpuMemoryHints::Performance);
}

/// The first call creates the shared device and its hints win: a call with
/// another budget and other hints gets the same device, and the device says
/// which hints it was made with.
#[test]
fn every_option_set_reaches_the_one_shared_device() {
    let Ok(gpu) = HeadlessGpu::shared() else {
        eprintln!("SKIP: no adapter");
        return;
    };
    let other = HeadlessGpu::shared_with_options(
        HeadlessOptions::new()
            .with_startup_budget(Duration::from_secs(120))
            .with_memory_hints(GpuMemoryHints::Performance),
    )
    .expect("the shared device exists");
    assert!(std::ptr::eq(gpu, other));
    assert_eq!(other.memory_hints(), gpu.memory_hints());
}

/// The shared device of this test binary is opened with the default
/// `MemoryUsage` (no test here asks first for anything else), and the
/// allocator shows it: no block past `MemoryUsage`'s largest holds more than
/// the one resource it was made for. Under `Performance` the first small
/// buffer lands in a 128 MiB block of its own.
#[test]
fn the_shared_device_reserves_memory_usage_sized_blocks() {
    let Ok(gpu) = HeadlessGpu::shared() else {
        eprintln!("SKIP: no adapter");
        return;
    };
    assert_eq!(gpu.memory_hints(), GpuMemoryHints::MemoryUsage);
    let small = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("memory-hints-probe"),
        size: 4096,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let Some(report) = gpu.device().generate_allocator_report() else {
        eprintln!("SKIP: this backend gives no allocator report");
        return;
    };
    for block in &report.blocks {
        if block.size <= MEMORY_USAGE_MAX_BLOCK {
            continue;
        }
        // A block this large is a dedicated allocation: one resource that
        // fills it.
        let held = &report.allocations[block.allocations.clone()];
        assert!(
            held.len() == 1 && held[0].size * 2 >= block.size,
            "a {} MiB block holds {:?} — sub-allocation blocks this large mean \
             the device was opened with Performance hints",
            block.size / (1024 * 1024),
            held.iter().map(|a| a.size).collect::<Vec<_>>(),
        );
    }
    drop(small);
}
