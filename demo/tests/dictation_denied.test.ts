/**
 * Dictation with the microphone permission denied (a file of its own:
 * Chrome's launch flags differ from dictation.test.ts, which grants it).
 * The tap on the microphone key must report the denial, without a retry,
 * and never listen.
 *
 * Needs the dictation worker and model in the served site, as
 * dictation.test.ts does.
 *
 * Run:  cd demo && bunx playwright test dictation_denied
 */

import { test, expect, devices, type Page } from "@playwright/test";
import { join } from "path";

const BASE = process.env.DEMO_URL ?? "/";
const SHOTS = join(__dirname, "snapshots");

const pixel = devices["Pixel 7"];
test.use({
  viewport: pixel.viewport,
  deviceScaleFactor: 2,
  isMobile: true,
  hasTouch: true,
  userAgent: pixel.userAgent,
  channel: "chrome",
  launchOptions: {
    args: ["--use-fake-device-for-media-stream", "--deny-permission-prompts"],
  },
});

// Must match the DEMOS / TESTS arrays in demo-ui/src/specs.rs.
const DEMO_COUNT = 34;
const TEST_COUNT = 13;
const TEXT_EDIT_IDX = 2;

function seedState(openIdx: number, w: number, h: number): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s += i === openIdx ? `d${i}=1,0,0,${w},${h},0\n` : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

async function waitForState(page: Page, pattern: RegExp, timeoutMs = 60_000): Promise<string> {
  const t0 = Date.now();
  for (;;) {
    const s =
      (await page.evaluate(() =>
        (window as unknown as Record<string, any>).__wasm["dictation_state"](),
      )) ?? "none";
    if (pattern.test(s)) return s;
    if (Date.now() - t0 > timeoutMs) throw new Error(`state stuck at ${s}, wanted ${pattern}`);
    await page.waitForTimeout(20);
  }
}

test("a tap reports the denied microphone without a retry", async ({ page }) => {
  test.setTimeout(120_000);
  const vp = page.viewportSize()!;
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(TEXT_EDIT_IDX, vp.width, vp.height));
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  // The text field brings the keyboard up; the model loads.
  await page.touchscreen.tap(250, 340);
  await waitForState(page, /^Ready$/);
  // The microphone key, in the keyboard's toolbar.
  await page.touchscreen.tap(380, 506);
  const denied = await waitForState(page, /^Error/, 10_000);
  expect(denied).toContain("Microphone access was denied");
  expect(denied).toContain("retry: false");
  await page.screenshot({ path: join(SHOTS, "dictation-7-denied.png") });
});
