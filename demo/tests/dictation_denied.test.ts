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

import { test, expect } from "@playwright/test";
import { join } from "path";

import {
  FIELD,
  KEYBOARD_SETTLES_MS,
  MIC,
  PHONE,
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

test("a tap reports the denied microphone without a retry", async ({ page }) => {
  test.setTimeout(120_000);
  await seedTextEdit(page);
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  // The text field brings the keyboard up; the model loads.
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  await page.touchscreen.tap(MIC.x, MIC.y);
  const denied = await waitForState(page, /^Error/, 10_000);
  expect(denied).toContain("Microphone access was denied");
  expect(denied).toContain("retry: false");
  await page.screenshot({ path: join(SHOTS, "dictation-7-denied.png") });
});
