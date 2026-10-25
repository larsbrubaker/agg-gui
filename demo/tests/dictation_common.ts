/**
 * What the dictation tests share (dictation.test.ts, dictation_denied.test.ts,
 * dictation_audio.test.ts): the saved state that opens the TextEdit demo
 * alone, reading the provider's state through the demo's `dictation_state`
 * export, the phone profile, and where to tap.
 *
 * The demo exposes no automation hooks for widget positions (its wasm
 * exports are the input entry points, the clipboard and `dictation_state`),
 * so the taps are positions measured on the Pixel 7 profile below with the
 * TextEdit demo filling the window: keep them here, in one place.
 */

import { devices, type Page } from "@playwright/test";
import { DEMO_COUNT, TEST_COUNT, demoIndex } from "./demo_specs";

/** The phone profile every dictation test runs on (Chrome, touch). */
const pixel = devices["Pixel 7"];
export const PHONE = {
  viewport: pixel.viewport,
  deviceScaleFactor: 2,
  isMobile: true,
  hasTouch: true,
  userAgent: pixel.userAgent,
  channel: "chrome",
};

/** The TextEdit demo's multi-line TextArea: a tap brings the keyboard up. */
export const FIELD = { x: 250, y: 340 };
/** The microphone key, in the keyboard's toolbar. */
export const MIC = { x: 380, y: 506 };
/** Above the field, outside it: a tap closes the keyboard. */
export const OUTSIDE = { x: 200, y: 135 };

/**
 * Long enough for the keyboard to finish sliding in after the field's tap,
 * so its keys are where [`MIC`] says. A model that loads from a local copy
 * is ready sooner: a tap before this misses the key.
 */
export const KEYBOARD_SETTLES_MS = 800;

export const TEXT_EDIT_IDX = demoIndex("TextEdit");

/** A saved state with only demo `openIdx` open, filling a `w` x `h` window. */
export function seedState(openIdx: number, w: number, h: number): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s += i === openIdx ? `d${i}=1,0,0,${w},${h},0\n` : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

/** Open the TextEdit demo alone on the next navigation. */
export async function seedTextEdit(page: Page): Promise<void> {
  const vp = page.viewportSize()!;
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(TEXT_EDIT_IDX, vp.width, vp.height));
}

/** The voice provider's state at the keyboard's last poll (Debug format). */
export async function dictationState(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return wasm["dictation_state"]() as string | null;
  });
}

/** Wait for a state matching `pattern`; throws with the last one seen. */
export async function waitForState(
  page: Page,
  pattern: RegExp,
  timeoutMs = 60_000,
): Promise<string> {
  const t0 = Date.now();
  for (;;) {
    const s = (await dictationState(page)) ?? "none";
    if (pattern.test(s)) return s;
    if (Date.now() - t0 > timeoutMs) throw new Error(`state stuck at ${s}, wanted ${pattern}`);
    await page.waitForTimeout(20);
  }
}

/** The focused field's text: select all and copy into the app's clipboard. */
export async function fieldText(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    wasm["on_key_down"]("a", false, true, false, false);
    wasm["on_key_down"]("c", false, true, false, false);
    return wasm["wasm_clipboard_get"]() as string | null;
  });
}
