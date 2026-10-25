/**
 * End-to-end touch-input tests for the WASM demo.
 *
 * Exercises the full touch pipeline: real DOM `TouchEvent`s on the canvas
 * → the raw-forwarding listeners in `src/app.ts` → the wasm-bindgen
 * bindings (`demo-wasm/src/input.rs`) → agg-gui core, where
 * `touch_emulation.rs` replays the primary finger as mouse events and
 * `touch_state.rs` aggregates multi-finger gestures — finally consumed by
 * the Lion and Multi Touch demo widgets.
 *
 * Determinism: each test preseeds `localStorage["agg-gui-demo-state"]` so
 * exactly one demo window is open at a known rectangle (window coords are
 * Y-up, so y=150,h=560 on an 800px canvas puts the title bar at CSS y≈90).
 * Frames are page screenshots, taken once the demo's frame loop has painted
 * what the events changed, and compared region-by-region with a small
 * per-channel tolerance: a gesture must change the demo's canvas
 * region while leaving the window's header strip untouched — which also
 * guards against the old behaviour where a one-finger drag scrolled the
 * window instead of driving the widget.
 *
 * Run:  cd demo && bunx playwright test touch
 */

import { test, expect, type Page } from "@playwright/test";
import { DEMO_COUNT, TEST_COUNT, demoIndex } from "./demo_specs";

const W = 1200;
const H = 800;

const LION_IDX = demoIndex("Lion");
const MULTI_TOUCH_IDX = demoIndex("Multi Touch");

// The single open window's rect in the app's Y-up coordinates.
const WIN = { x: 340, y: 150, w: 520, h: 560 };
// Same rect's vertical extent in CSS (Y-down) coordinates.
const WIN_TOP_CSS = H - (WIN.y + WIN.h); // 90
const WIN_BOTTOM_CSS = H - WIN.y; // 650

/** Serialized SavedState with exactly one demo window open. */
function seedState(openIdx: number): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s +=
      i === openIdx
        ? `d${i}=1,${WIN.x},${WIN.y},${WIN.w},${WIN.h},0\n`
        : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

async function bootWithState(page: Page, openIdx: number): Promise<void> {
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(openIdx));
  await page.setViewportSize({ width: W, height: H });
  await page.goto("/");
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 20_000 });
  await page.waitForTimeout(300);
}

/**
 * Capture what the page shows under `key`, as RGBA pixels stored page-side
 * (typed arrays don't round-trip through evaluate cheaply). It first waits
 * for the demo's frame loop to paint, which is also what latches / consumes
 * multi-touch gesture deltas between event batches, then decodes a page
 * screenshot in the page.
 */
async function capture(page: Page, key: string): Promise<void> {
  await page.waitForTimeout(80);
  const png = (await page.screenshot()).toString("base64");
  await page.evaluate(
    async ([key, png, w, h]: [string, string, number, number]) => {
      const win = window as unknown as Record<string, any>;
      const img = new Image();
      img.src = `data:image/png;base64,${png}`;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = w;
      c.height = h;
      const ctx = c.getContext("2d")!;
      ctx.drawImage(img, 0, 0);
      win.__frames = win.__frames ?? {};
      win.__frames[key] = ctx.getImageData(0, 0, w, h).data;
    },
    [key, png, W, H] as [string, string, number, number],
  );
}

/** Fraction of pixels inside `rect` (CSS/Y-down coords) differing between
 *  two captured frames by more than a small per-channel tolerance. */
async function diffFraction(
  page: Page,
  keyA: string,
  keyB: string,
  rect: { x: number; y: number; w: number; h: number },
): Promise<number> {
  return page.evaluate(
    ([keyA, keyB, r, w]: [
      string,
      string,
      { x: number; y: number; w: number; h: number },
      number,
    ]) => {
      const frames = (window as unknown as Record<string, any>).__frames;
      const a: Uint8Array = frames[keyA];
      const b: Uint8Array = frames[keyB];
      const TOL = 12;
      let differing = 0;
      let total = 0;
      for (let y = r.y; y < r.y + r.h; y++) {
        for (let x = r.x; x < r.x + r.w; x++) {
          const i = (y * w + x) * 4;
          total++;
          if (
            Math.abs(a[i] - b[i]) > TOL ||
            Math.abs(a[i + 1] - b[i + 1]) > TOL ||
            Math.abs(a[i + 2] - b[i + 2]) > TOL
          ) {
            differing++;
          }
        }
      }
      return differing / Math.max(1, total);
    },
    [keyA, keyB, rect, W] as [
      string,
      string,
      { x: number; y: number; w: number; h: number },
      number,
    ],
  );
}

/** Dispatch a real TouchEvent on the canvas.  Our listeners read only
 *  `changedTouches`, so `touches` is passed for spec-fidelity only. */
async function touch(
  page: Page,
  type: "touchstart" | "touchmove" | "touchend" | "touchcancel",
  changed: { id: number; x: number; y: number }[],
  remaining: { id: number; x: number; y: number }[] = [],
): Promise<void> {
  await page.evaluate(
    ([type, changed, remaining]: [
      string,
      { id: number; x: number; y: number }[],
      { id: number; x: number; y: number }[],
    ]) => {
      const canvas = document.getElementById("canvas") as HTMLCanvasElement;
      const mk = (p: { id: number; x: number; y: number }) =>
        new Touch({
          identifier: p.id,
          target: canvas,
          clientX: p.x,
          clientY: p.y,
        });
      const changedTouches = changed.map(mk);
      const touches = (
        type === "touchend" || type === "touchcancel" ? remaining : changed
      ).map(mk);
      canvas.dispatchEvent(
        new TouchEvent(type, {
          touches,
          changedTouches,
          targetTouches: touches,
          bubbles: true,
          cancelable: true,
        }),
      );
    },
    [type, changed, remaining] as [
      string,
      { id: number; x: number; y: number }[],
      { id: number; x: number; y: number }[],
    ],
  );
}

// Region safely inside the lion / multi-touch canvas (below the header
// labels, above the window's bottom edge), in CSS coordinates.
const CANVAS_REGION = { x: 400, y: 320, w: 400, h: 260 };
// The window's title bar and Alpha slider: gestures on the canvas region
// must NOT disturb them (the old bug scrolled the whole window content).
// The help text under the slider is left out: the lion, grown or turned,
// paints over it (it is not clipped to its canvas on the wgpu backend).
const HEADER_REGION = { x: WIN.x + 10, y: WIN_TOP_CSS + 4, w: WIN.w - 20, h: 80 };

test("one-finger touch drag rotates the lion (not scrolls)", async ({ page }) => {
  await bootWithState(page, LION_IDX);
  await capture(page, "before");

  // Drag straight down, right of the lion's centre — a polar-angle change
  // around the widget centre, i.e. a rotation.  Well above the 8px
  // tap-vs-drag threshold.
  await touch(page, "touchstart", [{ id: 1, x: 720, y: 445 }]);
  await capture(page, "t0");
  for (let i = 1; i <= 5; i++) {
    await touch(page, "touchmove", [{ id: 1, x: 720, y: 445 + i * 20 }]);
    await capture(page, `t${i}`);
  }
  await touch(page, "touchend", [{ id: 1, x: 720, y: 545 }]);
  await capture(page, "after");

  const lionChanged = await diffFraction(page, "before", "after", CANVAS_REGION);
  const headerChanged = await diffFraction(page, "before", "after", HEADER_REGION);
  expect(
    lionChanged,
    "lion canvas must visibly change after a one-finger rotate drag",
  ).toBeGreaterThan(0.02);
  expect(
    headerChanged,
    "window header must stay put — a drag that scrolls the window is the old bug",
  ).toBeLessThan(0.005);
  expect(WIN_BOTTOM_CSS).toBe(650); // sanity: geometry assumptions hold
});

/** Boot the Lion demo and pinch out around its centre, capturing "before"
 *  and "after". */
async function pinchLion(page: Page): Promise<void> {
  await bootWithState(page, LION_IDX);
  await capture(page, "before");

  // Horizontal pinch-out around the lion centre: spread 120px → 280px.
  await touch(page, "touchstart", [{ id: 1, x: 540, y: 445 }]);
  await touch(
    page,
    "touchstart",
    [{ id: 2, x: 660, y: 445 }],
    [{ id: 1, x: 540, y: 445 }],
  );
  await capture(page, "latch"); // first paint latches gesture baseline
  for (let i = 1; i <= 4; i++) {
    await touch(page, "touchmove", [
      { id: 1, x: 540 - i * 20, y: 445 },
      { id: 2, x: 660 + i * 20, y: 445 },
    ]);
    await capture(page, `z${i}`);
  }
  await touch(page, "touchend", [{ id: 1, x: 460, y: 445 }], [{ id: 2, x: 740, y: 445 }]);
  await touch(page, "touchend", [{ id: 2, x: 740, y: 445 }]);
  await capture(page, "after");
}

// The window's title bar alone: the zoomed lion reaches every row of the
// window's content (see the next test), never the title bar.
const TITLE_REGION = { x: WIN.x + 10, y: WIN_TOP_CSS + 4, w: WIN.w - 20, h: 18 };

test("two-finger pinch zooms the lion", async ({ page }) => {
  await pinchLion(page);
  const lionChanged = await diffFraction(page, "before", "after", CANVAS_REGION);
  const titleChanged = await diffFraction(page, "before", "after", TITLE_REGION);
  expect(
    lionChanged,
    "lion must visibly grow after a two-finger pinch-out",
  ).toBeGreaterThan(0.02);
  expect(
    titleChanged,
    "pinch must not move or disturb the window",
  ).toBeLessThan(0.005);
});

// Known bug: the lion is not clipped to its canvas on the wgpu backend. Grown
// by a pinch it paints over the Alpha slider and the help text above the
// canvas (and over the window's bottom edge). This passes once that is fixed;
// remove `test.fail` then.
test.fail("a pinched lion stays inside its canvas", async ({ page }) => {
  await pinchLion(page);
  const headerChanged = await diffFraction(page, "before", "after", HEADER_REGION);
  expect(headerChanged, "the slider and help text stay as they were").toBeLessThan(0.005);
});

test("two-finger twist drives the Multi Touch demo arrow", async ({ page }) => {
  await bootWithState(page, MULTI_TOUCH_IDX);
  await capture(page, "before");

  // Pure 90° rotation about (600, 425): both fingers orbit, distance kept.
  await touch(page, "touchstart", [{ id: 1, x: 520, y: 425 }]);
  await touch(
    page,
    "touchstart",
    [{ id: 2, x: 680, y: 425 }],
    [{ id: 1, x: 520, y: 425 }],
  );
  await capture(page, "latch");
  // Rotate in 30° steps: finger positions on the circle of radius 80.
  const steps = [30, 60, 90];
  for (const deg of steps) {
    const rad = (deg * Math.PI) / 180;
    const dx = Math.cos(rad) * 80;
    const dy = Math.sin(rad) * 80;
    await touch(page, "touchmove", [
      { id: 1, x: 600 - dx, y: 425 - dy },
      { id: 2, x: 600 + dx, y: 425 + dy },
    ]);
    await capture(page, `r${deg}`);
  }
  await touch(page, "touchend", [{ id: 1, x: 600, y: 345 }], [{ id: 2, x: 600, y: 505 }]);
  await touch(page, "touchend", [{ id: 2, x: 600, y: 505 }]);
  await capture(page, "after");

  const arrowChanged = await diffFraction(page, "before", "after", CANVAS_REGION);
  expect(
    arrowChanged,
    "the demo arrow must visibly rotate after a two-finger twist",
  ).toBeGreaterThan(0.005);
});
