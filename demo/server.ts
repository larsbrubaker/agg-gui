// Dev server with watch mode — rebuilds WASM + TS on Rust/TS file changes.
// Usage: bun run server.ts  (or: bun run dev)
//
// Port 3001 — port 3000 is reserved for the Manifold project.

import { join, extname } from "path";
import { watch } from "fs";

const PORT = 3001;
const DEMO_DIR = import.meta.dir;
const PROJECT_ROOT = join(DEMO_DIR, "..");
const PUBLIC_DIR = join(DEMO_DIR, "public");

const MIME_TYPES: Record<string, string> = {
  ".html": "text/html",
  ".css": "text/css",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".txt": "text/plain",
};

// --- Live reload via SSE ---
const reloadClients = new Set<ReadableStreamDefaultController>();

const RELOAD_SCRIPT = `<script>
(function(){
  const es = new EventSource("/__reload");
  es.onmessage = function(e) {
    if (e.data === "reload") { es.close(); location.reload(); }
  };
})();
</script>`;

function notifyReload() {
  for (const controller of reloadClients) {
    try { controller.enqueue("data: reload\n\n"); }
    catch { reloadClients.delete(controller); }
  }
}

// --- File serving ---
async function serveFile(pathname: string): Promise<Response | null> {
  for (const base of [PUBLIC_DIR, DEMO_DIR]) {
    const file = Bun.file(join(base, pathname));
    if (!(await file.exists())) continue;

    const ext = extname(pathname);
    const contentType = MIME_TYPES[ext] || "application/octet-stream";

    if (ext === ".html") {
      let html = await file.text();
      html = html.replace("</body>", `${RELOAD_SCRIPT}</body>`);
      return new Response(html, {
        headers: { "Content-Type": "text/html", "Cache-Control": "no-store", "Access-Control-Allow-Origin": "*" },
      });
    }

    return new Response(file, {
      headers: { "Content-Type": contentType, "Cache-Control": "no-store", "Access-Control-Allow-Origin": "*" },
    });
  }
  return null;
}

// --- Build helpers ---
let building = false;
let pendingRust = false;
let pendingTs = false;

async function runCommand(cmd: string[], cwd: string, label: string): Promise<boolean> {
  console.log(`\x1b[36m[${label}]\x1b[0m Building…`);
  const t = Date.now();
  const proc = Bun.spawn(cmd, { cwd, stdout: "inherit", stderr: "inherit" });
  const code = await proc.exited;
  const s = ((Date.now() - t) / 1000).toFixed(1);
  if (code === 0) { console.log(`\x1b[32m[${label}]\x1b[0m Done in ${s}s`); return true; }
  console.error(`\x1b[31m[${label}]\x1b[0m Failed (exit ${code})`);
  return false;
}

async function buildWasm(): Promise<boolean> {
  return runCommand(
    ["wasm-pack", "build", "demo-wasm", "--dev", "--target", "web", "--out-dir", "../demo/public/pkg", "--no-typescript"],
    PROJECT_ROOT, "wasm"
  );
}

async function buildTs(): Promise<boolean> {
  return runCommand(["bun", "run", "build.ts"], DEMO_DIR, "ts");
}

async function rebuild() {
  if (building) return;
  building = true;
  while (pendingRust || pendingTs) {
    const doRust = pendingRust;
    const doTs = pendingTs;
    pendingRust = false;
    pendingTs = false;
    let changed = false;
    if (doRust) { if (await buildWasm()) changed = true; }
    if (doTs || doRust) { if (await buildTs()) changed = true; }
    if (changed) notifyReload();
  }
  building = false;
}

function debounce(fn: () => void, ms: number) {
  let t: ReturnType<typeof setTimeout> | null = null;
  return () => { if (t) clearTimeout(t); t = setTimeout(fn, ms); };
}

const triggerRust = debounce(() => { pendingRust = true; rebuild(); }, 300);
const triggerTs   = debounce(() => { pendingTs   = true; rebuild(); }, 200);

// --- Watch ---
for (const dir of [
  join(PROJECT_ROOT, "agg-gui", "src"),
  join(PROJECT_ROOT, "demo-wasm", "src"),
  join(PROJECT_ROOT, "demo-native", "src"),
]) {
  watch(dir, { recursive: true }, (_, f) => {
    if (f?.endsWith(".rs")) { console.log(`\x1b[33m[watch]\x1b[0m Rust: ${f}`); triggerRust(); }
  });
}

watch(join(DEMO_DIR, "src"), { recursive: true }, (_, f) => {
  if (f?.endsWith(".ts")) { console.log(`\x1b[33m[watch]\x1b[0m TS: ${f}`); triggerTs(); }
});

watch(DEMO_DIR, { recursive: false }, (_, f) => {
  if (f?.endsWith(".html") || f?.endsWith(".css")) notifyReload();
});

// --- HTTP server ---
const server = Bun.serve({
  port: PORT,
  idleTimeout: 0,
  async fetch(req) {
    const url = new URL(req.url);
    let pathname = decodeURIComponent(url.pathname);

    if (pathname === "/__reload") {
      let ctrl: ReadableStreamDefaultController;
      const stream = new ReadableStream({
        start(c) { ctrl = c; reloadClients.add(c); c.enqueue("data: connected\n\n"); },
        cancel() { reloadClients.delete(ctrl); },
      });
      return new Response(stream, {
        headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", "Access-Control-Allow-Origin": "*" },
      });
    }

    if (pathname === "/") pathname = "/index.html";

    const resp = await serveFile(pathname);
    if (resp) return resp;
    return new Response("Not found", { status: 404 });
  },
});

console.log(`\x1b[32magg-gui demo running at http://localhost:${server.port}\x1b[0m`);
console.log("Watching: agg-gui/src/**/*.rs, demo-wasm/src/**/*.rs, demo/src/**/*.ts");
console.log("Press Ctrl+C to stop.");
