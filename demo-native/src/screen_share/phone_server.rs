//! Tiny LAN HTTP server for the Screen Share demo.
//!
//! Serves the agg-gui demo **web build** over the LAN so a phone on the same
//! Wi-Fi can scan the QR and load the *same* app the desktop runs. Opened with
//! `?host=<id>`, that page flips into sender mode and streams its canvas back.
//! Adapted from Marbles' `phone_server`, but serves the whole `demo/` site
//! (index.html + bundled JS + wasm-pack output + fonts) rather than a bespoke
//! page.
//!
//! Requires the web build to exist on disk (`bun run build:wasm` + the TS
//! bundle, or `bun run dev` once). Missing files simply 404 — the desktop app
//! itself still runs regardless.
//!
//! Static delivery only. A native desktop binary can open a TCP listener; a
//! wasm desktop build cannot, which is why this is native-only (the web build's
//! phone loads from the page's own origin instead).

use std::path::{Path, PathBuf};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// LAN-reachable base info for the QR.
pub struct PhoneServer {
    /// `http://<lan-ip>:<port>/`
    pub url: String,
}

/// Bind `0.0.0.0:0`, spawn an accept loop, and return the LAN URL. Resolves as
/// soon as the listener is bound; requests are handled by background tasks.
pub async fn start() -> Result<PhoneServer, String> {
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    let host = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let url = format!("http://{host}:{port}/");

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((sock, _peer)) => {
                    tokio::spawn(async move {
                        if let Err(err) = handle(sock).await {
                            eprintln!("screen-share phone-server: {err}");
                        }
                    });
                }
                Err(err) => {
                    eprintln!("screen-share phone-server accept: {err}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    });

    Ok(PhoneServer { url })
}

/// Idle wait for the next request on a kept-alive connection before giving up.
const KEEP_ALIVE_IDLE: std::time::Duration = std::time::Duration::from_secs(30);

async fn handle(mut sock: TcpStream) -> std::io::Result<()> {
    // Disable Nagle's algorithm.  The demo pulls many small assets (wasm,
    // bundle, dozens of fonts); with Nagle on, each small response collides with
    // the client's delayed ACK and stalls tens-to-hundreds of ms — invisible on
    // loopback but brutal over Wi-Fi, where it compounds into multi-minute loads.
    let _ = sock.set_nodelay(true);

    let (read_half, write_half) = sock.split();
    let mut reader = BufReader::new(read_half);
    let mut writer = write_half;

    // HTTP/1.1 keep-alive: serve many requests per connection so the phone isn't
    // paying a fresh TCP handshake per font.  Fully reading each request (line +
    // headers through the blank line) also drains the socket, so the close is a
    // clean FIN rather than an RST — an RST makes the browser treat the response
    // as failed and retry with backoff, which is its own multi-second stall.
    loop {
        let mut request_line = String::new();
        match tokio::time::timeout(KEEP_ALIVE_IDLE, reader.read_line(&mut request_line)).await {
            Ok(Ok(0)) => break,         // client closed
            Ok(Ok(_)) => {}             // got a request line
            Ok(Err(e)) => return Err(e),
            Err(_) => break,            // idle keep-alive timeout
        }

        let mut keep_alive = true; // HTTP/1.1 default
        let mut accept_gzip = false;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).await? == 0 {
                return Ok(()); // connection dropped mid-request
            }
            let line = header.trim_end();
            if line.is_empty() {
                break; // blank line: end of headers (GET requests carry no body)
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("connection")
                    && value.trim().eq_ignore_ascii_case("close")
                {
                    keep_alive = false;
                } else if name.eq_ignore_ascii_case("accept-encoding")
                    && value.to_ascii_lowercase().contains("gzip")
                {
                    accept_gzip = true;
                }
            }
        }

        // Path may carry a query string (?host=...) — strip it for routing.
        let raw_path = request_line.split_whitespace().nth(1).unwrap_or("/");
        let path = raw_path.split('?').next().unwrap_or("/");

        // Diagnostics beacon: the phone reports load milestones to `/__diag/<msg>`
        // (the browser console isn't visible to us), so a stalled phase shows up
        // here as a "begin" with no matching "done".  Pure logging — never the
        // filesystem, so the percent-decoded message can't be a path traversal.
        if let Some(rest) = path.strip_prefix("/__diag/") {
            eprintln!("screen-share phone diag: {}", percent_decode(rest));
            write_status(&mut writer, "204 No Content", keep_alive).await?;
            if !keep_alive {
                break;
            }
            continue;
        }

        match resolve_asset(path) {
            Some((body, content_type)) => {
                write_response(&mut writer, &body, content_type, keep_alive, accept_gzip).await?
            }
            None => write_status(&mut writer, "404 Not Found", keep_alive).await?,
        }

        if !keep_alive {
            break;
        }
    }

    writer.shutdown().await?;
    Ok(())
}

/// Map a request path to a file under the demo web build directory. `/` serves
/// `index.html`. Rejects path traversal.
fn resolve_asset(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let rel = if path == "/" { "index.html" } else { path.trim_start_matches('/') };
    if rel.is_empty() || rel.contains("..") || rel.contains('\\') {
        return None;
    }
    let full = demo_web_dir().join(rel);
    let body = std::fs::read(&full).ok()?;
    Some((body, content_type_for(rel)))
}

/// The agg-gui demo web root (`agg-gui/demo`). index.html references assets
/// under `./public/...`, which resolve to real files here.
fn demo_web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("demo-native lives under agg-gui/")
        .join("demo")
}

fn content_type_for(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ttf" => "font/ttf",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

async fn write_response(
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
    body: &[u8],
    content_type: &str,
    keep_alive: bool,
    accept_gzip: bool,
) -> std::io::Result<()> {
    let connection = if keep_alive { "keep-alive" } else { "close" };

    // gzip the big compressible assets (the 8.7 MB wasm shrinks to ~3.5 MB),
    // which is the dominant cost of a phone load over Wi-Fi.  `fast` keeps the
    // per-request CPU low; PNG/woff2 are already compressed so we skip them.
    let gz = if accept_gzip && is_compressible(content_type) {
        gzip(body).ok()
    } else {
        None
    };
    let (payload, encoding): (&[u8], &str) = match gz.as_deref() {
        Some(c) => (c, "\r\nContent-Encoding: gzip"),
        None => (body, ""),
    };

    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nCache-Control: no-cache{encoding}\r\nConnection: {connection}\r\n\r\n",
        payload.len()
    );
    // With TCP_NODELAY on, the header and body go out without a Nagle stall;
    // Content-Length frames the body so the client knows when this response ends
    // and can reuse the connection for the next request.
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(payload).await
}

fn is_compressible(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.contains("javascript")
        || content_type.contains("json")
        || content_type.contains("wasm")
        || content_type.contains("svg")
        || content_type.contains("font/ttf")
}

fn gzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = GzEncoder::new(Vec::with_capacity(data.len() / 2), Compression::fast());
    encoder.write_all(data)?;
    encoder.finish()
}

/// Minimal `%XX` URL decoder for the diagnostics beacon (avoids a dep). Invalid
/// escapes are passed through verbatim; non-UTF-8 bytes become replacement chars.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Bodyless status response (e.g. 404), honoring keep-alive.
async fn write_status(
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
    status: &str,
    keep_alive: bool,
) -> std::io::Result<()> {
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let response =
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: {connection}\r\n\r\n");
    writer.write_all(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream as ClientStream;

    /// One bare HTTP/1.1 GET, returning (body+header bytes, elapsed).
    async fn fetch(base: &str, path: &str) -> (usize, Duration) {
        let hostport = base.trim_start_matches("http://").trim_end_matches('/');
        let t = Instant::now();
        let mut s = ClientStream::connect(hostport).await.unwrap();
        let req = format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        (buf.len(), t.elapsed())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn serves_fonts_quickly_and_concurrently() {
        let server = start().await.unwrap();
        let base = server.url.clone();

        let (n1, single) = fetch(&base, "/assets/Nunito_Regular.ttf").await;
        assert!(n1 > 1000, "expected a real font body, got {n1} bytes");

        // Mirror installFontRequest's Promise.all of 3 concurrent font fetches.
        let paths = [
            "/assets/Nunito_Regular.ttf",
            "/assets/NotoEmoji-Regular.ttf",
            "/assets/Nunito_Bold.ttf",
        ];
        let t = Instant::now();
        let mut handles = Vec::new();
        for p in paths {
            let b = base.clone();
            handles.push(tokio::spawn(async move { fetch(&b, p).await }));
        }
        for h in handles {
            let (n, _) = h.await.unwrap();
            assert!(n > 1000, "expected a real font body, got {n} bytes");
        }
        let concurrent = t.elapsed();
        eprintln!("phone_server timing: single={single:?} concurrent3={concurrent:?}");
        assert!(
            concurrent < Duration::from_secs(5),
            "concurrent font fetches too slow: {concurrent:?}"
        );
    }

    /// Read one HTTP/1.1 response (status + headers + Content-Length body) from a
    /// buffered reader, returning (status_line, body_len).
    async fn read_response<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R) -> (String, usize) {
        use tokio::io::AsyncBufReadExt;
        let mut status = String::new();
        r.read_line(&mut status).await.unwrap();
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            r.read_line(&mut h).await.unwrap();
            let line = h.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((n, v)) = line.split_once(':') {
                if n.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap();
                }
            }
        }
        let mut body = vec![0u8; len];
        r.read_exact(&mut body).await.unwrap();
        (status.trim_end().to_string(), body.len())
    }

    /// Two requests over a SINGLE connection must both succeed — proving the
    /// keep-alive loop reuses the socket instead of one-shot-closing it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn keep_alive_reuses_one_connection() {
        let server = start().await.unwrap();
        let hostport = server
            .url
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let stream = ClientStream::connect(&hostport).await.unwrap();
        let mut conn = tokio::io::BufReader::new(stream);

        // First request keeps the connection alive.
        conn.get_mut()
            .write_all(b"GET /assets/Nunito_Regular.ttf HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
            .await
            .unwrap();
        let (status1, len1) = read_response(&mut conn).await;
        assert!(status1.contains("200"), "first response: {status1}");
        assert!(len1 > 1000, "first body {len1} bytes");

        // Second request on the SAME socket; closes after.
        conn.get_mut()
            .write_all(b"GET /assets/Nunito_Bold.ttf HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let (status2, len2) = read_response(&mut conn).await;
        assert!(status2.contains("200"), "second response: {status2}");
        assert!(len2 > 1000, "second body {len2} bytes");
    }

    /// A client that advertises gzip must get a `Content-Encoding: gzip` body
    /// that decompresses back to the real asset and is meaningfully smaller.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn gzip_encodes_compressible_assets() {
        use std::io::Read;
        let server = start().await.unwrap();
        let hostport = server
            .url
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let stream = ClientStream::connect(&hostport).await.unwrap();
        let mut conn = tokio::io::BufReader::new(stream);
        conn.get_mut()
            .write_all(b"GET /assets/Nunito_Regular.ttf HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

        use tokio::io::AsyncBufReadExt;
        let mut status = String::new();
        conn.read_line(&mut status).await.unwrap();
        assert!(status.contains("200"), "{status}");
        let mut gzipped = false;
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            conn.read_line(&mut h).await.unwrap();
            let line = h.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((n, v)) = line.split_once(':') {
                if n.eq_ignore_ascii_case("content-encoding") && v.trim().eq_ignore_ascii_case("gzip")
                {
                    gzipped = true;
                } else if n.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap();
                }
            }
        }
        assert!(gzipped, "expected gzip encoding");
        let raw = std::fs::read(demo_web_dir().join("assets/Nunito_Regular.ttf")).unwrap();
        assert!(len < raw.len(), "compressed {len} should be < raw {}", raw.len());

        let mut body = vec![0u8; len];
        tokio::io::AsyncReadExt::read_exact(&mut conn, &mut body)
            .await
            .unwrap();
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(&body[..])
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, raw, "gunzip must reproduce the original font");
    }

    #[test]
    fn percent_decode_roundtrips_beacon_text() {
        // What encodeURIComponent("123ms font primary done 4096B 7ms") yields.
        assert_eq!(
            percent_decode("123ms%20font%20primary%20done%204096B%207ms"),
            "123ms font primary done 4096B 7ms"
        );
        // Lone % and bad escapes pass through untouched.
        assert_eq!(percent_decode("50%"), "50%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
    }

    /// The diag beacon must log and return success without closing a kept-alive
    /// connection, so the phone can keep reporting milestones.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn diag_beacon_returns_no_content_and_keeps_alive() {
        let server = start().await.unwrap();
        let hostport = server
            .url
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let stream = ClientStream::connect(&hostport).await.unwrap();
        let mut conn = tokio::io::BufReader::new(stream);

        conn.get_mut()
            .write_all(b"GET /__diag/hello%20world HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
            .await
            .unwrap();
        let (status, len) = read_response(&mut conn).await;
        assert!(status.contains("204"), "diag status: {status}");
        assert_eq!(len, 0);

        // Connection still usable for a real asset afterwards.
        conn.get_mut()
            .write_all(b"GET /assets/Nunito_Regular.ttf HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let (status2, len2) = read_response(&mut conn).await;
        assert!(status2.contains("200") && len2 > 1000, "{status2} {len2}");
    }
}
