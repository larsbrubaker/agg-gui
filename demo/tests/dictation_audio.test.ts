/**
 * Dictation's audio and download edge cases in the WASM demo, in real
 * Chrome on a phone profile with a whistle-rust clip as the microphone
 * (like dictation.test.ts):
 *
 * - iOS Safari only lets an AudioContext start inside a user gesture. The
 *   page's AudioContext is wrapped so that it does the same (a context made
 *   or resumed outside a tap's touchend/pointerup stays suspended), and a
 *   start the tap defers (the first tap, which downloads the model; a tap
 *   that retries a failed download) must still hear the microphone; a
 *   context that never starts must be a microphone error, not Listening to
 *   silence.
 * - Without an AudioWorklet (its module fails to load) the provider falls
 *   back to a ScriptProcessor, says so once in the console, and dictates,
 *   even while the main thread stalls (as CI's software-rendered one does):
 *   a ScriptProcessor hands its audio over on the main thread, from a
 *   double buffer the audio thread refills one buffer later. A stop still
 *   delivers the audio up to it, which that buffer (about a second) holds
 *   until its next event.
 * - The microphone is asked for inside the tap, and the key says "Speak
 *   now" only once its audio flows. The user here speaks the clip at that
 *   cue (`speakAtCue`), so every word must arrive: when the capture module
 *   is slow (the stream waits for it at most 1.5 s, then a ScriptProcessor
 *   captures), and on a main thread 20 times slower (CI's software-rendered
 *   one lags by seconds), which only delays the cue.
 * - Main-thread stalls longer than the ScriptProcessor's buffer replay no
 *   audio: an event that ran too late (its buffer already refilled) is
 *   dropped, so the stall costs words rather than replaying others. Once
 *   events are dropped, whistle's guesses over the gap are not the
 *   fallback's doing: these tests then check only what it owns (no audio
 *   twice, drops only with a stall, a finished dictation, the surviving
 *   words in order, nothing before the clip). AudioWorklet is in every
 *   current browser, so the fallback only serves old ones: the stall tests
 *   prove the replay fix, not recognition quality.
 * - A model download that stalls times out and is an error the next tap
 *   retries.
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
  PHONE,
  cueRequests,
  fieldText,
  recordWorkerTraffic,
  speakAtCue,
  workerTraffic,
  seedTextEdit,
  waitForState,
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
// Its tail too: "round the top edges" has come back "round to top edges"
// and "rounded top edges".
const DICTATED = /^Edit this text \w+ the .+ wide and round(?:ed| \w+) top edges\.$/;


/**
 * Block the page's main thread for `ms` every `every` ms until
 * [`endStalls`], as a slow one does (CI's software GL runs its frames
 * hundreds of milliseconds behind).
 */
async function startStalls(page: Page, ms: number, every: number): Promise<void> {
  await page.evaluate(
    ([ms, every]) => {
      (window as unknown as Record<string, any>).__stalls = setInterval(() => {
        const t0 = performance.now();
        while (performance.now() - t0 < ms) {
          // Busy: nothing else runs on this thread meanwhile.
        }
      }, every);
    },
    [ms, every],
  );
}

async function endStalls(page: Page): Promise<void> {
  await page.evaluate(() => clearInterval((window as unknown as Record<string, any>).__stalls));
}

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
    // For the speaking user's own graph (`speakAtCue`): a real context.
    (window as unknown as Record<string, unknown>).__RealAudioContext = Real;
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

/** Open the TextEdit demo and bring the keyboard up on its field (nothing
 * of the dictation loads yet). The microphone is a user who speaks the
 * clip at the cue (`speakAtCue`). */
async function boot(page: Page): Promise<void> {
  page.on("console", (m) => console.log(`[page ${m.type()}] ${m.text()}`));
  if (existsSync(CLIP)) await speakAtCue(page, CLIP);
  await recordWorkerTraffic(page);
  await seedTextEdit(page);
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
}

// A dictation that came out wrong says what the worker was sent and said.
test.afterEach(async ({ page }, testInfo) => {
  if (testInfo.status !== testInfo.expectedStatus) {
    console.log(`worker traffic: ${await workerTraffic(page)}`);
    console.log(`pushed audio: ${await pushedEnvelope(page, 0.5, 50)}`);
  }
});

/** Wait for the cue, let the user say the clip, stop, and wait for the
 * final text. */
async function dictateClip(page: Page): Promise<void> {
  await waitForState(page, /^Listening/, 60_000);
  await page.waitForTimeout(5_000);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
  await page.waitForTimeout(300);
}

test("the first tap, which waits for the download, starts the audio inside the tap and dictates once loaded", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  await emulateIosAudio(page, false);
  await boot(page);
  // The tap downloads the model; the microphone opens when it is in, after
  // the tap, from the worker's reply.
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  expect(text ?? "").toMatch(DICTATED);
});

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
  await page.touchscreen.tap(MIC.x, MIC.y);
  const failed = await waitForState(page, /^Error/);
  expect(failed).toContain("retry: true");

  // The tap retries the download; the microphone opens when the model is
  // in, long after the tap, from the worker's reply.
  blocked = false;
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  expect(text ?? "").toMatch(DICTATED);
});

test("on a main thread 20 times slower the request is still made once, in the tap, and every word said at the cue arrives", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 20 });
  await boot(page);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  const asked = await cueRequests(page);
  expect(asked.length).toBe(1);
  expect(asked[0].inTap, "made inside the tap's touchend dispatch").toBe(true);
  // Spoken at the cue: every word arrives, however late the cue came.
  expect(text ?? "").toMatch(DICTATED);
});

test("a capture module still loading 1.5 s after the stream gives way to the ScriptProcessor", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  // The worklet's module takes 2.5 s to load; count the processors.
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__processors = 0;
    const addModule = AudioWorklet.prototype.addModule;
    AudioWorklet.prototype.addModule = function (this: AudioWorklet, url: string, options?: any) {
      return new Promise((resolve) => setTimeout(resolve, 2_500)).then(() =>
        addModule.call(this, url, options),
      );
    };
    const create = BaseAudioContext.prototype.createScriptProcessor;
    BaseAudioContext.prototype.createScriptProcessor = function (...args) {
      win.__processors++;
      return create.apply(this, args);
    };
  });
  await boot(page);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  const text = await fieldText(page);
  console.log(`field text: ${JSON.stringify(text)}`);
  const processors = await page.evaluate(
    () => (window as unknown as Record<string, any>).__processors as number,
  );
  expect(processors, "the dictation fell back to a ScriptProcessor").toBe(1);
  // The cue came once the ScriptProcessor's audio flowed: nothing said
  // after it is lost.
  expect(text ?? "").toMatch(DICTATED);
});

test("an AudioContext that never starts is a microphone error, not listening to silence", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await emulateIosAudio(page, true);
  await boot(page);
  // The tap loads the model first (from the page's copy), then opens the
  // microphone.
  await page.touchscreen.tap(MIC.x, MIC.y);
  const seen: string[] = [];
  const t0 = Date.now();
  for (;;) {
    const s = (await page.evaluate(() =>
      (window as unknown as Record<string, any>).__wasm["dictation_state"](),
    )) as string;
    if (seen[seen.length - 1] !== s) seen.push(s);
    if (s.startsWith("Error") || Date.now() - t0 > 60_000) break;
    await page.waitForTimeout(20);
  }
  console.log("states:", seen.join(" | "));
  expect(seen.some((s) => s.startsWith("Listening")), "never listens").toBe(false);
  expect(seen[seen.length - 1]).toMatch(/^Error .*retry: true/);
});

/**
 * The worklet's module never loads (a request route doesn't see a
 * worklet's fetch, so the failure is made here); count the fallback's
 * processors (`__processors`).
 */
/** Record every sample pushed to the worker, in order, and count the
 * ScriptProcessor's events (`pushedBlocks`). */
async function recordPushedSamples(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__samples = [] as number[];
    win.__processorEvents = 0;
    // The longest the main thread was blocked since `armLateness`: how
    // late a 20 ms timer ran. A dropped event needs about a second of it.
    win.__lateness = { armed: false, max: 0 };
    let last = performance.now();
    setInterval(() => {
      const now = performance.now();
      if (win.__lateness.armed) {
        win.__lateness.max = Math.max(win.__lateness.max, now - last - 20);
      }
      last = now;
    }, 20);
    const proto = ScriptProcessorNode.prototype;
    const handler = Object.getOwnPropertyDescriptor(proto, "onaudioprocess")!;
    Object.defineProperty(proto, "onaudioprocess", {
      configurable: true,
      get() {
        return handler.get!.call(this);
      },
      set(fn) {
        const counted = (e: AudioProcessingEvent) => {
          win.__processorEvents++;
          return fn.call(this, e);
        };
        handler.set!.call(this, fn ? counted : null);
      },
    });
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (this: Worker, msg: any, ...rest: any[]) {
      if (msg && msg.type === "push") for (const v of msg.samples) win.__samples.push(v);
      return (post as any).call(this, msg, ...rest);
    };
  });
}

/**
 * The fallback's pushed audio cut into its buffers (16384 frames, pushed
 * back to back from the start): how many buffers of sound went twice
 * (replayed), and how many of the processor's events were dropped (ran too
 * late to hold their own audio, `processor_clock.rs`), their audio replaced
 * by a quiet room's noise (no sample above 0.002, not exact zeros: no
 * second of the clip is that quiet), or not pushed at all.
 */
async function pushedBlocks(
  page: Page,
): Promise<{ repeated: number; dropped: number; quietBefore: boolean }> {
  return page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    const all = win.__samples as number[];
    const frames = 16_384;
    const seen = new Set<string>();
    let repeated = 0;
    let blocks = 0;
    let filled = 0;
    for (let i = 0; i + frames <= all.length; i += frames) {
      blocks++;
      const block = all.slice(i, i + frames);
      if (block.every((v) => v === 0)) continue;
      if (block.every((v) => Math.abs(v) <= 0.002)) {
        filled++;
        continue;
      }
      const key = block.filter((_, j) => j % 61 === 0).join(",");
      if (seen.has(key)) repeated++;
      seen.add(key);
    }
    const unpushed = Math.max(0, (win.__processorEvents as number) - blocks);
    // The first buffer was captured before the cue, before the clip: it
    // must be silence (or the noise floor), not speech replayed from later.
    const first = all.slice(0, frames);
    const quietBefore = first.length < frames || first.every((v) => Math.abs(v) <= 0.002);
    return { repeated, dropped: filled + unpushed, quietBefore };
  });
}

/** Start measuring how long the main thread is blocked (`recordPushedSamples`). */
async function armLateness(page: Page): Promise<void> {
  await page.evaluate(() => {
    (window as unknown as Record<string, any>).__lateness = { armed: true, max: 0 };
  });
}

/** The clip's words, and how whistle has read each of them. */
const CLIP_WORDS = ["make", "the", "cube", "20", "mm", "wide", "and", "round", "the", "top", "edges"];
const READINGS: Record<string, string[]> = {
  mate: ["make"],
  cute: ["cube"],
  queue: ["cube"],
  q: ["cube"],
  q20: ["cube", "20"],
  twenty: ["20"],
  millimeters: ["mm"],
  meters: ["mm"],
  rounded: ["round"],
  to: ["the"],
};

/**
 * The clip's words in a dictation, as whistle read them, that are out of
 * the clip's order: none when the words that survived come in order. Other
 * words are whistle's guesses where audio was missing, which this skips.
 */
function outOfOrder(text: string | null): string[] {
  const said = (text ?? "")
    .replace(/^Edit this text/, "")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((w) => w.length > 0);
  const wrong: string[] = [];
  let at = 0;
  for (const word of said) {
    const meant = READINGS[word] ?? (CLIP_WORDS.includes(word) ? [word] : []);
    for (const m of meant) {
      const next = CLIP_WORDS.indexOf(m, at);
      if (next < 0) wrong.push(word);
      else at = next + 1;
    }
  }
  return wrong;
}

/**
 * What the fallback owns once events were dropped (audio the main thread
 * held back too long, `processor_clock.rs`): whistle's guesses over the
 * gap are not its doing, so not the words, but that no audio was pushed
 * twice, that drops came only with a real stall, that the dictation
 * finished, that the words left come in the clip's order, and that nothing
 * from before the clip was pushed as sound (replayed speech read "money").
 */
async function expectWhatTheFallbackOwns(
  page: Page,
  text: string | null,
  blocks: { repeated: number; dropped: number; quietBefore: boolean },
): Promise<void> {
  const blocked = await page.evaluate(
    () => (window as unknown as Record<string, any>).__lateness.max as number,
  );
  const finals = await page.evaluate(
    () => ((window as unknown as Record<string, any>).__traffic?.replies?.final ?? 0) as number,
  );
  console.log(
    `field text: ${JSON.stringify(text)}; ${blocks.dropped} event(s) dropped, ` +
      `${blocks.repeated} buffer(s) pushed twice; main thread blocked up to ` +
      `${blocked.toFixed(0)} ms`,
  );
  expect(blocks.repeated, "a buffer of audio pushed twice").toBe(0);
  if (blocks.dropped > 0) {
    // An event is dropped only when it ran more than a buffer (1.024 s,
    // less 50 ms) late: a block of the main thread close to that.
    expect(blocked, "events dropped without a stall to hold them back").toBeGreaterThan(800);
  }
  expect(finals, "the dictation finished with its final text").toBeGreaterThan(0);
  expect(blocks.quietBefore, "sound pushed from before the clip").toBe(true);
  expect(outOfOrder(text), JSON.stringify(text)).toEqual([]);
}

async function withoutWorklet(page: Page): Promise<void> {
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
}

test("without an AudioWorklet the ScriptProcessor fallback dictates and says so once", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  await withoutWorklet(page);
  const warnings: string[] = [];
  page.on("console", (m) => {
    // Ours, not Chrome's own deprecation notice for ScriptProcessorNode.
    if (m.text().startsWith("agg-gui-whistle: no AudioWorklet")) warnings.push(m.text());
  });
  await recordPushedSamples(page);
  await boot(page);
  // Stalls of 300 ms: a 64 ms buffer came out shuffled and repeated
  // ("je trouve que je trouve que ..." on CI).
  await startStalls(page, 300, 500);
  await armLateness(page);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  await endStalls(page);
  const text = await fieldText(page);
  const blocks = await pushedBlocks(page);
  await expectWhatTheFallbackOwns(page, text, blocks);
  if (blocks.dropped === 0) {
    // The main thread kept within the buffer: every word.
    expect(text ?? "").toMatch(DICTATED);
  }
  // With events dropped (CI's own lag on top of these stalls ran some more
  // than a buffer late), their audio is lost and whistle guesses over the
  // gap ("bite" for "wide" on CI): only what the fallback owns is checked.
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

test("without an AudioWorklet, main-thread stalls longer than its buffer replay no audio", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(180_000);
  await withoutWorklet(page);
  await recordPushedSamples(page);
  await boot(page);
  // Stalls of 2.5 s every 3 s: the events that wait one out run more than
  // a buffer (1.024 s) late, when Chrome has refilled their buffer with
  // later audio; pushed as read, that audio went twice ("cute twenty the
  // top edges").
  await startStalls(page, 2_500, 3_000);
  await armLateness(page);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await dictateClip(page);
  await endStalls(page);
  const text = await fieldText(page);
  const blocks = await pushedBlocks(page);
  // A proof of the replay fix, not of recognition: the stalls must drop
  // events, and what the fallback owns must hold.
  expect(blocks.dropped, "the stalls ran events too late").toBeGreaterThan(0);
  await expectWhatTheFallbackOwns(page, text, blocks);
});

/**
 * Record the fallback's audio: each ScriptProcessor event's time and length
 * (`__events`), each push to the worker (`__pushes`), and the first
 * touchend once `__armed` (`__stopAt`).
 */
async function recordProcessorAudio(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__events = [];
    win.__pushes = [];
    win.__stopAt = 0;
    win.__armed = false;
    const proto = ScriptProcessorNode.prototype;
    const handler = Object.getOwnPropertyDescriptor(proto, "onaudioprocess")!;
    Object.defineProperty(proto, "onaudioprocess", {
      configurable: true,
      get() {
        return handler.get!.call(this);
      },
      set(fn) {
        const wrapped = (e: AudioProcessingEvent) => {
          const b = e.inputBuffer;
          win.__events.push({ t: performance.now(), seconds: b.length / b.sampleRate });
          return fn.call(this, e);
        };
        handler.set!.call(this, fn ? wrapped : null);
      },
    });
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (this: Worker, msg: any, ...rest: any[]) {
      if (msg && msg.type === "push") {
        win.__pushes.push({ t: performance.now(), seconds: msg.samples.length / msg.rate });
      }
      return (post as any).call(this, msg, ...rest);
    };
    document.addEventListener(
      "touchend",
      () => {
        if (win.__armed && !win.__stopAt) win.__stopAt = performance.now();
      },
      { capture: true },
    );
  });
}

test("without an AudioWorklet a stop delivers the audio up to the stop", async ({ page }) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(120_000);
  await withoutWorklet(page);
  await recordProcessorAudio(page);
  await boot(page);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/, 60_000);
  // Stop 300 ms after an event, mid-speech: that audio is in the
  // processor's buffer, which only its next event hands over.
  await page.waitForFunction(() => (window as any).__events.length >= 2);
  const seen = await page.evaluate(() => (window as any).__events.length as number);
  await page.waitForFunction((n) => (window as any).__events.length > n, seen);
  await page.waitForTimeout(300);
  await page.evaluate(() => ((window as any).__armed = true));
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
  const { events, pushes, stopAt } = await page.evaluate(() => {
    const w = window as unknown as Record<string, any>;
    return { events: w.__events, pushes: w.__pushes, stopAt: w.__stopAt as number };
  });
  expect(stopAt, "the stop's tap was seen").toBeGreaterThan(0);
  // The first event handed over its buffer as it filled: the microphone's
  // audio at the stop is that much plus the time since.
  const heard = events[0].seconds + (stopAt - events[0].t) / 1000;
  const pushed = pushes.reduce((sum: number, p: { seconds: number }) => sum + p.seconds, 0);
  console.log(`heard ${heard.toFixed(2)} s by the stop, pushed ${pushed.toFixed(2)} s`);
  expect(pushed).toBeGreaterThanOrEqual(heard - 0.1);
});

test("a download that stalls times out and the next tap retries it", async ({
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
  await page.touchscreen.tap(MIC.x, MIC.y);
  const failed = await waitForState(page, /^Error/, 60_000);
  console.log(`stalled download failed after ${Date.now() - t0} ms: ${failed}`);
  expect(failed).toContain("Couldn't download the speech model");
  expect(failed).toContain("retry: true");

  stalled = false;
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Ready$/, 30_000);
});
