//! How a device's memory allocator sizes the blocks it reserves:
//! [`GpuMemoryHints`], the choice a window ([`crate::GpuConfig::memory_hints`])
//! and the headless device ([`crate::headless::HeadlessOptions::memory_hints`])
//! request their device with, mapped to `wgpu::MemoryHints` by
//! `crate::gpu::device_descriptor`.
//!
//! The choice is ours rather than `wgpu::MemoryHints` directly because that
//! type is not `Copy` (its `Manual` variant holds a `Range`), and both configs
//! are plain `Copy` values a host passes around freely.
//!
//! What the two hints cost, measured on Vulkan (GTX 1070) under wgpu 29: the
//! Vulkan and DX12 allocators reserve memory in blocks per memory type, 128 to
//! 256 MiB of device memory and 64 to 128 MiB of host memory per block with
//! `Performance`, 8 to 64 MiB and 4 to 32 MiB with `MemoryUsage`. Opening a
//! device reserved 198 MiB with `Performance` and 18 MiB with `MemoryUsage`;
//! with one 3D view's textures, 768 MiB against 628 MiB. Metal and the
//! browser ignore the hint.

/// Which way a device's allocator leans when it reserves memory blocks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuMemoryHints {
    /// Large blocks: fewer native allocations as resources come and go, at
    /// the price of a few hundred MiB reserved up front. The default for a
    /// window, where one long-lived device per process serves an interactive
    /// app and allocation churn is what a frame would feel.
    #[default]
    Performance,
    /// Small blocks: the reservation follows what is actually allocated. The
    /// default for the headless device, which serves tests and offscreen
    /// renders where up-front reservation is pure overhead — paid again by
    /// every process when a test runner gives each test its own.
    MemoryUsage,
}

impl From<GpuMemoryHints> for wgpu::MemoryHints {
    fn from(hints: GpuMemoryHints) -> Self {
        match hints {
            GpuMemoryHints::Performance => wgpu::MemoryHints::Performance,
            GpuMemoryHints::MemoryUsage => wgpu::MemoryHints::MemoryUsage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GpuMemoryHints;

    #[test]
    fn each_hint_maps_to_the_wgpu_hint_of_the_same_name() {
        // `wgpu::MemoryHints` has no `PartialEq`.
        assert!(matches!(
            wgpu::MemoryHints::from(GpuMemoryHints::Performance),
            wgpu::MemoryHints::Performance
        ));
        assert!(matches!(
            wgpu::MemoryHints::from(GpuMemoryHints::MemoryUsage),
            wgpu::MemoryHints::MemoryUsage
        ));
    }
}
