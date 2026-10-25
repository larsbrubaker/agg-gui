/**
 * End-to-end smoke test for the deployed WASM demo.
 *
 * Boots the page, waits for WASM init + first paint, then grabs the WebGL
 * canvas via `canvas.toDataURL` and writes it to
 * `tests/snapshots/wasm-screenshot.png`.  Runs basic sanity checks: the
 * PNG must decode, have non-zero size, contain more than a trivial number of
 * distinct colours, and not be entirely one colour.
 *
 * Purpose: gate `deploy-demo.yml` so a broken WASM build (panic on init,
 * blank canvas, font-load failure, GPU pipeline regression) cannot ship to
 * GitHub Pages.
 *
 * Why not use the in-app "Take Screenshot" + "Download" buttons?  Their
 * server-side path (`read_captured_screenshot` in `demo-wgpu`) does a
 * sync `device.poll(wait_indefinitely)` followed by `receiver.recv()`,
 * which deadlocks the JS main thread on wasm32 — the browser hangs
 * waiting for GPU progress that can't happen until the main thread
 * yields.  That's a real bug in the demo's Save path but separate from
 * "is the WASM build producing visible output".  Grabbing the canvas
 * via `toDataURL` exercises the entire WebGL2 render pipeline (shaders,
 * geometry, fonts) without touching the broken sync-readback path.
 *
 * Run:  cd demo && bunx playwright test screenshot
 */

import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";

const SNAPSHOT_DIR = path.join(__dirname, "snapshots");

/** Minimum distinct colour buckets we expect in a real, painted UI. */
const MIN_DISTINCT_COLOR_BUCKETS = 32;
/** No single colour bucket may dominate more than this fraction. */
const MAX_DOMINANT_FRACTION = 0.98;

test("WASM demo paints a non-blank canvas", async ({ page }) => {
  fs.mkdirSync(SNAPSHOT_DIR, { recursive: true });
  const outPath = path.join(SNAPSHOT_DIR, "wasm-screenshot.png");
  if (fs.existsSync(outPath)) fs.unlinkSync(outPath);

  // Surface any browser console errors so a wgpu / panic regression
  // shows up in the test log instead of silently producing a blank frame.
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });
  page.on("pageerror", (err) => consoleErrors.push(String(err)));

  await page.goto("/");
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 20_000 });
  // Let the animation loop settle one paint after loading hides.
  await page.waitForTimeout(500);

  // ── Capture the canvas the user actually sees ──────────────────────────
  // The GL canvas isn't allocated with preserveDrawingBuffer, so toDataURL
  // can return blank if called outside the rAF tick that did the draw.
  // Force a fresh render in the SAME synchronous JS task that calls
  // toDataURL.
  const dataUrl = await page.evaluate(() => {
    const w = window as unknown as Record<string, unknown>;
    const wasm = w.__wasm as Record<string, (...a: unknown[]) => unknown> | undefined;
    const canvas = document.getElementById("canvas") as HTMLCanvasElement;
    if (!wasm || !canvas) throw new Error("wasm / canvas missing");
    const dpr = window.devicePixelRatio || 1;
    const cw = Math.floor((canvas.parentElement?.clientWidth ?? 1200) * dpr);
    const ch = Math.floor((canvas.parentElement?.clientHeight ?? 800) * dpr);
    canvas.width = cw;
    canvas.height = ch;
    wasm["render"](cw, ch, 0);
    return canvas.toDataURL("image/png");
  });

  // Persist PNG to disk for human inspection on CI failures.
  const base64 = dataUrl.replace(/^data:image\/png;base64,/, "");
  fs.writeFileSync(outPath, Buffer.from(base64, "base64"));
  console.log(`Saved screenshot to ${outPath}`);

  const stat = fs.statSync(outPath);
  expect(stat.size, "PNG file must be non-empty").toBeGreaterThan(1024);

  const analysis = await page.evaluate(async (url: string) => {
    const img = new Image();
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("PNG failed to decode"));
      img.src = url;
    });
    const c = document.createElement("canvas");
    c.width = img.width;
    c.height = img.height;
    const ctx = c.getContext("2d")!;
    ctx.drawImage(img, 0, 0);
    const px = ctx.getImageData(0, 0, img.width, img.height).data;

    // Quantise to 4 bits/channel to bucket near-identical colours
    // together — kills anti-aliasing noise while still detecting
    // "everything is one shade of grey" failures.
    const buckets = new Map<number, number>();
    const totalPixels = px.length / 4;
    for (let i = 0; i < px.length; i += 4) {
      const r = px[i] >> 4;
      const g = px[i + 1] >> 4;
      const b = px[i + 2] >> 4;
      const key = (r << 8) | (g << 4) | b;
      buckets.set(key, (buckets.get(key) ?? 0) + 1);
    }
    let dominantCount = 0;
    for (const n of buckets.values())
      if (n > dominantCount) dominantCount = n;

    return {
      width: img.width,
      height: img.height,
      totalPixels,
      distinctBuckets: buckets.size,
      dominantFraction: dominantCount / totalPixels,
    };
  }, dataUrl);

  console.log(
    `Screenshot ${analysis.width}x${analysis.height} ` +
      `distinct colour buckets=${analysis.distinctBuckets} ` +
      `dominant=${(analysis.dominantFraction * 100).toFixed(1)}%`,
  );

  expect(analysis.width, "image width").toBeGreaterThan(0);
  expect(analysis.height, "image height").toBeGreaterThan(0);
  expect(
    analysis.distinctBuckets,
    `expected >${MIN_DISTINCT_COLOR_BUCKETS} distinct colour buckets — ` +
      "a near-blank or solid-fill screenshot fails this check",
  ).toBeGreaterThan(MIN_DISTINCT_COLOR_BUCKETS);
  expect(
    analysis.dominantFraction,
    `single colour bucket dominates >${(MAX_DOMINANT_FRACTION * 100).toFixed(
      0,
    )}% of pixels — the WASM render likely failed to paint UI content`,
  ).toBeLessThan(MAX_DOMINANT_FRACTION);

  expect(
    consoleErrors,
    `browser console errors during WASM run:\n${consoleErrors.join("\n")}`,
  ).toEqual([]);
});
