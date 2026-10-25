/**
 * End-to-end dictation test for the WASM demo: the on-screen keyboard's
 * microphone key on agg-gui-whistle, in real Chrome on a phone profile.
 *
 * Chrome's fake capture device plays a whistle-rust test clip as the
 * microphone (`--use-file-for-fake-audio-capture`), and its fake media UI
 * grants the permission. The test opens the TextEdit demo, taps the
 * single-line field (the keyboard comes up and the model downloads, with
 * progress), taps the microphone, waits out the clip, taps it again, and
 * checks the words landed in the field. It reports the time from that
 * second tap to the final text.
 *
 * Needs the dictation worker and model in the served site
 * (`demo/build-wasm.sh` with ../whistle-rust/models/whistle.cact, or the
 * deploy's dist) and the clip, from the sibling whistle-rust checkout or
 * `DICTATION_CLIP`.
 *
 * Run:  cd demo && bunx playwright test dictation
 * (`DEMO_URL=http://localhost:3011/` points it at another server.)
 */

import { test, expect, devices, type Page } from "@playwright/test";
import { existsSync } from "fs";
import { join } from "path";

const CLIP =
  process.env.DICTATION_CLIP ??
  join(__dirname, "../../../whistle-rust/test-data/clips/en_samantha.wav");
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

async function dictationState(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return wasm["dictation_state"]() as string | null;
  });
}

test("a tap on the microphone dictates into the TextEdit field", async ({ page }) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  const vp = page.viewportSize()!;
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(TEXT_EDIT_IDX, vp.width, vp.height));
  page.on("console", (m) => console.log(`[page ${m.type()}] ${m.text()}`));
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(SHOTS, "dictation-0-boot.png") });
  console.log("state at boot:", await dictationState(page));

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

  // Tap the multi-line field: the keyboard comes up and the model loads.
  await page.touchscreen.tap(250, 340);
  const states: string[] = [];
  const t0 = Date.now();
  let shotProgress = false;
  for (;;) {
    const s = (await dictationState(page)) ?? "none";
    if (states[states.length - 1] !== s) states.push(s);
    if (!shotProgress && s.includes("progress: Some")) {
      shotProgress = true;
      await page.screenshot({ path: join(SHOTS, "dictation-1-preparing.png") });
    }
    if (s === "Ready" || s.startsWith("Error") || Date.now() - t0 > 60_000) break;
    await page.waitForTimeout(20);
  }
  console.log(`model ready after ${Date.now() - t0} ms; states:`, states.join(" | "));
  await page.screenshot({ path: join(SHOTS, "dictation-2-ready.png") });
  expect(states).toContain("Ready");

  // Time the stop in the page: from the stop tap's touchend to the poll
  // that reports Ready (the final text lands no later than that poll).
  await page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    win.__stopAt = 0;
    win.__readyAt = 0;
    win.__armed = false;
    document.addEventListener(
      "touchend",
      () => {
        if (win.__armed && !win.__stopAt) win.__stopAt = performance.now();
      },
      { capture: true },
    );
    const poll = () => {
      if (win.__stopAt && !win.__readyAt && win.__wasm["dictation_state"]() === "Ready") {
        win.__readyAt = performance.now();
      }
    };
    setInterval(poll, 2);
  });

  // Tap the microphone: the fake device plays the clip from now.
  const MIC = { x: 380, y: 506 };
  await page.touchscreen.tap(MIC.x, MIC.y);
  const tListen = Date.now();
  const listening: string[] = [];
  while (Date.now() - tListen < 10_000) {
    const s = (await dictationState(page)) ?? "none";
    if (listening[listening.length - 1] !== s.replace(/level: Some\([^)]*\)/, "level: ..")) {
      listening.push(s.replace(/level: Some\([^)]*\)/, "level: .."));
    }
    if (s.startsWith("Listening")) break;
    await page.waitForTimeout(20);
  }
  console.log(`listening after ${Date.now() - tListen} ms; states:`, listening.join(" | "));
  expect(listening[listening.length - 1]).toMatch(/^Listening/);

  // Mid-clip: the pending tail shows as composition text.
  await page.waitForTimeout(2_500);
  await page.screenshot({ path: join(SHOTS, "dictation-3-listening.png") });
  // The clip is 3.4 s. By default the stream also hears a pause after it
  // (its endpoint); STOP_AFTER_MS stops sooner, mid-tail.
  await page.waitForTimeout(Number(process.env.STOP_AFTER_MS ?? 5_000) - 2_500);
  await page.screenshot({ path: join(SHOTS, "dictation-4-before-stop.png") });

  await page.evaluate(() => {
    (window as unknown as Record<string, any>).__armed = true;
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
  const text = await page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    wasm["on_key_down"]("a", false, true, false, false);
    wasm["on_key_down"]("c", false, true, false, false);
    return wasm["wasm_clipboard_get"]() as string | null;
  });
  console.log(`field text: ${JSON.stringify(text)}`);
  // The words land after the field's text, spaced by the keyboard. The
  // editor is a multi-line TextArea, which auto-capitalises, but the caret
  // follows "Edit this text" (no `.`, `!`, `?` or line break), so the
  // keyboard's VoiceContext has sentence_start false and the provider
  // lowercases the first word: "make". The middle is the engine's: on live audio
  // whistle's stream reads the clip's second word differently from run to
  // run ("cube", "cute"), and has been seen to commit "mm" and later
  // "millimeters" for the same audio (agg-gui-whistle's own tests check the
  // provider passes on exactly the text whistle commits).
  expect(text ?? "").toMatch(/^Edit this text make the .*\b20\b.*wide and round the top edges\.$/);
});

async function waitForState(page: Page, pattern: RegExp, timeoutMs = 60_000): Promise<string> {
  const t0 = Date.now();
  for (;;) {
    const s = (await dictationState(page)) ?? "none";
    if (pattern.test(s)) return s;
    if (Date.now() - t0 > timeoutMs) throw new Error(`state stuck at ${s}, wanted ${pattern}`);
    await page.waitForTimeout(20);
  }
}

test("a failed download is an error the next keyboard show retries, then the cache serves", async ({
  page,
  context,
}) => {
  test.setTimeout(120_000);
  const vp = page.viewportSize()!;
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(TEXT_EDIT_IDX, vp.width, vp.height));
  // Every copy of the model fails to download (the worker's fetches too).
  let blocked = true;
  await context.route(/whistle\.cact/, (route) =>
    blocked ? route.abort("internetdisconnected") : route.continue(),
  );
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);

  await page.touchscreen.tap(250, 340);
  const failed = await waitForState(page, /^Error/);
  expect(failed).toContain("Couldn't download the speech model");
  expect(failed).toContain("retry: true");
  await page.screenshot({ path: join(SHOTS, "dictation-6-download-failed.png") });

  // Close the keyboard (tap outside the field), let downloads through, and
  // bring it back: the show prepares again, which retries the download.
  blocked = false;
  await page.touchscreen.tap(200, 135);
  await page.waitForTimeout(600);
  await page.touchscreen.tap(250, 340);
  await waitForState(page, /^Ready$/);

  // A reload loads the checked copy from the Cache API: no download.
  let fetched = 0;
  await context.route(/whistle\.cact/, (route) => {
    fetched++;
    return route.continue();
  });
  await page.reload();
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  const t0 = Date.now();
  await page.touchscreen.tap(250, 340);
  await waitForState(page, /^Ready$/);
  console.log(`ready from the cache after ${Date.now() - t0} ms`);
  expect(fetched, "the model comes from the cache").toBe(0);
});
