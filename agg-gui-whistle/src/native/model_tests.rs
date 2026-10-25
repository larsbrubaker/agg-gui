//! Tests for `native/model.rs`: the configured path, the cache, downloads
//! (with `native-download`), and how failures are told apart. Tests that
//! need the real model skip without `../whistle-rust/models/whistle.cact`.

use std::path::PathBuf;

use super::*;

fn real_model() -> Option<PathBuf> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../whistle-rust/models/whistle.cact");
    path.exists().then_some(path)
}

/// A fresh, empty directory for one test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agg-gui-whistle-test-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn source(path: Option<PathBuf>, cache_dir: Option<PathBuf>, urls: &[&str]) -> Source {
    Source {
        path,
        cache_dir,
        urls: urls.iter().map(|u| u.to_string()).collect(),
    }
}

fn no_progress(_: u64, _: u64) {}

#[test]
fn nothing_to_load_from_is_a_download_failure() {
    let dir = temp_dir("nothing");
    let err = source(None, Some(dir), &[]).load(&no_progress).unwrap_err();
    assert_eq!(err.0, LoadFailure::Download);
}

#[test]
fn a_missing_path_is_a_download_failure_naming_it() {
    let dir = temp_dir("missing");
    let missing = dir.join("nope.cact");
    let err = source(Some(missing), Some(dir), &[])
        .load(&no_progress)
        .unwrap_err();
    assert_eq!(err.0, LoadFailure::Download);
    assert!(err.1.contains("nope.cact"), "{}", err.1);
}

#[test]
fn a_wrong_file_at_the_path_is_a_checksum_failure() {
    let dir = temp_dir("wrong");
    let wrong = dir.join("whistle.cact");
    std::fs::write(&wrong, b"definitely not a model").expect("write");
    let err = source(Some(wrong), Some(dir), &[])
        .load(&no_progress)
        .unwrap_err();
    assert_eq!(err.0, LoadFailure::Checksum);
}

#[test]
fn the_configured_path_loads_and_a_cached_copy_is_used_next() {
    let Some(model) = real_model() else {
        eprintln!("skipped: no ../whistle-rust/models/whistle.cact");
        return;
    };
    let dir = temp_dir("cache");
    let (bytes, from) = source(Some(model.clone()), Some(dir.clone()), &[])
        .load(&no_progress)
        .expect("loads");
    assert_eq!(from, model.display().to_string());
    // Seed the cache as a download would, then load with no path.
    let src = source(None, Some(dir.clone()), &[]);
    save(&src.cache_file().expect("cache file"), &bytes);
    let (cached, from) = src.load(&no_progress).expect("from cache");
    assert_eq!(from, "cache");
    assert_eq!(cached.len(), bytes.len());
}

#[test]
fn a_damaged_cached_copy_is_deleted() {
    let dir = temp_dir("damaged");
    let src = source(None, Some(dir), &[]);
    let file = src.cache_file().expect("cache file");
    save(&file, b"half a model");
    assert!(file.exists());
    assert!(src.load(&no_progress).is_err());
    assert!(!file.exists(), "the bad copy is gone");
}

#[test]
fn only_http_urls_are_kept_natively() {
    let config = Config::default();
    let src = Source::from_config(&config);
    assert!(src.urls.iter().all(|u| u.starts_with("https://")));
    assert_eq!(src.urls.len(), 2, "the page-relative URL is dropped");
}

#[cfg(feature = "native-download")]
mod download_tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    use super::*;

    /// Serve one canned response on a loopback port; its URL.
    fn serve_once(status_line: &'static str, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{addr}/whistle.cact")
    }

    /// A URL on a loopback port nothing listens on: the connection is
    /// refused, a network failure.
    fn refused_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        format!("http://{addr}/whistle.cact")
    }

    /// Serve the headers of a whole model and a little of it, then stall
    /// (the connection stays open) for `stall`.
    fn serve_stalling(stall: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    model_check::expected_bytes()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&[0u8; 4096]);
                let _ = stream.flush();
                std::thread::sleep(stall);
            }
        });
        format!("http://{addr}/whistle.cact")
    }

    /// A download that stops arriving times out (a phone's link dropped
    /// mid-transfer must not leave the key on Preparing for good).
    #[test]
    fn a_stalled_download_times_out() {
        let url = serve_stalling(Duration::from_secs(30));
        let t0 = Instant::now();
        let agent = download::agent_with(Duration::from_millis(300));
        let err = download::get_with(&agent, &url, &no_progress).unwrap_err();
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "took {:?}",
            t0.elapsed()
        );
        assert!(err.to_lowercase().contains("timeout"), "{err}");
    }

    /// A slow link that keeps delivering must be able to finish however
    /// long the 17 MB take: only silence times out, never the total.
    #[test]
    fn only_silence_times_a_download_out() {
        let stall = Duration::from_millis(300);
        let agent = download::agent_with(stall);
        assert_eq!(
            agent.config().timeouts().recv_body,
            None,
            "no cap on the body"
        );
        assert_eq!(agent.config().timeout_per_read(), Some(stall));
    }

    /// A body that trickles in, each piece well inside the stall timeout,
    /// arrives whole even when the transfer takes several times as long.
    #[test]
    fn a_trickling_download_completes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n");
                for _ in 0..10 {
                    let _ = stream.write_all(&[7u8; 100]);
                    let _ = stream.flush();
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        let url = format!("http://{addr}/whistle.cact");
        let agent = download::agent_with(Duration::from_millis(300));
        let bytes = download::get_with(&agent, &url, &no_progress).expect("arrives");
        assert_eq!(bytes.len(), 1000);
    }

    /// More bytes than whistle.cact has is some other file, refused while
    /// it downloads: a download failure saying so, as on the web.
    #[test]
    fn an_oversized_download_is_a_download_failure_saying_so() {
        let url = serve_once(
            "HTTP/1.1 200 OK",
            vec![0u8; model_check::expected_bytes() + 10],
        );
        let err = source(None, Some(temp_dir("dlhuge")), &[&url])
            .load(&no_progress)
            .unwrap_err();
        assert_eq!(err.0, LoadFailure::Download, "{}", err.1);
        assert!(err.1.contains("larger than whistle.cact"), "{}", err.1);
    }

    #[test]
    fn a_network_failure_after_a_wrong_file_is_a_download_failure() {
        let wrong = serve_once("HTTP/1.1 200 OK", b"not the model".to_vec());
        let refused = refused_url();
        let err = source(
            None,
            Some(temp_dir("dlwrongthenrefused")),
            &[&wrong, &refused],
        )
        .load(&no_progress)
        .unwrap_err();
        assert_eq!(err.0, LoadFailure::Download, "{}", err.1);
    }

    #[test]
    fn a_failed_download_is_a_download_failure() {
        let url = serve_once("HTTP/1.1 404 Not Found", b"missing".to_vec());
        let err = source(None, Some(temp_dir("dl404")), &[&url])
            .load(&no_progress)
            .unwrap_err();
        assert_eq!(err.0, LoadFailure::Download);
    }

    #[test]
    fn a_wrong_download_is_a_checksum_failure() {
        let url = serve_once("HTTP/1.1 200 OK", b"not the model".to_vec());
        let err = source(None, Some(temp_dir("dlwrong")), &[&url])
            .load(&no_progress)
            .unwrap_err();
        assert_eq!(err.0, LoadFailure::Checksum);
    }

    #[test]
    fn a_download_is_checked_cached_and_reported() {
        let Some(model) = real_model() else {
            eprintln!("skipped: no ../whistle-rust/models/whistle.cact");
            return;
        };
        let bytes = std::fs::read(model).expect("model");
        let url = serve_once("HTTP/1.1 200 OK", bytes.clone());
        let dir = temp_dir("dlok");
        let src = source(None, Some(dir), &[&url]);
        let seen = std::cell::Cell::new(0u64);
        let (got, from) = src
            .load(&|loaded, total| {
                assert!(loaded <= total);
                assert_eq!(total, model_check::expected_bytes() as u64);
                seen.set(loaded);
            })
            .expect("downloads");
        assert_eq!(from, url);
        assert_eq!(got.len(), bytes.len());
        assert_eq!(seen.get(), bytes.len() as u64, "progress reaches the end");
        assert!(src.cache_file().expect("file").exists(), "cached");
    }
}
