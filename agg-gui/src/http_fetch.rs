//! Fire-and-forget HTTP(S) GET used by `MarkdownView`'s remote images
//! (`widgets/markdown/image_loader.rs`).
//!
//! [`fetch_bytes`] calls its completion callback with the response body (or an
//! error message) once the request finishes; it never blocks the caller.
//!
//! - **Native:** ureq's blocking client on a short-lived worker thread, over
//!   rustls with the RustCrypto provider and the Mozilla root store compiled
//!   in (`webpki-roots`).  The whole TLS stack is pure Rust: no ring, no
//!   aws-lc, no `cc` build step.  The callback runs on the worker thread, so
//!   it must be `Send` and signal the UI through thread-safe state (the
//!   markdown loader signals the starting UI thread's queue, captured with
//!   `ui_thread::current_queue` before the fetch: the worker is unbound, so
//!   its own `animation::signal_async_state_change` would wake the main
//!   queue instead).
//! - **wasm32:** the browser's fetch API through `ehttp`; the callback runs on
//!   the main thread when the promise resolves.

/// Result of a GET: the body of a 2xx response, or a human-readable reason.
pub(crate) type FetchResult = Result<Vec<u8>, String>;

/// Largest body accepted, in bytes.  ureq's default cap is 10 MB; remote
/// markdown images (screenshots, animated GIFs) can exceed that.
#[cfg(not(target_arch = "wasm32"))]
const MAX_BODY_BYTES: u64 = 256 * 1024 * 1024;

/// GET `url` and hand the outcome to `on_done`.  Non-2xx statuses are
/// reported as errors.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fetch_bytes(url: String, on_done: impl FnOnce(FetchResult) + Send + 'static) {
    let agent = native_agent().clone();
    let spawned = std::thread::Builder::new()
        .name("agg-gui-http".into())
        .spawn(move || on_done(get(&agent, &url)));
    if let Err(err) = spawned {
        // Thread creation failed (resource exhaustion).  `on_done` was moved
        // into the failed spawn and dropped, so it cannot be told; log it.
        eprintln!("agg-gui: could not start HTTP worker thread: {err}");
    }
}

/// GET `url` and hand the outcome to `on_done`.  Non-2xx statuses are
/// reported as errors.
#[cfg(target_arch = "wasm32")]
pub(crate) fn fetch_bytes(url: String, on_done: impl FnOnce(FetchResult) + Send + 'static) {
    ehttp::fetch(ehttp::Request::get(url), move |result| {
        on_done(match result {
            Ok(response) if response.ok => Ok(response.bytes),
            Ok(response) => Err(format!("HTTP {} {}", response.status, response.status_text)),
            Err(err) => Err(err),
        })
    });
}

/// One agent for the process, so connections and TLS sessions are reused.
#[cfg(not(target_arch = "wasm32"))]
fn native_agent() -> &'static ureq::Agent {
    use std::sync::{Arc, OnceLock};
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .unversioned_rustls_crypto_provider(Arc::new(rustls_rustcrypto::provider()))
            .root_certs(ureq::tls::RootCerts::WebPki)
            .build();
        ureq::Agent::config_builder().tls_config(tls).build().into()
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn get(agent: &ureq::Agent, url: &str) -> FetchResult {
    // With ureq's default `http_status_as_error`, any non-2xx status
    // arrives as `Err(StatusCode)`.
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::fetch_bytes;

    /// Serve one canned HTTP/1.1 response on a loopback port; returns its URL.
    fn serve_once(status_line: &'static str, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        format!("http://{addr}/image.png")
    }

    fn fetch_blocking(url: String) -> Result<Vec<u8>, String> {
        let (tx, rx) = mpsc::channel();
        fetch_bytes(url, move |result| {
            let _ = tx.send(result);
        });
        rx.recv_timeout(Duration::from_secs(10))
            .expect("fetch callback ran")
    }

    #[test]
    fn fetch_delivers_the_body_of_a_2xx_response() {
        let url = serve_once("HTTP/1.1 200 OK", b"pixels");
        assert_eq!(fetch_blocking(url), Ok(b"pixels".to_vec()));
    }

    #[test]
    fn fetch_reports_non_2xx_as_an_error() {
        let url = serve_once("HTTP/1.1 404 Not Found", b"missing");
        assert!(fetch_blocking(url).is_err());
    }

    #[test]
    fn tls_agent_builds_with_the_rustcrypto_provider() {
        // Building the agent exercises the TLS configuration (provider +
        // root store) without needing the network.
        let _ = super::native_agent();
    }
}
