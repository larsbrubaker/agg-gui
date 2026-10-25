/**
 * The on-screen keyboard stays on screen while Android Chrome's address bar
 * shows.
 *
 * On Android Chrome `100vh` is the *large* viewport, measured with the
 * address bar hidden; while the bar shows, `innerHeight` is about 56 CSS px
 * shorter. A page sized with `100vh` then lays its canvas out below the
 * screen's bottom edge, and the app's bottom, where the keyboard sits, is cut
 * off: the keyboard's bottom row (?123, comma, space, period, Enter) is lost.
 *
 * Desktop Chrome has no address bar to emulate, so the test reproduces the
 * mismatch directly: at the Pixel 7 profile (412 × 839) it pins the page's
 * body and canvas wrapper at 839 + 56 px — what `100vh` measures there — and
 * then resizes the window between the bar-shown and bar-hidden heights. The
 * demo's harness (`src/app.ts`, `capToVisibleViewport`) must hold the canvas,
 * and so the app's height, to `innerHeight`. The keyboard is painted from the
 * app viewport's bottom edge up, so a canvas whose bottom edge is on screen,
 * and whose backing store is its own CSS size (not stretched), shows the
 * keyboard's bottom row.
 *
 * `index.html` itself uses `100dvh` (with a `100vh` fallback), checked here
 * too, so the page fits without the harness's help.
 *
 * Run:  cd demo && bunx playwright test viewport_bar
 */

import { test, expect, devices, type Page } from "@playwright/test";

// The Pixel 7 profile: 412 × 839 CSS px at DPR 2.625, an Android UA, touch.
const { defaultBrowserType: _browser, ...PIXEL } = devices["Pixel 7"];
test.use({ ...PIXEL, channel: "chrome" });

const W = PIXEL.viewport.width; // 412
const BAR_SHOWN = PIXEL.viewport.height; // 839: innerHeight with the bar
const BAR = 56; // Android Chrome's address bar, CSS px
const LARGE_VH = BAR_SHOWN + BAR; // what 100vh measures: the bar hidden

// Must match the DEMOS / TESTS arrays in demo-ui/src/specs.rs.
const DEMO_COUNT = 35;
const TEST_COUNT = 13;
const MOBILE_KEYBOARD_IDX = 4;

// The one open window, in the app's Y-up coordinates. On a phone the demo
// docks it under the menu bar whatever the canvas's height.
const WIN = { x: 0, y: 100, w: 412, h: 650 };
// The demo's first text field, from the canvas's top edge (CSS px; measured on
// a screenshot at this profile).
const FIELD_FROM_CANVAS_TOP = 576;

function seedState(): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s +=
      i === MOBILE_KEYBOARD_IDX
        ? `d${i}=1,${WIN.x},${WIN.y},${WIN.w},${WIN.h},0\n`
        : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

/** Boot with the Mobile Keyboard demo open and the page laid out at
 *  `LARGE_VH`, as a `100vh` page is on Android Chrome. */
async function bootAsLargeViewportPage(page: Page): Promise<void> {
  await page.addInitScript(
    ([state, largeVh]: [string, number]) => {
      localStorage.setItem("agg-gui-demo-state", state);
      document.addEventListener("DOMContentLoaded", () => {
        const style = document.createElement("style");
        style.textContent =
          `body, .canvas-wrap { height: ${largeVh}px !important; }`;
        document.head.appendChild(style);
      });
    },
    [seedState(), LARGE_VH] as [string, number],
  );
  await page.goto("/");
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(500);
}

interface Geometry {
  innerHeight: number;
  pageHeight: number;
  canvasTop: number;
  canvasBottom: number;
  canvasCssHeight: number;
  backingCssHeight: number;
  scrollY: number;
  keyboardVisible: boolean;
}

async function geometry(page: Page): Promise<Geometry> {
  return page.evaluate(() => {
    const canvas = document.getElementById("canvas") as HTMLCanvasElement;
    const rect = canvas.getBoundingClientRect();
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return {
      innerHeight: window.innerHeight,
      pageHeight: parseFloat(getComputedStyle(document.body).height),
      canvasTop: rect.top,
      canvasBottom: rect.bottom,
      canvasCssHeight: rect.height,
      backingCssHeight: canvas.height / window.devicePixelRatio,
      scrollY: window.scrollY,
      keyboardVisible: wasm["software_keyboard_visible"]() as boolean,
    };
  });
}

/** The keyboard's bottom row is on screen: the app's bottom edge (the
 *  canvas's) lies on the window's bottom, and the app's height is the
 *  canvas's CSS height, not a taller backing store squeezed into it. */
function expectKeyboardBottomRowOnScreen(g: Geometry): void {
  expect(g.keyboardVisible, "the on-screen keyboard is up").toBe(true);
  expect(g.scrollY, "the page did not scroll").toBe(0);
  expect(g.canvasBottom, "the canvas's bottom edge is on screen").toBeLessThanOrEqual(
    g.innerHeight + 0.5,
  );
  expect(g.canvasBottom, "the canvas reaches the window's bottom").toBeGreaterThanOrEqual(
    g.innerHeight - 1,
  );
  expect(
    Math.abs(g.backingCssHeight - g.canvasCssHeight),
    "the app's height is the canvas's visible height",
  ).toBeLessThanOrEqual(1);
}

test("the keyboard's bottom row stays on screen on a 100vh page with the address bar showing", async ({
  page,
}) => {
  await bootAsLargeViewportPage(page);
  const before = await geometry(page);
  // The emulation took: the page is laid out taller than the window.
  expect(before.innerHeight).toBe(BAR_SHOWN);
  expect(before.pageHeight).toBe(LARGE_VH);

  // Tap the first text field: the Android keyboard slides up.
  await page.touchscreen.tap(W / 2, before.canvasTop + FIELD_FROM_CANVAS_TOP);
  await page.waitForTimeout(800);

  const shown = await geometry(page);
  expectKeyboardBottomRowOnScreen(shown);
  expect(shown.canvasCssHeight).toBeCloseTo(BAR_SHOWN, 0);

  // The bar hides (innerHeight grows to what 100vh measures): the app
  // follows and is its full height again.
  await page.setViewportSize({ width: W, height: LARGE_VH });
  await page.waitForTimeout(500);
  const hidden = await geometry(page);
  expectKeyboardBottomRowOnScreen(hidden);
  expect(hidden.canvasCssHeight).toBeCloseTo(LARGE_VH, 0);

  // And shows again.
  await page.setViewportSize({ width: W, height: BAR_SHOWN });
  await page.waitForTimeout(500);
  const again = await geometry(page);
  expectKeyboardBottomRowOnScreen(again);
  expect(again.canvasCssHeight).toBeCloseTo(BAR_SHOWN, 0);
});

test("the demo page sizes itself to the dynamic viewport (100dvh)", async ({ page }) => {
  await page.goto("/");
  const heights = await page.evaluate(() => {
    const found: Record<string, string> = {};
    for (const sheet of Array.from(document.styleSheets)) {
      for (const rule of Array.from(sheet.cssRules)) {
        if (rule instanceof CSSStyleRule && ["body", ".canvas-wrap"].includes(rule.selectorText)) {
          found[rule.selectorText] = rule.style.height;
        }
      }
    }
    return found;
  });
  // A browser that understands dvh keeps the later declaration; the 100vh
  // before it is the fallback for one that does not.
  expect(heights).toEqual({ body: "100dvh", ".canvas-wrap": "100dvh" });
});
