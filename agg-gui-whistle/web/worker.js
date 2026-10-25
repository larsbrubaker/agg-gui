// agg-gui-whistle's transcription Web Worker: a module worker the provider
// starts (src/web/mod.rs) from Config::worker_url. It loads the worker wasm
// module (agg-gui-whistle-worker, built with simd128 by the app's build into
// this file's directory as agg_gui_whistle_worker.js + _bg.wasm) and hands
// it every message; all the logic is Rust (src/worker/). Messages that
// arrive while the module loads are queued and passed to run() in order.
// If the module cannot load, the provider is told the worker could not start.
const queued = [];
self.onmessage = (event) => queued.push(event.data);

try {
  const glue = await import(new URL("./agg_gui_whistle_worker.js", import.meta.url).href);
  await glue.default();
  glue.run(queued);
} catch (err) {
  self.postMessage({
    type: "loadFailed",
    reason: "startup",
    detail: err instanceof Error ? err.message : String(err),
  });
}
