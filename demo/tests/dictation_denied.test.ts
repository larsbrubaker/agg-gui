/**
 * Dictation with the microphone permission denied (a file of its own:
 * Chrome's launch flags differ from dictation.test.ts, which grants it).
 * The tap on the microphone key asks for the microphone at once (the model
 * loads meanwhile) and must report the denial, never listening. The error
 * is one a tap retries: the next tap asks the browser again, and, still
 * refused, the message shows again.
 *
 * Needs the dictation worker and model in the served site, as
 * dictation.test.ts does.
 *
 * Run:  cd demo && bunx playwright test dictation_denied
 */

import { test, expect } from "@playwright/test";
import { join } from "path";

import {
  FIELD,
  KEYBOARD_SETTLES_MS,
  MIC,
  PHONE,
  dictationState,
  micRequests,
  recordMicRequests,
  seedTextEdit,
  waitForState,
} from "./dictation_common";

const BASE = process.env.DEMO_URL ?? "/";
const SHOTS = join(__dirname, "snapshots");

test.use({
  ...PHONE,
  launchOptions: {
    args: ["--use-fake-device-for-media-stream", "--deny-permission-prompts"],
  },
});

test("a tap reports the denied microphone, and the next tap asks again", async ({ page }) => {
  test.setTimeout(120_000);
  await recordMicRequests(page);
  await seedTextEdit(page);
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  // The text field brings the keyboard up; nothing loads until the tap.
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  // The tap asks for the microphone; the model loads meanwhile.
  await page.touchscreen.tap(MIC.x, MIC.y);
  const denied = await waitForState(page, /^Error/, 60_000);
  expect(denied).toContain("Microphone access was denied");
  expect(denied).toContain("retry: true");
  await page.screenshot({ path: join(SHOTS, "dictation-7-denied.png") });
  const first = await micRequests(page);
  expect(first.map((r) => r.granted)).toEqual([false]);

  // The next tap asks the browser again, which still refuses: the message
  // shows again, and nothing listens.
  await page.waitForTimeout(500);
  await page.touchscreen.tap(MIC.x, MIC.y);
  const seen: string[] = [];
  const t0 = Date.now();
  while (Date.now() - t0 < 30_000) {
    const s = (await dictationState(page)) ?? "none";
    if (seen[seen.length - 1] !== s) seen.push(s);
    const asked = await micRequests(page);
    if (asked.length === 2 && asked[1].granted === false && s.startsWith("Error")) break;
    await page.waitForTimeout(20);
  }
  console.log("states after the second tap:", seen.join(" | "));
  expect((await micRequests(page)).map((r) => r.granted)).toEqual([false, false]);
  expect(seen.some((s) => s.startsWith("Listening")), "never listens").toBe(false);
  const again = await waitForState(page, /^Error/, 5_000);
  expect(again).toContain("Microphone access was denied");
});
