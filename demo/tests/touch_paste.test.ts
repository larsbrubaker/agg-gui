/**
 * Touch Paste reads the phone's system clipboard (the WASM demo at the
 * Pixel 7 profile: 412 CSS px wide, touch, Android).
 *
 * Text copied in another app is only on the system clipboard, never in the
 * page's own clipboard, so the touch selection toolbar's Paste reads it with
 * `navigator.clipboard.readText()` inside the tap
 * (`agg-gui/src/clipboard/system_paste.rs`) and pastes it into the field
 * when the promise resolves. This grants clipboard access, writes text with
 * `navigator.clipboard.writeText`, long-presses a word in the TextEdit demo,
 * taps Paste and reads the field back (`focused_text`).
 *
 * Determinism: localStorage is preseeded so exactly the TextEdit window is
 * open, filling the phone's width, as in touch.test.ts. Touches are real CDP
 * touch events. The coordinates are CSS pixels on that layout.
 *
 * Run:  cd demo && bunx playwright test touch_paste
 */

import { test, expect, devices, type Page, type CDPSession } from "@playwright/test";
import { DEMO_COUNT, TEST_COUNT, demoIndex } from "./demo_specs";

const { defaultBrowserType: _ignored, ...PIXEL } = devices["Pixel 7"];
test.use({ ...PIXEL, channel: "chrome" });

const TEXT_EDIT_IDX = demoIndex("TextEdit");

/** The word "this" in the multi-line editor's "Edit this text". */
const WORD = { x: 101, y: 287 };
/** The toolbar's Paste button (Cut, Copy, Paste, Select all) above it. */
const PASTE = { x: 173, y: 246 };

function seedState(openIdx: number): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s += i === openIdx ? `d${i}=1,0,0,412,700,0\n` : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

async function touchAt(cdp: CDPSession, page: Page, at: { x: number; y: number }, holdMs: number) {
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x: at.x, y: at.y, id: 1 }],
  });
  await page.waitForTimeout(holdMs);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(300);
}

async function focusedText(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return wasm["focused_text"]() as string | null;
  });
}

test("Paste on the touch toolbar pastes what the system clipboard holds", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(TEXT_EDIT_IDX));
  await page.goto("/");
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 20_000 });
  await page.waitForTimeout(300);
  // Copied "in another app": on the system clipboard only.
  await page.evaluate(() => navigator.clipboard.writeText("pasted"));

  const cdp = await context.newCDPSession(page);
  await touchAt(cdp, page, WORD, 900); // long press: select "this"
  expect(await focusedText(page)).toBe("Edit this text");
  await touchAt(cdp, page, PASTE, 30); // tap Paste
  await expect.poll(() => focusedText(page)).toBe("Edit pasted text");
});
