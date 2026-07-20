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
 * Frames are captured via `render_software_pixels` (the pure-software
 * renderer, pixel-identical across calls when nothing changed) and
 * compared region-by-region: a gesture must change the demo's canvas
 * region while leaving the window's header strip untouched — which also
 * guards against the old behaviour where a one-finger drag scrolled the
 * window instead of driving the widget.
 *
 * Run:  cd demo && bunx playwright test touch
 */

import { test, expect, type Page } from "@playwright/test";

const W = 1200;
const H = 800;

// Must match the DEMOS / TESTS arrays in demo-ui/src/specs.rs.
const DEMO_COUNT = 34;
const TEST_COUNT = 13;
const LION_IDX = 25;
const MULTI_TOUCH_IDX = 31;

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
 * Capture a deterministic software render under `key` (stored page-side —
 * typed arrays don't round-trip through evaluate cheaply).  Calling this
 * also advances the app one paint, which is what latches / consumes
 * multi-touch gesture deltas between event batches.
 */
async function capture(page: Page, key: string): Promise<void> {
  await page.evaluate(
    ([key, w, h]: [string, number, number]) => {
      const win = window as unknown as Record<string, any>;
      const wasm = win.__wasm;
      win.__frames = win.__frames ?? {};
      win.__frames[key] = new Uint8Array(wasm["render_software_pixels"](w, h));
    },
    [key, W, H] as [string, number, number],
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
// The window's title bar + header labels: gestures on the canvas region
// must NOT disturb it (the old bug scrolled the whole window content).
const HEADER_REGION = { x: WIN.x + 10, y: WIN_TOP_CSS + 4, w: WIN.w - 20, h: 120 };

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

test("two-finger pinch zooms the lion", async ({ page }) => {
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

  const lionChanged = await diffFraction(page, "before", "after", CANVAS_REGION);
  const headerChanged = await diffFraction(page, "before", "after", HEADER_REGION);
  expect(
    lionChanged,
    "lion must visibly grow after a two-finger pinch-out",
  ).toBeGreaterThan(0.02);
  expect(
    headerChanged,
    "pinch must not scroll or disturb the window header",
  ).toBeLessThan(0.005);
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
