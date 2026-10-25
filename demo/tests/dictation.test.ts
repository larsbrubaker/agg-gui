/**
 * End-to-end dictation test for the WASM demo: the on-screen keyboard's
 * microphone key on agg-gui-whistle, in real Chrome on a phone profile.
 *
 * Chrome's fake capture device plays a whistle-rust test clip as the
 * microphone (`--use-file-for-fake-audio-capture`), and its fake media UI
 * grants the permission. The test opens the TextEdit demo and taps its
 * multi-line TextArea: the keyboard comes up and nothing of the dictation
 * is requested. The first tap on the microphone opens it and downloads the
 * model, with progress, and says "Speak now" once the model is in and the
 * audio flows, with no second tap; the user (a Web Audio stream standing in
 * for the microphone, `speakAtCue`) says the clip at that cue; the test
 * taps again to stop and checks the words landed in the field. It reports the times from the first tap to the progress shown and
 * to listening, the download's, and from the stop to the final text.
 *
 * The other tests here: a second tap while the model downloads cancels the
 * dictation (nothing listens, the audio made in the tap is closed, and the
 * download completes so the next tap listens at once), and a failed
 * download is an error the next tap retries, after which the Cache API
 * serves the model.
 *
 * Needs the dictation worker and model in the served site
 * (`demo/build-wasm.sh` with ../whistle-rust/models/whistle.cact, or the
 * deploy's dist) and the clip, from the sibling whistle-rust checkout or
 * `DICTATION_CLIP`.
 *
 * Run:  cd demo && bunx playwright test dictation
 * (`DEMO_URL=http://localhost:3011/` points it at another server.)
 */

import { test, expect } from "@playwright/test";
import { existsSync } from "fs";
import { join } from "path";

import {
  FIELD,
  KEYBOARD_SETTLES_MS,
  MIC,
  PHONE,
  CAPTURE_MODULE,
  WHISTLE_REQUEST,
  dictationState,
  fieldText,
  modelLoad,
  recordWorkerTraffic,
  speakAtCue,
  workerTraffic,
  seedTextEdit,
  waitForState,
  watchModelLoad,
  pushedEnvelope,
} from "./dictation_common";

const CLIP =
  process.env.DICTATION_CLIP ??
  join(__dirname, "../../../whistle-rust/test-data/clips/en_samantha.wav");
const BASE = process.env.DEMO_URL ?? "/";
const SHOTS = join(__dirname, "snapshots");

test.use({
  ...PHONE,
  launchOptions: {
    args: [
      "--use-fake-ui-for-media-stream",
      "--use-fake-device-for-media-stream",
      // Played once, then silence.
      `--use-file-for-fake-audio-capture=${CLIP}%noloop`,
    ],
  },
});

// One retry here only (the config has none): the clip's words come from a
// live stream whose passes depend on when the fake device's audio starts,
// so a whistle re-read must not block a deploy. The assertions only pin
// what agg-gui-whistle owns.
test.describe.configure({ retries: 1 });

// A dictation that came out wrong says what the worker was sent and said.
test.afterEach(async ({ page }, testInfo) => {
  if (testInfo.status !== testInfo.expectedStatus) {
    console.log(`worker traffic: ${await workerTraffic(page)}`);
    console.log(`pushed audio: ${await pushedEnvelope(page, 0.5, 50)}`);
  }
});

test("a tap on the microphone dictates into the TextEdit field", async ({ page }) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  await seedTextEdit(page);
  page.on("console", (m) => console.log(`[page ${m.type()}] ${m.text()}`));
  await watchModelLoad(page);
  // A user who says the clip when the key says "Speak now".
  await speakAtCue(page, CLIP);
  await recordWorkerTraffic(page);
  // Every request of the dictation's, the worker's included.
  const requests: string[] = [];
  page.on("request", (r) => {
    if (WHISTLE_REQUEST.test(r.url()) && !CAPTURE_MODULE.test(r.url())) requests.push(r.url());
  });
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(SHOTS, "dictation-0-boot.png") });
  console.log("state at boot:", await dictationState(page));

  // Tap the multi-line field: the keyboard comes up, and the key is ready
  // to tap with nothing loaded or downloading.
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS + 1_500);
  await page.screenshot({ path: join(SHOTS, "dictation-1-keyboard-up.png") });
  expect(await dictationState(page)).toBe("Ready");
  // Only the capture module is prefetched (it is not counted here):
  // nothing of the worker or the model.
  expect(requests, "nothing of the dictation is requested before the tap").toEqual([]);

  // A phone-like link (about 2 MB/s), so the download takes long enough to
  // see its progress. Chrome applies it to the page's dedicated worker too.
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: 40,
    downloadThroughput: 2_000_000,
    uploadThroughput: 500_000,
  });

  // Time it in the page: from the tap's touchend to the first poll that
  // shows each state, and from the stop's touchend to the poll that
  // reports Ready (the final text lands no later than that poll).
  await page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    win.__tapAt = 0;
    win.__preparingAt = 0;
    win.__progressAt = 0;
    win.__listenAt = 0;
    win.__stopAt = 0;
    win.__readyAt = 0;
    win.__armed = "tap";
    document.addEventListener(
      "touchend",
      () => {
        const now = performance.now();
        if (win.__armed === "tap" && !win.__tapAt) win.__tapAt = now;
        if (win.__armed === "stop" && !win.__stopAt) win.__stopAt = now;
      },
      { capture: true },
    );
    const poll = () => {
      const s = win.__wasm["dictation_state"]() as string;
      const now = performance.now();
      if (!win.__tapAt) return;
      if (!win.__preparingAt && s.startsWith("Preparing")) win.__preparingAt = now;
      if (!win.__progressAt && s.includes("progress: Some")) win.__progressAt = now;
      if (!win.__listenAt && s.startsWith("Listening")) win.__listenAt = now;
      if (win.__stopAt && !win.__readyAt && s === "Ready") win.__readyAt = now;
    };
    setInterval(poll, 2);
  });

  // The first tap on the microphone opens it at once and downloads the
  // model, with progress, then listens without another tap; the user says
  // the clip at that cue. (Speech during the download is
  // dictation_permission.test.ts's.)
  await page.touchscreen.tap(MIC.x, MIC.y);
  const states: string[] = [];
  const t0 = Date.now();
  let shotProgress = false;
  for (;;) {
    const s = ((await dictationState(page)) ?? "none").replace(/level: Some\([^)]*\)/, "level: ..");
    if (states[states.length - 1] !== s) states.push(s);
    if (!shotProgress && s.includes("progress: Some")) {
      shotProgress = true;
      await page.screenshot({ path: join(SHOTS, "dictation-2-preparing.png") });
    }
    if (s.startsWith("Listening") || s.startsWith("Error") || Date.now() - t0 > 90_000) break;
    await page.waitForTimeout(20);
  }
  await cdp.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: 0,
    downloadThroughput: -1,
    uploadThroughput: -1,
  });
  console.log("states after the tap:", states.join(" | "));
  expect(states[states.length - 1]).toMatch(/^Listening/);
  expect(states.some((s) => s.includes("progress: Some")), "the download shows progress").toBe(
    true,
  );
  const model = requests.find((url) => url.includes("whistle.cact"));
  expect(model, "the tap downloads the model").toBeTruthy();
  const load = await modelLoad(page);
  const times = await page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    return {
      preparing: win.__preparingAt - win.__tapAt,
      progress: win.__progressAt - win.__tapAt,
      listening: win.__listenAt - win.__tapAt,
    };
  });
  console.log(
    `tap to Preparing ${times.preparing.toFixed(1)} ms, to progress shown ` +
      `${times.progress.toFixed(1)} ms, to Listening ${times.listening.toFixed(1)} ms; ` +
      `download (first to last progress) ${(load.lastProgress - load.firstProgress).toFixed(0)} ms, ` +
      `model loaded ${(load.loaded - load.lastProgress).toFixed(0)} ms after it (${model})`,
  );

  // Mid-clip: the pending tail shows as composition text.
  await page.waitForTimeout(2_500);
  await page.screenshot({ path: join(SHOTS, "dictation-3-listening.png") });
  // The clip is 3.4 s. By default the stream also hears a pause after it
  // (its endpoint); STOP_AFTER_MS stops sooner, mid-tail.
  await page.waitForTimeout(Number(process.env.STOP_AFTER_MS ?? 5_000) - 2_500);
  await page.screenshot({ path: join(SHOTS, "dictation-4-before-stop.png") });

  await page.evaluate(() => {
    (window as unknown as Record<string, any>).__armed = "stop";
  });
  await page.touchscreen.tap(MIC.x, MIC.y);
  await page.waitForFunction(
    () => (window as unknown as Record<string, any>).__readyAt > 0,
    undefined,
    { timeout: 30_000 },
  );
  const stopMs = await page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    return win.__readyAt - win.__stopAt;
  });
  console.log(`tap-stop to final text: ${stopMs.toFixed(1)} ms`);
  await page.waitForTimeout(300);
  await page.screenshot({ path: join(SHOTS, "dictation-5-done.png") });

  // Read the field back: select all and copy into the app's clipboard.
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  // The words land after the field's text, spaced by the keyboard. The
  // editor is a multi-line TextArea, which auto-capitalises, but the caret
  // follows "Edit this text" (no `.`, `!`, `?` or line break), so the
  // keyboard's VoiceContext has sentence_start false and the provider
  // lowercases the first word ("make", which whistle sometimes hears as
  // "mate"). The rest is the engine's: on live audio whistle's stream reads
  // the clip differently from run to run ("cube 20", "cute 20", "Q20"), and
  // has been seen to commit "mm" and later "millimeters" for the same audio
  // (agg-gui-whistle's own tests check the provider passes on exactly the
  // text whistle commits). So this pins the lowercase first word and the
  // clip's beginning and end, not whistle's reading of its middle.
  // ("round the top edges" has also come back "round to top edges" and
  // "rounded top edges".)
  expect(text ?? "").toMatch(/^Edit this text ma[kt]e the .*20.*wide and round(?:ed| \w+) top edges\.$/);
});

test("a second tap while the model downloads cancels the dictation; the download completes", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await seedTextEdit(page);
  // Every AudioContext the page makes, and the ones it closed.
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
  await watchModelLoad(page);
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  // Slow enough that the second tap lands mid-download.
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: 40,
    downloadThroughput: 2_000_000,
    uploadThroughput: 500_000,
  });

  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /progress: Some/, 30_000);
  const made = await page.evaluate(
    () => (window as unknown as Record<string, any>).__contexts.made as number,
  );
  expect(made, "the tap made the microphone's audio context").toBe(1);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 5_000);
  expect((await modelLoad(page)).loaded, "still downloading at the cancel").toBe(0);
  const contexts = await page.evaluate(
    () => (window as unknown as Record<string, any>).__contexts,
  );
  expect(contexts, "the cancel closed it").toEqual({ made: 1, closed: 1 });

  // The download carries on and the model loads; nothing listens meanwhile.
  const seen: string[] = [];
  const t0 = Date.now();
  while (!(await modelLoad(page)).loaded && Date.now() - t0 < 60_000) {
    const s = (await dictationState(page)) ?? "none";
    if (seen[seen.length - 1] !== s) seen.push(s);
    await page.waitForTimeout(50);
  }
  console.log(`model loaded ${Date.now() - t0} ms after the cancel`);
  await page.waitForTimeout(2_000);
  seen.push((await dictationState(page)) ?? "none");
  console.log("states after the cancel:", seen.join(" | "));
  expect((await modelLoad(page)).loaded, "the download completed").toBeGreaterThan(0);
  expect(seen.every((s) => s === "Ready"), "nothing listens").toBe(true);

  // The model is in: the next tap listens at once.
  await cdp.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: 0,
    downloadThroughput: -1,
    uploadThroughput: -1,
  });
  const t1 = Date.now();
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/, 10_000);
  console.log(`listening ${Date.now() - t1} ms after the next tap`);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
});

test("a failed download is an error the next tap retries, then the cache serves", async ({
  page,
  context,
}) => {
  test.setTimeout(120_000);
  await seedTextEdit(page);
  // Every copy of the model fails to download (the worker's fetches too).
  let blocked = true;
  await context.route(/whistle\.cact/, (route) =>
    blocked ? route.abort("internetdisconnected") : route.continue(),
  );
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);

  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  await page.touchscreen.tap(MIC.x, MIC.y);
  const failed = await waitForState(page, /^Error/);
  expect(failed).toContain("Couldn't download the speech model");
  expect(failed).toContain("retry: true");
  await page.screenshot({ path: join(SHOTS, "dictation-6-download-failed.png") });

  // Let downloads through: the next tap downloads again and listens.
  blocked = false;
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);

  // A reload loads the checked copy from the Cache API: no download.
  let fetched = 0;
  await context.route(/whistle\.cact/, (route) => {
    fetched++;
    return route.continue();
  });
  await page.reload();
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
  const t0 = Date.now();
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/);
  console.log(`listening from the cache ${Date.now() - t0} ms after the tap`);
  expect(fetched, "the model comes from the cache").toBe(0);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
});
