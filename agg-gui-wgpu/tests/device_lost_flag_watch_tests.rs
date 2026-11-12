//! `DeviceLostFlag::watch` on a device the test opens itself, as a renderer
//! does for its own compute device: the flag stays down while the device
//! lives, is raised with wgpu's reason by the poll that finds the destroyed
//! device's queue empty, and takes over the device's lost callback, so a
//! callback set before it, or a flag watched before it, is no longer told.
//!
//! The loss is real (`wgpu::Device::destroy`). Skipped (passes trivially)
//! when no GPU adapter is available.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agg_gui_wgpu::DeviceLostFlag;

/// A device and its queue on an instance of the test's own, or `None`
/// without an adapter. The queue is kept: wgpu runs a destroyed device's
/// lost callback only from a poll that finds the device's queue empty, and a
/// device whose queue is dropped has none to find.
fn own_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter =
        match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        {
            Ok(adapter) => adapter,
            Err(e) => {
                eprintln!("SKIP: no GPU adapter ({e})");
                return None;
            }
        };
    let opened = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .expect("open a device on the adapter");
    Some(opened)
}

#[test]
fn a_watched_device_raises_its_flag_when_it_is_lost() {
    let Some((device, _queue)) = own_device() else {
        return;
    };
    let flag = DeviceLostFlag::watch(&device);
    let kept = flag.clone();
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("a live device polls");
    assert!(!flag.is_lost(), "a live device is not lost");
    assert_eq!(flag.lost_reason(), None);

    device.destroy();
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    assert!(kept.is_lost(), "the poll after destroy() raises the flag");
    let reason = kept.lost_reason().expect("a lost device has a reason");
    assert!(
        reason.starts_with("Destroyed"),
        "the reason is wgpu's: {reason}"
    );
}

#[test]
fn watching_a_device_replaces_its_lost_callback() {
    let Some((device, _queue)) = own_device() else {
        return;
    };
    let told = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&told);
    device.set_device_lost_callback(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let first = DeviceLostFlag::watch(&device);
    let second = DeviceLostFlag::watch(&device);

    device.destroy();
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    assert!(second.is_lost(), "the last watch is told of the loss");
    assert!(
        !first.is_lost(),
        "a flag watched before the last watch is no longer told"
    );
    assert_eq!(
        told.load(Ordering::SeqCst),
        0,
        "a callback set before the watch is no longer called"
    );
}
