//! Tests for `audio_input` without a microphone: the default input device is
//! read without prompting, each AVAuthorizationStatus maps to its
//! [`AudioAuthorization`], the sink node's AudioBufferList reads to slices
//! (planar and interleaved, a buffer shorter than its memory or without
//! data, no more buffers than the list holds), a receiver call hands its
//! cycle on and contains a panic, and the configuration-change observer
//! (`engine_change`), with the change posted by hand for a stand-in object in
//! place of the engine, calls back for its engine only, until dropped, and
//! contains a panic; and both run their closure inside a callback scope
//! naming their engine and their kind (audio, change), restored afterwards
//! (nested, and after a panic), and the two decisions the teardown takes
//! from that scope: whether to forget the observer (inside this engine's
//! change callback only) and whether to leak the engine (inside either of
//! its callbacks). That a real engine delivers and posts the change, and
//! that `unobserve`, `shut_down`, `leak` and the drop act on those
//! decisions, can only be seen live (agg-mic's `tests/live_mic.rs`) or by
//! review: it needs a real engine.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use objc2_avf_audio::AVAudioEngineConfigurationChangeNotification;
use objc2_core_audio_types::AudioBuffer;
use objc2_foundation::{NSNotificationCenter, NSObject};

use super::*;

#[test]
fn the_default_input_device_is_readable_without_prompting() {
    // A Mac without an input device has none; one with a microphone names it.
    if let Some(device) = default_audio_input() {
        assert!(!device.name.is_empty());
    }
}

#[test]
fn each_authorization_status_maps_to_its_variant() {
    assert_eq!(
        AudioAuthorization::from_status(0),
        AudioAuthorization::NotDetermined
    );
    assert_eq!(
        AudioAuthorization::from_status(1),
        AudioAuthorization::Restricted
    );
    assert_eq!(
        AudioAuthorization::from_status(2),
        AudioAuthorization::Denied
    );
    assert_eq!(
        AudioAuthorization::from_status(3),
        AudioAuthorization::Authorized
    );
    assert_eq!(
        AudioAuthorization::from_status(7),
        AudioAuthorization::Unknown(7)
    );
}

#[test]
fn the_authorization_is_readable_without_prompting() {
    // Whatever this machine's answer is, reading it asks nothing.
    let _ = audio_authorization();
}

/// An AudioBufferList with room for two buffers, as the engine hands a planar stereo input.
#[repr(C)]
struct TwoBuffers {
    count: u32,
    buffers: [AudioBuffer; 2],
}

fn buffer(channels: u32, samples: &mut [f32]) -> AudioBuffer {
    AudioBuffer {
        mNumberChannels: channels,
        mDataByteSize: std::mem::size_of_val(samples) as u32,
        mData: samples.as_mut_ptr() as *mut c_void,
    }
}

/// `list` as an AudioBufferList.
fn as_list(list: &mut TwoBuffers) -> NonNull<AudioBufferList> {
    NonNull::from(list).cast()
}

/// A cycle's buffers copied out: each one's samples and channel count.
type Copied = Vec<(Vec<f32>, usize)>;

/// `list` read as the receiver block reads it, copied out.
fn read(list: &mut TwoBuffers) -> Copied {
    // SAFETY: TwoBuffers is laid out as an AudioBufferList of `count` (at most 2) buffers, each
    // pointing at live samples of the size it says.
    let buffers = unsafe { buffers_from_list(as_list(list)) };
    buffers
        .iter()
        .map(|&(samples, channels)| (samples.to_vec(), channels))
        .collect()
}

#[test]
fn a_planar_buffer_list_reads_one_slice_per_channel() {
    let (mut left, mut right) = ([1.0f32, 0.5], [0.0f32, -0.5]);
    let mut list = TwoBuffers {
        count: 2,
        buffers: [buffer(1, &mut left), buffer(1, &mut right)],
    };
    assert_eq!(
        read(&mut list),
        vec![(vec![1.0, 0.5], 1), (vec![0.0, -0.5], 1)]
    );
}

#[test]
fn an_interleaved_buffer_list_reads_no_more_buffers_than_it_holds() {
    let mut stereo = [1.0f32, 0.0, 0.5, 0.5];
    let mut unused = [9.0f32];
    let mut list = TwoBuffers {
        count: 1,
        buffers: [buffer(2, &mut stereo), buffer(1, &mut unused)],
    };
    assert_eq!(read(&mut list), vec![(vec![1.0, 0.0, 0.5, 0.5], 2)]);
}

#[test]
fn a_buffer_is_read_no_further_than_its_size_says() {
    let (mut left, mut right) = ([1.0f32, 1.0], [1.0f32, 9.0]);
    let mut list = TwoBuffers {
        count: 2,
        buffers: [buffer(1, &mut left), buffer(1, &mut right)],
    };
    // The second buffer's memory holds two floats, its byte size says one (and a stray byte).
    list.buffers[1].mDataByteSize = 5;
    assert_eq!(read(&mut list), vec![(vec![1.0, 1.0], 1), (vec![1.0], 1)]);
    // A buffer without data reads as empty.
    list.buffers[1].mData = std::ptr::null_mut();
    assert_eq!(read(&mut list), vec![(vec![1.0, 1.0], 1), (vec![], 1)]);
}

#[test]
fn a_receiver_call_hands_on_the_cycle() {
    let (mut left, mut right) = ([1.0f32, 0.5], [0.0f32, -0.5]);
    let mut list = TwoBuffers {
        count: 2,
        buffers: [buffer(1, &mut left), buffer(1, &mut right)],
    };
    let seen: Mutex<Vec<(Copied, usize, u32)>> = Mutex::default();
    let on_audio = |cycle: InputCycle<'_>| {
        let buffers = cycle
            .buffers
            .iter()
            .map(|&(samples, channels)| (samples.to_vec(), channels))
            .collect();
        seen.lock()
            .unwrap()
            .push((buffers, cycle.frames, cycle.sample_rate));
    };
    // SAFETY: as in `read`.
    unsafe { receive(as_list(&mut list), 2, 48_000, 1, &on_audio) };
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(vec![(vec![1.0, 0.5], 1), (vec![0.0, -0.5], 1)], 2, 48_000)]
    );
}

#[test]
fn a_panic_in_the_audio_closure_does_not_leave_the_receiver_call() {
    let mut samples = [0.25f32];
    let mut unused = [0.0f32];
    let mut list = TwoBuffers {
        count: 1,
        buffers: [buffer(1, &mut samples), buffer(1, &mut unused)],
    };
    let calls = AtomicUsize::new(0);
    let on_audio = |_: InputCycle<'_>| {
        calls.fetch_add(1, Ordering::SeqCst);
        panic!("a consumer bug");
    };
    // SAFETY: as in `read`.
    unsafe { receive(as_list(&mut list), 1, 16_000, 1, &on_audio) };
    // SAFETY: as in `read`.
    unsafe { receive(as_list(&mut list), 1, 16_000, 1, &on_audio) };
    assert_eq!(calls.load(Ordering::SeqCst), 2, "each cycle is offered");
}

/// An observer callback that counts its calls, and the count.
fn counter() -> (impl Fn() + Send + Sync + 'static, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let into = Arc::clone(&count);
    (
        move || {
            into.fetch_add(1, Ordering::SeqCst);
        },
        count,
    )
}

/// Posts the engine's configuration change for `object`, as AVAudioEngine does.
fn post_change(object: &NSObject) {
    // SAFETY: the notification name AVFAudio defines and a plain object as its poster.
    unsafe {
        NSNotificationCenter::defaultCenter().postNotificationName_object(
            AVAudioEngineConfigurationChangeNotification,
            Some(object.as_ref()),
        );
    }
}

#[test]
fn each_configuration_change_is_reported() {
    let engine = NSObject::new();
    let (on_change, count) = counter();
    let _observer = ChangeObserver::watch(engine.as_ref(), 1, on_change);
    post_change(&engine);
    post_change(&engine);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn another_engines_change_is_not_ours() {
    let (engine, other) = (NSObject::new(), NSObject::new());
    let (on_change, count) = counter();
    let _observer = ChangeObserver::watch(engine.as_ref(), 1, on_change);
    post_change(&other);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn nothing_is_reported_once_the_observer_is_dropped() {
    let engine = NSObject::new();
    let (on_change, count) = counter();
    drop(ChangeObserver::watch(engine.as_ref(), 1, on_change));
    post_change(&engine);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn a_panic_in_the_change_closure_does_not_leave_the_post() {
    let engine = NSObject::new();
    let count = Arc::new(AtomicUsize::new(0));
    let into = Arc::clone(&count);
    let _observer = ChangeObserver::watch(engine.as_ref(), 1, move || {
        into.fetch_add(1, Ordering::SeqCst);
        panic!("a consumer bug");
    });
    post_change(&engine);
    post_change(&engine);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

/// What the innermost callback scope on this thread names (`None`: none).
fn running() -> Option<(u64, CallbackKind)> {
    RUNNING_CALLBACK.get()
}

/// The teardown's two decisions for `engine` here: (forget the observer,
/// leak the engine).
fn decisions(engine: u64) -> (bool, bool) {
    (must_forget_observer(engine), must_leak(engine))
}

#[test]
fn outside_any_scope_nothing_is_forgotten_or_leaked() {
    assert_eq!(running(), None);
    assert_eq!(decisions(5), (false, false));
}

#[test]
fn inside_its_audio_callback_an_engine_removes_its_observer_but_leaks() {
    let _scope = CallbackScope::enter(5, CallbackKind::Audio);
    assert_eq!(decisions(5), (false, true));
    assert_eq!(decisions(6), (false, false), "another engine tears down");
}

#[test]
fn inside_its_change_callback_an_engine_forgets_its_observer_and_leaks() {
    let _scope = CallbackScope::enter(5, CallbackKind::Change);
    assert_eq!(decisions(5), (true, true));
    assert_eq!(decisions(6), (false, false), "another engine tears down");
}

#[test]
fn nested_scopes_restore_the_outer_engine_and_kind() {
    {
        let _scope = CallbackScope::enter(5, CallbackKind::Audio);
        {
            let _inner = CallbackScope::enter(5, CallbackKind::Change);
            assert_eq!(decisions(5), (true, true));
        }
        assert_eq!(running(), Some((5, CallbackKind::Audio)));
        assert_eq!(decisions(5), (false, true), "the outer kind is restored");
        {
            let _inner = CallbackScope::enter(6, CallbackKind::Change);
            assert_eq!(decisions(6), (true, true));
            assert_eq!(decisions(5), (false, false), "only the innermost counts");
        }
        assert_eq!(decisions(5), (false, true), "the outer scope is restored");
    }
    assert_eq!(running(), None);
    assert_eq!(decisions(5), (false, false));
}

#[test]
fn a_callback_scope_is_restored_when_its_callback_panics() {
    let _outer = CallbackScope::enter(3, CallbackKind::Change);
    let unwound = std::panic::catch_unwind(|| {
        let _scope = CallbackScope::enter(4, CallbackKind::Audio);
        panic!("a consumer bug");
    });
    assert!(unwound.is_err());
    assert_eq!(running(), Some((3, CallbackKind::Change)));
}

/// A one-buffer list, the receiver called on it for engine `engine` with
/// `on_audio`.
fn receive_one(engine: u64, on_audio: &impl Fn(InputCycle<'_>)) {
    let mut samples = [0.25f32];
    let mut unused = [0.0f32];
    let mut list = TwoBuffers {
        count: 1,
        buffers: [buffer(1, &mut samples), buffer(1, &mut unused)],
    };
    // SAFETY: as in `read`.
    unsafe { receive(as_list(&mut list), 1, 16_000, engine, on_audio) };
}

/// Where a closure records the scope it ran in.
type Seen = Arc<Mutex<Option<(u64, CallbackKind)>>>;

#[test]
fn the_audio_closure_runs_in_its_engines_audio_scope() {
    let seen: Seen = Seen::default();
    receive_one(11, &|_: InputCycle<'_>| *seen.lock().unwrap() = running());
    assert_eq!(*seen.lock().unwrap(), Some((11, CallbackKind::Audio)));
    assert_eq!(running(), None, "restored afterwards");
}

#[test]
fn the_audio_closures_scope_nests_and_survives_a_panic() {
    let _outer = CallbackScope::enter(20, CallbackKind::Change);
    let seen: Seen = Seen::default();
    receive_one(21, &|_: InputCycle<'_>| {
        *seen.lock().unwrap() = running();
        panic!("a consumer bug");
    });
    assert_eq!(*seen.lock().unwrap(), Some((21, CallbackKind::Audio)));
    assert_eq!(
        running(),
        Some((20, CallbackKind::Change)),
        "the outer scope is restored"
    );
}

/// An observer callback that records the scope it runs in, and the record.
fn scope_recorder(panics: bool) -> (impl Fn() + Send + Sync + 'static, Seen) {
    let seen = Seen::default();
    let into = Arc::clone(&seen);
    (
        move || {
            *into.lock().unwrap() = running();
            if panics {
                panic!("a consumer bug");
            }
        },
        seen,
    )
}

#[test]
fn the_change_closure_runs_in_its_engines_change_scope() {
    let engine = NSObject::new();
    let (on_change, seen) = scope_recorder(false);
    let _observer = ChangeObserver::watch(engine.as_ref(), 31, on_change);
    post_change(&engine);
    assert_eq!(*seen.lock().unwrap(), Some((31, CallbackKind::Change)));
    assert_eq!(running(), None, "restored afterwards");
}

#[test]
fn the_change_closures_scope_nests_and_survives_a_panic() {
    let engine = NSObject::new();
    let (on_change, seen) = scope_recorder(true);
    let _observer = ChangeObserver::watch(engine.as_ref(), 41, on_change);
    let _outer = CallbackScope::enter(40, CallbackKind::Audio);
    post_change(&engine);
    assert_eq!(*seen.lock().unwrap(), Some((41, CallbackKind::Change)));
    assert_eq!(
        running(),
        Some((40, CallbackKind::Audio)),
        "the outer scope is restored"
    );
}
