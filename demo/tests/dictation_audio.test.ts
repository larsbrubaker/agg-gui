/**
 * Dictation's audio and download edge cases in the WASM demo, in real
 * Chrome on a phone profile with a whistle-rust clip as the microphone
 * (like dictation.test.ts):
 *
 * - iOS Safari only lets an AudioContext start inside a user gesture. The
 *   page's AudioContext is wrapped so that it does the same (a context made
 *   or resumed outside a tap's touchend/pointerup stays suspended), and a
 *   start the tap defers (the model still downloading after a failed try)
 *   must still hear the microphone; a context that never starts must be a
 *   microphone error, not Listening to silence.
 * - Without an AudioWorklet (its module fails to load) the provider falls
 *   back to a ScriptProcessor, says so once in the console, and dictates.
 * - A model download that stalls times out and is an error the next show
 *   of the keyboard retries.
 *
 * Needs the dictation worker and model in the served site and the clip, as
 * dictation.test.ts does.
 *
 * Run:  cd demo && bunx playwright test dictation_audio
 */

import { test, expect, type Page } from "@playwright/test";
import { existsSync } from "fs";
import { join } from "path";

import {
  FIELD,
  KEYBOARD_SETTLES_MS,
  MIC,
  OUTSIDE,
  PHONE,
  fieldText,
  seedTextEdit,
  waitForState,
} from "./dictation_common";

const CLIP =
  process.env.DICTATION_CLIP ??
  join(__dirname, "../../../whistle-rust/test-data/clips/en_samantha.wav");
const BASE = process.env.DEMO_URL ?? "/";

test.use({
  ...PHONE,
  launchOptions: {
    args: [
      "--use-fake-ui-for-media-stream",
      "--use-fake-device-for-media-stream",
      // Played once from when the microphone opens, then silence.
      `--use-file-for-fake-audio-capture=${CLIP}%noloop`,
    ],
  },
});

/**
 * The clip's words from its first to its last, after the field's own text:
 * these tests check the audio arrives whole. The middle is whistle's live
 * reading, which varies from run to run ("make" or "mate", "cube 20",
 * "cute 20" or "Q20"; see dictation.test.ts).
 */
const DICTATED = /^Edit this text \w+ the .+ wide and round the top edges\.$/;


/**
 * Make the page's AudioContext behave as iOS Safari's: one created or
 * resumed outside a tap's touchend / pointerup dispatch stays suspended.
 * With `never`, no context ever starts.
 */
async function emulateIosAudio(page: Page, never: boolean): Promise<void> {
  await page.addInitScript((never: boolean) => {
    let inGesture = false;
    for (const type of ["touchend", "pointerup", "mouseup", "click", "keydown"]) {
      window.addEventListener(type, () => (inGesture = true), { capture: true });
      // The window's bubbling listener runs after the canvas's handler.
      window.addEventListener(type, () => (inGesture = false));
    }
    const Real = window.AudioContext;
    class GestureAudioContext extends Real {
      private unlocked: boolean;
      constructor(options?: AudioContextOptions) {
        super(options);
        this.unlocked = inGesture && !never;
        if (!this.unlocked) void super.suspend();
      }
      resume(): Promise<void> {
        if (inGesture && !never) this.unlocked = true;
        // Outside a gesture iOS leaves a locked context suspended.
        return this.unlocked ? super.resume() : Promise.resolve();
      }
    }
    (window as unknown as Record<string, unknown>).AudioContext = GestureAudioContext;
  }, never);
}

async function boot(page: Page): Promise<void> {
  page.on("console", (m) => console.log(`[page ${m.type()}] ${m.text()}`));
  await seedTextEdit(page);
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
}

/** Listen to the whole clip, stop, and wait for the final text. */
async function dictateClip(page: Page): Promise<void> {
  await waitForState(page, /^Listening/, 60_000);
  await page.waitForTimeout(5_000);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
  await page.waitForTimeout(300);
}

test("a tap that retries a failed download starts the audio inside the tap and dictates once loaded", async ({
  page,
  context,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  await emulateIosAudio(page, false);
  let blocked = true;
  await context.route(/whistle\.cact/, (route) =>
    blocked ? route.abort("internetdisconnected") : route.continue(),
  );
  await boot(page);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  const failed = await waitForState(page, /^Error/);
  expect(failed).toContain("retry: true");
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);

  // The tap retries the download; the microphone opens when the model is
  // in, long after the tap, from the worker's reply.
  blocked = false;
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  expect(text ?? "").toMatch(DICTATED);
});

test("an AudioContext that never starts is a microphone error, not listening to silence", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await emulateIosAudio(page, true);
  await boot(page);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  await page.touchscreen.tap(MIC.x, MIC.y);
  const seen: string[] = [];
  const t0 = Date.now();
  for (;;) {
    const s = (await page.evaluate(() =>
      (window as unknown as Record<string, any>).__wasm["dictation_state"](),
    )) as string;
    if (seen[seen.length - 1] !== s) seen.push(s);
    if (s.startsWith("Error") || Date.now() - t0 > 10_000) break;
    await page.waitForTimeout(20);
  }
  console.log("states:", seen.join(" | "));
  expect(seen.some((s) => s.startsWith("Listening")), "never listens").toBe(false);
  expect(seen[seen.length - 1]).toMatch(/^Error .*retry: true/);
});

test("without an AudioWorklet the ScriptProcessor fallback dictates and says so once", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  // The worklet's module never loads (a request route doesn't see a
  // worklet's fetch, so the failure is made here); count the fallback's
  // processors.
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__processors = 0;
    AudioWorklet.prototype.addModule = () => Promise.reject(new Error("blocked by the test"));
    const create = BaseAudioContext.prototype.createScriptProcessor;
    BaseAudioContext.prototype.createScriptProcessor = function (...args) {
      win.__processors++;
      return create.apply(this, args);
    };
  });
  const warnings: string[] = [];
  page.on("console", (m) => {
    // Ours, not Chrome's own deprecation notice for ScriptProcessorNode.
    if (m.text().startsWith("agg-gui-whistle: no AudioWorklet")) warnings.push(m.text());
  });
  await boot(page);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  expect(text ?? "").toMatch(DICTATED);
  // A second dictation (the clip is over: silence) falls back again,
  // without another message.
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/, 30_000);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
  expect(warnings.length, warnings.join("\n")).toBe(1);
  const processors = await page.evaluate(
    () => (window as unknown as Record<string, any>).__processors as number,
  );
  expect(processors, "both dictations captured with a ScriptProcessor").toBe(2);
});

test("a download that stalls times out and the next show of the keyboard retries it", async ({
  page,
  context,
}) => {
  test.setTimeout(120_000);
  // The page's own copy answers nothing at all; the others fail at once.
  let stalled = true;
  await context.route(/whistle\.cact/, (route) => {
    if (!stalled) return route.continue();
    if (route.request().url().includes("localhost")) return; // never answered
    return route.abort("internetdisconnected");
  });
  await boot(page);
  const t0 = Date.now();
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  const failed = await waitForState(page, /^Error/, 60_000);
  console.log(`stalled download failed after ${Date.now() - t0} ms: ${failed}`);
  expect(failed).toContain("Couldn't download the speech model");
  expect(failed).toContain("retry: true");

  stalled = false;
  await page.touchscreen.tap(OUTSIDE.x, OUTSIDE.y);
  await page.waitForTimeout(600);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
});
