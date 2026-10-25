//! Tests for `native/mic.rs` without a device: what the [`Sink`] does when
//! the engine is gone, and what a cpal stream error reports.

use std::sync::mpsc::{self, Receiver};

use agg_gui::ui_thread::UiQueue;

use super::*;

/// A sink for session 7 at 16 kHz with 100-sample pushes, the engine's
/// inbox and the UI thread's events.
fn sink() -> (Arc<Mutex<Sink>>, Receiver<ToWorker>, Receiver<Event>) {
    let (engine, inbox) = mpsc::channel();
    let (events_tx, events) = mpsc::channel();
    let notifier = Notifier {
        events: events_tx,
        queue: UiQueue::default(),
    };
    let sink = Sink {
        session: 7,
        rate: 16_000,
        buffer: Vec::new(),
        chunk: 100,
        engine,
        notifier,
        open: true,
        level: 0.0,
        since_level: 0,
    };
    (Arc::new(Mutex::new(sink)), inbox, events)
}

#[test]
fn a_full_chunk_goes_to_the_engine() {
    let (sink, inbox, _events) = sink();
    sink.lock().unwrap().feed(&[0.1; 150]);
    assert!(matches!(
        inbox.try_recv(),
        Ok(ToWorker::Push { session: 7, rate: 16_000, ref samples }) if samples.len() == 150
    ));
}

#[test]
fn a_push_the_engine_cannot_take_reports_it_lost_once() {
    let (sink, inbox, events) = sink();
    drop(inbox);
    sink.lock().unwrap().feed(&[0.1; 150]);
    sink.lock().unwrap().feed(&[0.1; 150]);
    let lost: Vec<_> = events
        .try_iter()
        .filter(|e| matches!(e, Event::EngineLost(_)))
        .collect();
    assert_eq!(lost.len(), 1);
    assert!(!sink.lock().unwrap().open, "the sink takes no more audio");
}

#[test]
fn a_device_that_goes_away_fails_the_microphone() {
    let (sink, _inbox, events) = sink();
    on_stream_error(&sink, cpal::StreamError::DeviceNotAvailable);
    assert!(matches!(
        events.try_recv(),
        Ok(Event::MicFailed(7, MicError::NoDevice))
    ));
    // Once: the stream is done.
    on_stream_error(&sink, cpal::StreamError::StreamInvalidated);
    assert!(events.try_recv().is_err());
}

#[test]
fn a_broken_stream_fails_the_microphone() {
    let (sink, _inbox, events) = sink();
    on_stream_error(&sink, cpal::StreamError::StreamInvalidated);
    assert!(matches!(
        events.try_recv(),
        Ok(Event::MicFailed(7, MicError::Other(_)))
    ));
}

/// An overrun is a glitch, not the end of the stream.
#[test]
fn an_overrun_only_logs() {
    let (sink, _inbox, events) = sink();
    on_stream_error(&sink, cpal::StreamError::BufferUnderrun);
    assert!(events.try_recv().is_err());
    assert!(sink.lock().unwrap().open);
}

#[test]
fn a_closed_sink_reports_nothing() {
    let (sink, _inbox, events) = sink();
    sink.lock().unwrap().open = false;
    on_stream_error(&sink, cpal::StreamError::DeviceNotAvailable);
    assert!(events.try_recv().is_err());
}
