/**
 * The microphone of a dictation that waits for the model, in real Chrome
 * on a phone profile with a whistle-rust clip as the microphone (like
 * dictation.test.ts), on a slow link so the download takes seconds.
 *
 * - The first tap makes the dictation's one `getUserMedia` request inside
 *   the tap's own dispatch (a user gesture; the permission prompt shows
 *   then, and iOS can't refuse it for want of a gesture), and that
 *   stream is the one the dictation uses: it is live through the download,
 *   no second request follows, and the key says Listening only once the
 *   model is in. (Chrome's fake device, real here.)
 * - Speech from the moment the microphone is live, during the download,
 *   ends up in the field once the model is in. The speaker
 *   (`speakAtCue`, "wired") starts when the app wires the stream: Chrome's
 *   fake device starts its clip when the device starts, and a starved
 *   renderer (CI's) hands the page that stream seconds later, so its clip
 *   was mostly over before any page could hear it ("FADGES." on CI).
 *
 * What the key says while the model loads is sampled in the page itself
 * (`watchTheLoad`), the state and whether the model is in read together:
 * a test reading them one after the other can see the load end in between
 * and take the Listening that follows for one before it.
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
  speakAtCue,
  workerTraffic,
  pushedEnvelope,
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
const DICTATED = /^Edit this text \w+ the .+ wide and round(?:ed| \w+) top edges\.$/;

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
  await watchTheLoad(page);
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

/**
 * Until the model is in, sample in the page every 5 ms what the key says
 * (`__watch.states`), whether it ever said Listening (`early`), and, once a
 * microphone track exists, whether it was live (`live` / `dead` samples
 * taken while the key showed the download). The test's listener on the
 * worker sees "loaded" before the app does, so a Listening sampled while
 * the model is not yet in is one the app reported before it.
 */
async function watchTheLoad(page: Page): Promise<void> {
  await page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    const w = (win.__watch = { states: [] as string[], early: false, live: 0, dead: 0 });
    const poll = setInterval(() => {
      if (win.__load.loaded) {
        clearInterval(poll);
        return;
      }
      const s = String(win.__wasm["dictation_state"]() ?? "none");
      const shown = s.replace(/Some\([^)]*\)/, "Some(..)");
      if (w.states[w.states.length - 1] !== shown) w.states.push(shown);
      if (s.startsWith("Listening")) w.early = true;
      const tracks = win.__tracks as MediaStreamTrack[];
      if (s.includes("progress: Some") && tracks.length > 0) {
        if (tracks.some((t) => t.readyState === "live")) w.live++;
        else w.dead++;
      }
    }, 5);
  });
}

async function theLoad(
  page: Page,
): Promise<{ states: string[]; early: boolean; live: number; dead: number }> {
  return page.evaluate(() => (window as unknown as Record<string, any>).__watch);
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
    console.log(`pushed audio: ${await pushedEnvelope(page, 0.5, 50)}`);
  }
});

test.beforeEach(() => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(120_000);
});

/** Wait for the model, then what was sampled meanwhile. */
async function afterTheLoad(page: Page, tapAt: number) {
  await page.waitForFunction(
    () => (window as unknown as Record<string, any>).__load.loaded > 0,
    undefined,
    { timeout: 60_000 },
  );
  const watched = await theLoad(page);
  const load = await modelLoad(page);
  const asked = await micRequests(page);
  console.log("states while loading:", watched.states.join(" | "));
  console.log(
    `asked ${(asked[0]?.at - tapAt).toFixed(1)} ms after the tap; download progress ` +
      `${(load.firstProgress - tapAt).toFixed(0)}..${(load.lastProgress - tapAt).toFixed(0)} ms; ` +
      `model in ${(load.loaded - tapAt).toFixed(0)} ms; track live in ${watched.live} of ` +
      `${watched.live + watched.dead} samples`,
  );
  return { watched, load, asked };
}

test("the first tap opens the dictation's one microphone at once, live through the download", async ({
  page,
}) => {
  const tapAt = await tapOnASlowLink(page);
  // While the model downloads: one request, made in the tap and granted,
  // its track live (capturing), the key showing the download, never
  // Listening.
  const { watched, asked } = await afterTheLoad(page, tapAt);
  expect(asked.length, "one request").toBe(1);
  expect(asked[0].inTap, "made inside the tap's touchend dispatch").toBe(true);
  expect(asked[0].granted).toBe(true);
  expect(
    watched.states.some((s) => s.includes("progress: Some")),
    "the key shows the download",
  ).toBe(true);
  expect(watched.early, "not listening before the model").toBe(false);
  expect(watched.dead, "the dictation's track stays live through the download").toBe(0);
  expect(watched.live).toBeGreaterThan(0);

  // Once the model is in, the key listens with no second tap.
  await waitForState(page, /^Listening/, 30_000);
  await page.waitForTimeout(1_000);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
  expect((await micRequests(page)).length, "still the one request").toBe(1);
  expect(await liveTracks(page), "released at the stop").toBe(0);
});

test("speech from the moment the microphone is live, during the download, is dictated", async ({
  page,
}) => {
  await speakAtCue(page, CLIP, "wired");
  const tapAt = await tapOnASlowLink(page);
  const { watched, load } = await afterTheLoad(page, tapAt);
  const spoke = await page.evaluate(
    () => (window as unknown as Record<string, any>).__cue.startedAt as number,
  );
  console.log(`spoke ${(spoke - tapAt).toFixed(0)} ms after the tap`);
  expect(spoke, "spoke while the model downloaded").toBeGreaterThan(0);
  expect(spoke).toBeLessThan(load.lastProgress);
  expect(watched.early, "not listening before the model").toBe(false);

  // Once the model is in, what was said meanwhile is recognised.
  await waitForState(page, /^Listening/, 30_000);
  await page.waitForTimeout(4_000);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
  await page.waitForTimeout(300);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  expect(text ?? "").toMatch(DICTATED);
  expect((await micRequests(page)).length, "one request").toBe(1);
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
