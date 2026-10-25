/**
 * The microphone of a dictation that waits for the model, in real Chrome
 * on a phone profile with a whistle-rust clip as the microphone (like
 * dictation.test.ts), on a slow link so the download takes seconds.
 *
 * - The first tap makes the dictation's one `getUserMedia` request inside
 *   the tap's own dispatch (a user gesture; the permission prompt shows
 *   then, and iOS can't refuse it for want of a gesture), and that
 *   stream is the one the dictation uses: it is live through the download,
 *   no second request follows, and speech from the tap on (the fake
 *   device's clip starts when the microphone opens) ends up in the field
 *   once the model is in.
 * - A second tap during the download cancels the dictation: its tracks
 *   stop, its audio context closes, nothing is asked again and nothing is
 *   dictated, though the download completes.
 *
 * Needs the dictation worker and model in the served site and the clip, as
 * dictation.test.ts does.
 *
 * Run:  cd demo && bunx playwright test dictation_permission
 */

import { test, expect, type Page } from "@playwright/test";
import { existsSync } from "fs";
import { join } from "path";

import {
  FIELD,
  KEYBOARD_SETTLES_MS,
  MIC,
  PHONE,
  dictationState,
  fieldText,
  liveTracks,
  micRequests,
  modelLoad,
  recordMicRequests,
  seedTextEdit,
  waitForState,
  watchModelLoad,
  recordWorkerTraffic,
  workerTraffic,
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
      // Played once from when the microphone opens (at the tap), then
      // silence.
      `--use-file-for-fake-audio-capture=${CLIP}%noloop`,
    ],
  },
});

/** The clip's words after the field's own text (see dictation_audio.test.ts). */
const DICTATED = /^Edit this text \w+ the .+ wide and round \w+ top edges\.$/;

/**
 * Open the TextEdit demo, bring the keyboard up, slow the network to about
 * 2 MB/s, and tap the microphone, timing the tap's touchend in the page
 * (`__tapAt`) and counting the audio contexts it makes and closes
 * (`__contexts`).
 */
async function tapOnASlowLink(page: Page): Promise<number> {
  await recordMicRequests(page);
  await recordWorkerTraffic(page);
  await watchModelLoad(page);
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__contexts = { made: 0, closed: 0 };
    const Real = window.AudioContext;
    win.AudioContext = class extends Real {
      constructor(options?: AudioContextOptions) {
        super(options);
        win.__contexts.made++;
      }
      close(): Promise<void> {
        win.__contexts.closed++;
        return super.close();
      }
    };
  });
  await seedTextEdit(page);
  page.on("console", (m) => console.log(`[page ${m.type()}] ${m.text()}`));
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  expect(await micRequests(page), "nothing asked before the tap").toEqual([]);

  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: 40,
    downloadThroughput: 2_000_000,
    uploadThroughput: 500_000,
  });
  await page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    win.__tapAt = 0;
    document.addEventListener(
      "touchend",
      () => {
        if (!win.__tapAt) win.__tapAt = performance.now();
      },
      { capture: true },
    );
  });
  await page.touchscreen.tap(MIC.x, MIC.y);
  return page.evaluate(() => (window as unknown as Record<string, any>).__tapAt as number);
}

async function contexts(page: Page): Promise<{ made: number; closed: number }> {
  return page.evaluate(() => (window as unknown as Record<string, any>).__contexts);
}

// A dictation that came out wrong (once, under heavy machine load, nothing
// at all with a normal request-to-stream gap) says what the worker was sent
// and said.
test.afterEach(async ({ page }, testInfo) => {
  if (testInfo.status !== testInfo.expectedStatus) {
    console.log(`worker traffic: ${await workerTraffic(page)}`);
  }
});

test.beforeEach(() => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(120_000);
});

test("the first tap opens the dictation's one microphone at once, and speech during the download is dictated", async ({
  page,
}) => {
  const tapAt = await tapOnASlowLink(page);
  // While the model downloads: one request, made in the tap and granted,
  // its track live (capturing), the key showing the download.
  const states: string[] = [];
  let liveSamples = 0;
  let deadSamples = 0;
  const t0 = Date.now();
  while (!(await modelLoad(page)).loaded && Date.now() - t0 < 60_000) {
    const s = ((await dictationState(page)) ?? "none").replace(/Some\([^)]*\)/, "Some(..)");
    if (states[states.length - 1] !== s) states.push(s);
    if (s.includes("progress: Some")) {
      if ((await liveTracks(page)) > 0) liveSamples++;
      else deadSamples++;
    }
    await page.waitForTimeout(50);
  }
  const load = await modelLoad(page);
  const asked = await micRequests(page);
  console.log("states while loading:", states.join(" | "));
  console.log(
    `asked ${(asked[0]?.at - tapAt).toFixed(1)} ms after the tap; download progress ` +
      `${(load.firstProgress - tapAt).toFixed(0)}..${(load.lastProgress - tapAt).toFixed(0)} ms; ` +
      `model in ${(load.loaded - tapAt).toFixed(0)} ms; track live in ${liveSamples} of ` +
      `${liveSamples + deadSamples} samples`,
  );
  expect(asked.length, "one request").toBe(1);
  expect(asked[0].inTap, "made inside the tap's touchend dispatch").toBe(true);
  expect(asked[0].granted).toBe(true);
  expect(states.some((s) => s.includes("progress: Some")), "the key shows the download").toBe(true);
  expect(states.some((s) => s.startsWith("Listening")), "not listening before the model").toBe(
    false,
  );
  expect(deadSamples, "the dictation's track stays live through the download").toBe(0);
  expect(liveSamples).toBeGreaterThan(0);

  // The clip (3.4 s) played during the download; once the model is in, it
  // is recognised and the key listens, with no second tap.
  await waitForState(page, /^Listening/, 30_000);
  await page.waitForTimeout(4_000);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
  await page.waitForTimeout(300);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  expect(text ?? "").toMatch(DICTATED);
  expect((await micRequests(page)).length, "still the one request").toBe(1);
  expect(await liveTracks(page), "released at the stop").toBe(0);
});

test("a second tap during the download stops the tracks, asks nothing more and dictates nothing", async ({
  page,
}) => {
  await tapOnASlowLink(page);
  await waitForState(page, /progress: Some/, 30_000);
  await page.waitForFunction(() =>
    ((window as unknown as Record<string, any>).__tracks as MediaStreamTrack[]).some(
      (t) => t.readyState === "live",
    ),
  );
  await page.waitForTimeout(1_000);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 5_000);
  expect((await modelLoad(page)).loaded, "still downloading at the cancel").toBe(0);
  expect(await liveTracks(page), "the cancel stopped the tracks").toBe(0);
  expect(await contexts(page), "and closed the audio").toEqual({ made: 1, closed: 1 });

  // The download completes; nothing listens, nothing is asked or dictated.
  const t0 = Date.now();
  while (!(await modelLoad(page)).loaded && Date.now() - t0 < 60_000) {
    expect((await dictationState(page)) ?? "none").toBe("Ready");
    await page.waitForTimeout(100);
  }
  await page.waitForTimeout(2_000);
  expect(await dictationState(page)).toBe("Ready");
  expect((await micRequests(page)).length, "no second request").toBe(1);
  expect(await fieldText(page)).toBe("Edit this text");
});
