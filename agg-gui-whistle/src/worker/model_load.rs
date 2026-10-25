//! The worker's model download (wasm, feature `worker`): `whistle.cact`
//! from the Cache API when a checked copy is there, else from each URL in
//! turn, reporting progress, checked against the size and SHA-256 the
//! whistle crate was tested with (`whistle::model_file`, the check in
//! `model_check.rs`) and cached under that checksum, so a new model version
//! never reads an old copy. The digest is the browser's SubtleCrypto.
//! Mirrors whistle-rust's demo (`demo/web/src/model.ts`).
//!
//! A download silent for [`STALL_MS`] (no response, no bytes) is aborted
//! and counts as failed, so the next tap retries it. Progress
//! counts against the model's pinned size, not `Content-Length` (which
//! counts encoded bytes when the server compresses the body). When every
//! place fails, the last failure names the reason: a network failure after
//! a wrong file is a network failure.

use std::cell::Cell;
use std::rc::Rc;

use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, Cache, DedicatedWorkerGlobalScope, ReadableStreamDefaultReader, RequestInit,
    Response,
};

use crate::model_check::{self, Check};
use crate::protocol::LoadFailure;

/// The Cache API store; bump with a change of key scheme.
const CACHE_NAME: &str = "agg-gui-whistle-model-v1";

/// Progress is reported at most this often, in ms.
const PROGRESS_MS: f64 = 100.0;

/// A download with no response or no new bytes for this long has stalled,
/// in ms.
const STALL_MS: i32 = 20_000;

/// Aborts a download once it has been silent for [`STALL_MS`]; re-armed by
/// each chunk. Dropping it disarms it.
struct Watchdog {
    scope: DedicatedWorkerGlobalScope,
    controller: AbortController,
    stalled: Rc<Cell<bool>>,
    on_stall: Closure<dyn FnMut()>,
    timer: Option<i32>,
}

impl Watchdog {
    fn new(scope: &DedicatedWorkerGlobalScope) -> Result<Watchdog, JsValue> {
        let controller = AbortController::new()?;
        let stalled = Rc::new(Cell::new(false));
        let on_stall = {
            let (controller, stalled) = (controller.clone(), stalled.clone());
            Closure::<dyn FnMut()>::new(move || {
                stalled.set(true);
                controller.abort();
            })
        };
        let mut watchdog = Watchdog {
            scope: scope.clone(),
            controller,
            stalled,
            on_stall,
            timer: None,
        };
        watchdog.rearm();
        Ok(watchdog)
    }

    fn rearm(&mut self) {
        if let Some(timer) = self.timer.take() {
            self.scope.clear_timeout_with_handle(timer);
        }
        self.timer = self
            .scope
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                self.on_stall.as_ref().unchecked_ref(),
                STALL_MS,
            )
            .ok();
    }

    /// `err`, or that the download stalled when that is why it failed.
    fn reason(&self, url: &str, err: &JsValue) -> String {
        if self.stalled.get() {
            format!("{url}: stalled (nothing for {} s)", STALL_MS / 1000)
        } else {
            format!("{url}: {}", text(err))
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            self.scope.clear_timeout_with_handle(timer);
        }
    }
}

fn text(err: &JsValue) -> String {
    err.as_string()
        .or_else(|| {
            Reflect::get(err, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| format!("{err:?}"))
}

async fn sha256_hex(scope: &DedicatedWorkerGlobalScope, bytes: &[u8]) -> Result<String, String> {
    let subtle = scope.crypto().map_err(|e| text(&e))?.subtle();
    let promise = subtle
        .digest_with_str_and_u8_array("SHA-256", bytes)
        .map_err(|e| text(&e))?;
    let digest = JsFuture::from(promise).await.map_err(|e| text(&e))?;
    Ok(model_check::hex(&Uint8Array::new(&digest).to_vec()))
}

async fn check(scope: &DedicatedWorkerGlobalScope, bytes: &[u8]) -> Result<Check, String> {
    if !model_check::size_ok(bytes.len()) {
        return Ok(Check::Mismatch);
    }
    Ok(model_check::digest_check(&sha256_hex(scope, bytes).await?))
}

async fn open_cache(scope: &DedicatedWorkerGlobalScope) -> Option<Cache> {
    // No cache in private browsing or with storage disabled: download every
    // visit.
    let storage = scope.caches().ok()?;
    let cache = JsFuture::from(storage.open(CACHE_NAME)).await.ok()?;
    cache.dyn_into::<Cache>().ok()
}

async fn from_cache(
    scope: &DedicatedWorkerGlobalScope,
    cache: &Cache,
    key: &str,
) -> Option<Vec<u8>> {
    let hit = JsFuture::from(cache.match_with_str(key)).await.ok()?;
    let response = hit.dyn_into::<Response>().ok()?;
    let buffer = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
    let bytes = Uint8Array::new(&buffer).to_vec();
    if matches!(check(scope, &bytes).await, Ok(Check::Ok)) {
        return Some(bytes);
    }
    let _ = JsFuture::from(cache.delete_with_str(key)).await;
    None
}

/// Why a download produced no model.
enum Failed {
    /// The request or the transfer failed.
    Transfer(String),
    /// More bytes came than whistle.cact has: some other file (a download
    /// failure, with its own message, as natively).
    TooLarge(String),
}

async fn download(
    scope: &DedicatedWorkerGlobalScope,
    url: &str,
    progress: &dyn Fn(u64, u64),
) -> Result<Vec<u8>, Failed> {
    let fail = Failed::Transfer;
    let mut watchdog = Watchdog::new(scope).map_err(|e| fail(format!("{url}: {}", text(&e))))?;
    let init = RequestInit::new();
    init.set_signal(Some(&watchdog.controller.signal()));
    let response = JsFuture::from(scope.fetch_with_str_and_init(url, &init))
        .await
        .map_err(|e| fail(watchdog.reason(url, &e)))?;
    let response: Response = response
        .dyn_into()
        .map_err(|_| fail(format!("{url}: no response")))?;
    if !response.ok() {
        return Err(fail(format!("{url}: HTTP {}", response.status())));
    }
    let expected = model_check::expected_bytes();
    let total = expected as u64;
    let body = response
        .body()
        .ok_or_else(|| fail(format!("{url}: no body")))?;
    let reader: ReadableStreamDefaultReader = body
        .get_reader()
        .dyn_into()
        .map_err(|_| fail(format!("{url}: no reader")))?;
    let mut bytes = Vec::with_capacity(expected);
    let mut last = 0.0;
    loop {
        let chunk = JsFuture::from(reader.read())
            .await
            .map_err(|e| fail(watchdog.reason(url, &e)))?;
        watchdog.rearm();
        let done = Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok()
            .and_then(|d| d.as_bool())
            .unwrap_or(true);
        if done {
            break;
        }
        let value = Reflect::get(&chunk, &JsValue::from_str("value")).unwrap_or(JsValue::UNDEFINED);
        let value: Uint8Array = value
            .dyn_into()
            .map_err(|_| fail(format!("{url}: bad chunk")))?;
        if bytes.len() + value.length() as usize > expected {
            let _ = reader.cancel();
            return Err(Failed::TooLarge(format!(
                "{url} is larger than whistle.cact"
            )));
        }
        bytes.extend_from_slice(&value.to_vec());
        let now = js_sys::Date::now();
        if now - last >= PROGRESS_MS {
            last = now;
            progress(bytes.len() as u64, total);
        }
    }
    progress(bytes.len() as u64, total);
    Ok(bytes)
}

/// The checked model and where it came from ("cache" or the URL), or why
/// none could be had.
pub(super) async fn load(
    scope: &DedicatedWorkerGlobalScope,
    urls: &[String],
    progress: &dyn Fn(u64, u64),
) -> Result<(Vec<u8>, String), (LoadFailure, String)> {
    let key = model_check::cache_key();
    let cache = open_cache(scope).await;
    if let Some(cache) = &cache {
        if let Some(bytes) = from_cache(scope, cache, &key).await {
            progress(bytes.len() as u64, bytes.len() as u64);
            return Ok((bytes, "cache".into()));
        }
    }
    let mut failures = Vec::new();
    // The last failure's kind is the one reported.
    let mut last = LoadFailure::Download;
    for url in urls {
        let mut bytes = match download(scope, url, progress).await {
            Ok(bytes) => bytes,
            Err(Failed::Transfer(err)) => {
                last = LoadFailure::Download;
                failures.push(err);
                continue;
            }
            Err(Failed::TooLarge(err)) => {
                last = LoadFailure::Download;
                failures.push(err);
                continue;
            }
        };
        match check(scope, &bytes).await {
            Ok(Check::Ok) => {}
            Ok(Check::Mismatch) => {
                last = LoadFailure::Checksum;
                failures.push(format!("{url}: not the expected whistle.cact"));
                continue;
            }
            Err(err) => {
                last = LoadFailure::Download;
                failures.push(format!("{url}: {err}"));
                continue;
            }
        }
        if let Some(cache) = &cache {
            if let Ok(response) = Response::new_with_opt_u8_array(Some(&mut bytes)) {
                // Over quota: still usable this visit.
                let _ = JsFuture::from(cache.put_with_str(&key, &response)).await;
            }
        }
        return Ok((bytes, url.clone()));
    }
    if failures.is_empty() {
        failures.push("no model URL configured".into());
    }
    Err((last, failures.join("; ")))
}
