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

use tokio::io::AsyncBufReadExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
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

async fn handle(mut sock: TcpStream) -> std::io::Result<()> {
    let (read_half, write_half) = sock.split();
    let mut reader = BufReader::new(read_half);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    // Drain headers without parsing.
    let mut header_buf = [0u8; 4096];
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        reader.read(&mut header_buf),
    )
    .await;

    let mut writer = write_half;
    // Path may carry a query string (?host=...) — strip it for routing.
    let raw_path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");

    match resolve_asset(path) {
        Some((body, content_type)) => write_response(&mut writer, &body, content_type).await?,
        None => {
            let response =
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            writer.write_all(response.as_bytes()).await?;
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
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    writer.write_all(response.as_bytes()).await?;
    writer.write_all(body).await
}
