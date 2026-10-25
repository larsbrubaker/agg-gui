/**
 * A microphone lost mid-dictation (unplugged, switched off, its permission
 * revoked: the browser ends its track, which fires `ended`): the dictation
 * finishes what it heard, so the words said until then land in the field,
 * and the key then says the microphone was disconnected, an error a tap
 * retries. The user is the speak-at-cue Web Audio stream
 * (`dictation_common.ts`), whose track the test ends once some words are
 * in.
 *
 * Needs the dictation worker and model in the served site, as
 * dictation.test.ts does.
 *
 * Run:  cd demo && bunx playwright test dictation_lost
 */

import { test, expect } from "@playwright/test";
import { existsSync } from "fs";
import { join } from "path";

import {
  FIELD,
  KEYBOARD_SETTLES_MS,
  MIC,
  PHONE,
  fieldText,
  seedTextEdit,
  speakAtCue,
  waitForState,
} from "./dictation_common";

const CLIP =
  process.env.DICTATION_CLIP ??
  join(__dirname, "../../../whistle-rust/test-data/clips/en_samantha.wav");
const BASE = process.env.DEMO_URL ?? "/";

test.use({
  ...PHONE,
  launchOptions: { args: ["--use-fake-ui-for-media-stream"] },
});

// The words come from a live stream (see dictation.test.ts): one retry.
test.describe.configure({ retries: 1 });

test("a microphone lost mid-dictation keeps the words heard and says it was disconnected", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  await seedTextEdit(page);
  await speakAtCue(page, CLIP);
  // Keep the streams the speaking user handed over, to end their tracks.
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__streams = [];
    const gum = MediaDevices.prototype.getUserMedia;
    MediaDevices.prototype.getUserMedia = async function (this: MediaDevices, c) {
      const stream = await gum.call(this, c);
      win.__streams.push(stream);
      return stream;
    };
  });
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  const before = (await fieldText(page)) ?? "";
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);

  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/, 120_000);
  // The clip runs 3.2 s from the cue: end the microphone once all of it
  // has played (the words are in the pipeline, maybe not yet recognised).
  await page.waitForFunction(
    () => {
      const cue = (window as unknown as Record<string, any>).__cue;
      return cue.startedAt > 0 && performance.now() - cue.startedAt > 3_600;
    },
    undefined,
    { timeout: 30_000 },
  );
  const ended = await page.evaluate(() => {
    const streams = (window as unknown as Record<string, any>).__streams as MediaStream[];
    let n = 0;
    for (const s of streams)
      for (const t of s.getTracks()) {
        t.dispatchEvent(new Event("ended"));
        n++;
      }
    return n;
  });
  expect(ended, "the dictation's track").toBeGreaterThan(0);

  // No stop tap: the dictation ends by itself, words first, then the error.
  const error = await waitForState(page, /^Error/, 60_000);
  expect(error).toContain("The microphone was disconnected");
  expect(error).toContain("retry: true");
  const text = (await fieldText(page)) ?? "";
  console.log(`field before: ${JSON.stringify(before)}, after: ${JSON.stringify(text)}`);
  expect(text.length, "the words heard landed in the field").toBeGreaterThan(before.length);
  // The clip's last words: the audio up to the microphone's end was kept
  // (which word whistle makes of "cube" is not this test's business).
  expect(text.toLowerCase()).toContain("top edges");
});
