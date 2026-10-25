//! The real microphone on each platform. Saying it is available (and, on macOS, reading the
//! permission status, which never prompts) always runs; an actual recording needs a microphone,
//! and on macOS a person to answer the permission prompt, so it is ignored unless asked for:
//! `AGG_LIVE_MIC_TESTS=1 cargo test -p agg-mic --test live_mic -- --ignored --nocapture`
//! (unbundled on macOS, with Terminal or the IDE allowed in Privacy & Security > Microphone; on
//! Windows with Settings > Privacy & security > Microphone letting desktop apps use it; on Linux
//! with a PulseAudio or PipeWire server running). From mattercad-ai's
//! `mac_audio_recorder_tests.rs`, `rust_only_windows_audio_recorder_tests.rs` and
//! `rust_only_linux_audio_recorder_tests.rs`, whose live recordings checked the WAV MatterCAD
//! builds from these chunks; here the chunks themselves are checked.
//!
//! `a_live_recording_delivers_a_second_of_mono_audio` also measures the two things a dictation
//! notices at its end, and prints them: how long before the stop the last chunk arrived (the
//! audio a stop can lose: about one I/O buffer, ~10 ms, on macOS; one poll, 10 ms, on Windows;
//! one 20 ms fragment on Linux), and how long `Microphone::stop` took (agg-gui-whistle runs it on
//! the UI thread, so it must stay well under a frame). Speak through the stop, as a dictation
//! ends: a short tail means the last word reached the handler.
//!
//! With `AGG_MIC_TRACE=1` (the recording test sets it) a stop prints its own breakdown: the
//! backend's stop and the delivery's close, and on macOS the observer's removal, the engine's
//! hand-off and, from the teardown thread, `AVAudioEngine.stop()` and the sink node's detach.
//!
//! The live tests take turns on the microphone (`ONE_AT_A_TIME`), so cargo's parallel test
//! threads never have two recordings open at once.
//!
//! A lost device needs a person too: with `AGG_MIC_UNPLUG_TEST=1` as well,
//! `a_live_microphone_that_goes_away_ends_with_an_error` records and waits up to 30 s for the
//! microphone to be unplugged (on macOS switching the input in System Settings > Sound also does;
//! on Linux the sound server may move the stream to another source instead of ending it, which
//! the test reports as no failure). It says which device it records from first; a Mac's built-in
//! microphone can't be unplugged, so use an external one (or switch the input while it waits).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agg_mic::{MicError, Microphone};

/// Held by each test that records, so two never have the microphone open at once.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The device a recording will use, as the person running the test should know it.
fn describe_input() -> String {
    #[cfg(target_os = "macos")]
    {
        match agg_mic::macos::default_input_device() {
            Some(device) if device.built_in => format!(
                "\"{}\", built into this Mac: it can't be unplugged. Plug in an external \
                 microphone and make it the input in System Settings > Sound, then rerun; or, \
                 while this waits, switch the input there, which ends the recording the same way.",
                device.name
            ),
            Some(device) => format!("\"{}\": unplug it now.", device.name),
            None => "no input device found.".to_owned(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        "the system's default input device: unplug it now.".to_owned()
    }
}

#[test]
fn the_platform_microphone_says_whether_it_is_available() {
    let microphone = Microphone::default_input();
    let supported = cfg!(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    ));
    assert_eq!(microphone.is_available(), supported);
    assert!(!microphone.is_recording());
    assert_eq!(
        Microphone::with_app_name("agg-mic tests").is_available(),
        supported
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_permission_status_is_readable_without_prompting() {
    use agg_mic::macos::{
        authorization_status, AUTHORIZATION_AUTHORIZED, AUTHORIZATION_NOT_DETERMINED,
    };

    let status = authorization_status();
    assert!(status >= AUTHORIZATION_NOT_DETERMINED);
    assert!(status <= AUTHORIZATION_AUTHORIZED);
}

/// What the live recording's chunk handler saw.
#[derive(Default)]
struct Seen {
    samples: usize,
    rate: u32,
    chunks: usize,
    /// When the latest chunk arrived.
    last: Option<Instant>,
}

#[test]
#[ignore = "records from the real microphone; set AGG_LIVE_MIC_TESTS=1 and run ignored tests"]
fn a_live_recording_delivers_a_second_of_mono_audio() {
    if std::env::var("AGG_LIVE_MIC_TESTS").as_deref() != Ok("1") {
        eprintln!("Set AGG_LIVE_MIC_TESTS=1 to record from the real microphone.");
        return;
    }

    let _turn = one_at_a_time();
    // Print the stop's breakdown (edition 2021: setting the environment is safe; this test holds
    // the microphone lock, and nothing else here reads the variable meanwhile).
    std::env::set_var("AGG_MIC_TRACE", "1");
    let microphone = Microphone::with_app_name("agg-mic tests");
    let shared = Arc::new(Mutex::new(Seen::default()));
    let into = Arc::clone(&shared);
    let failed = Arc::new(Mutex::new(None));
    let failed_into = Arc::clone(&failed);
    microphone
        .start(
            Box::new(move |samples: &[f32], rate: u32| {
                assert!(samples.iter().all(|s| s.is_finite()));
                let mut seen = into.lock().unwrap();
                seen.samples += samples.len();
                seen.rate = rate;
                seen.chunks += 1;
                seen.last = Some(Instant::now());
            }),
            Box::new(move |error| *failed_into.lock().unwrap() = Some(error)),
        )
        .unwrap();
    assert!(microphone.is_recording());
    eprintln!("Recording for a second: speak, and keep speaking through the stop.");

    // A real second of audio; there is no event to wait for when the thing measured is time
    // itself.
    std::thread::sleep(Duration::from_millis(1000));
    let stopping = Instant::now();
    microphone.stop();
    let stop_took = stopping.elapsed();

    assert_eq!(*failed.lock().unwrap(), None);
    let seen = shared.lock().unwrap();
    let (samples, rate, chunks) = (seen.samples, seen.rate, seen.chunks);
    // How long before the stop the last chunk came (zero if one came during the stop).
    let tail = stopping.saturating_duration_since(seen.last.expect("a chunk arrived"));
    eprintln!(
        "{chunks} chunks, {samples} samples at {rate} Hz; the last arrived {:.1} ms before the \
         stop; the stop took {:.2} ms",
        tail.as_secs_f64() * 1000.0,
        stop_took.as_secs_f64() * 1000.0
    );
    if cfg!(any(target_os = "windows", target_os = "linux")) && rate != 16_000 {
        // The audio engine (WASAPI) or the sound server converts to 16 kHz, unless WASAPI
        // refused and the device's own format is recorded.
        eprintln!("Recorded at the device's rate, {rate} Hz.");
    }
    assert!(rate >= 8000, "rate {rate}");
    assert!(chunks > 1, "{chunks} chunks: delivered as it records");
    assert!(
        samples as f64 > f64::from(rate) * 0.5,
        "{samples} samples at {rate} Hz"
    );
    // The end is kept: at most a fragment's worth (Linux's 20 ms) and scheduling lies between
    // the last chunk and the stop.
    assert!(tail < Duration::from_millis(40), "tail {tail:?}");
    // agg-gui-whistle stops on the UI thread: well under a 60 Hz frame.
    assert!(
        stop_took < Duration::from_millis(16),
        "stop took {stop_took:?}"
    );
    drop(seen);

    // Nothing arrives once the stop has returned (and the teardown thread prints its own times
    // meanwhile).
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(shared.lock().unwrap().chunks, chunks);
}

#[test]
#[ignore = "needs a person to unplug the microphone; set AGG_LIVE_MIC_TESTS=1 and AGG_MIC_UNPLUG_TEST=1"]
fn a_live_microphone_that_goes_away_ends_with_an_error() {
    if std::env::var("AGG_LIVE_MIC_TESTS").as_deref() != Ok("1")
        || std::env::var("AGG_MIC_UNPLUG_TEST").as_deref() != Ok("1")
    {
        eprintln!(
            "Set AGG_LIVE_MIC_TESTS=1 and AGG_MIC_UNPLUG_TEST=1, then unplug the microphone."
        );
        return;
    }

    let _turn = one_at_a_time();
    let microphone = Microphone::with_app_name("agg-mic tests");
    let (failed_tx, failed) = mpsc::channel();
    microphone
        .start(
            Box::new(|_: &[f32], _: u32| {}),
            Box::new(move |error| {
                let _ = failed_tx.send(error);
            }),
        )
        .unwrap();
    eprintln!(
        "Recording from {} Waiting up to 30 s for it to go away.",
        describe_input()
    );
    let error = failed
        .recv_timeout(Duration::from_secs(30))
        .expect("no failure reported within 30 s");
    eprintln!("Reported: {error}");
    assert!(matches!(error, MicError::NoDevice(_)), "{error:?}");
    assert!(!microphone.is_recording());
    // Cleaning up after the failure reports nothing more.
    microphone.stop();
    assert!(failed.try_recv().is_err());
}
