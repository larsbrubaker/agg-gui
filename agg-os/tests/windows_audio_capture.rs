//! `agg_os::windows::audio_capture`: what holds on any Windows machine, the
//! CI runner with no capture device included. COM initializes and
//! uninitializes on a thread, twice in a row and nested; opening the default
//! capture endpoint either fails with an error or, on a machine that has a
//! microphone, opens a stream whose few packets read and which then drops;
//! an apartment whose COM someone else initialized opens no stream.
//! The capture itself, with audio, is agg-mic's ignored `tests/live_mic.rs`.

#![cfg(all(windows, feature = "microphone"))]

use std::cell::Cell;
use std::time::{Duration, Instant};

use agg_os::windows::audio_capture::{CaptureFormat, CaptureStream, ComApartment, OpenError};
use windows_062::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows_062::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

/// Runs `f` on a thread of its own, whose COM no other test touches.
fn on_fresh_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f)
        .join()
        .expect("the test thread panicked");
}

#[test]
fn com_initializes_and_drops_twice_in_a_row() {
    on_fresh_thread(|| {
        for _ in 0..2 {
            let com = ComApartment::multithreaded();
            assert!(com.is_initialized());
            drop(com);
        }
    });
}

#[test]
fn com_initializes_nested_on_one_thread() {
    on_fresh_thread(|| {
        let outer = ComApartment::multithreaded();
        let inner = ComApartment::multithreaded();
        // The second is S_FALSE, which counts as a success to pair.
        assert!(outer.is_initialized());
        assert!(inner.is_initialized());
        drop(inner);
        drop(outer);
    });
}

#[test]
fn a_stream_does_not_open_on_com_the_apartment_does_not_hold() {
    on_fresh_thread(|| {
        // SAFETY: no reserved pointer; this thread's single-threaded
        // apartment is uninitialized below, on this thread, once.
        let sta = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        assert!(sta.is_ok(), "{sta:?}");
        let com = ComApartment::multithreaded();
        // RPC_E_CHANGED_MODE: the thread is in the STA already.
        assert!(!com.is_initialized());
        let fell_back = Cell::new(false);
        let accepted_mix = Cell::new(false);
        let opened = CaptureStream::open_default(
            &com,
            16_000,
            2_000_000,
            |_| {
                fell_back.set(true);
                true
            },
            |_| {
                accepted_mix.set(true);
                true
            },
        );
        match &opened {
            Err(OpenError::Call(error)) => {
                assert_eq!(error.code(), RPC_E_CHANGED_MODE.0, "{error}")
            }
            Err(other) => panic!("not the CoInitializeEx error: {other}"),
            Ok(_) => panic!("a stream opened on COM the apartment does not hold"),
        }
        assert!(!fell_back.get());
        assert!(!accepted_mix.get());
        drop(opened);
        drop(com);
        // SAFETY: pairs the successful CoInitializeEx above, on this thread;
        // no COM object made on it is alive.
        unsafe { CoUninitialize() };
    });
}

#[test]
fn opening_the_default_microphone_fails_or_reads_a_few_packets() {
    on_fresh_thread(|| {
        let com = ComApartment::multithreaded();
        let started = Instant::now();
        let opened = CaptureStream::open_default(&com, 16_000, 2_000_000, |_| true, |_| true);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "opening took {:?}",
            started.elapsed()
        );
        match opened {
            Err(error) => eprintln!("no capture stream: {error}"),
            Ok(stream) => {
                let block_align = match stream.format() {
                    CaptureFormat::Converted => 2,
                    CaptureFormat::Mix(mix) => usize::from(mix.block_align),
                };
                let mut bytes = Vec::new();
                for _ in 0..5 {
                    std::thread::sleep(Duration::from_millis(10));
                    let before = bytes.len();
                    match stream.next_packet(&mut bytes) {
                        Ok(Some(packet)) => {
                            let added = bytes.len() - before;
                            let expected = if packet.silent {
                                0
                            } else {
                                packet.frames as usize * block_align
                            };
                            assert_eq!(added, expected, "{packet:?}");
                        }
                        Ok(None) => assert_eq!(bytes.len(), before),
                        Err(error) => {
                            eprintln!("capture error: {error}");
                            break;
                        }
                    }
                }
                drop(stream);
            }
        }
        // `com` drops last, after the stream it outlives.
    });
}
