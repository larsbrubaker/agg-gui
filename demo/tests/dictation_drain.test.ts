/**
 * The end of a web dictation, watched from the page in real Chrome on a
 * phone profile with a whistle-rust clip as the microphone (like
 * dictation.test.ts). Init scripts record every message the provider posts
 * to its worker ({type, session, time}), and every "flush" it posts to the
 * capture worklet.
 *
 * - A stop drains the worklet (agg-gui-whistle/src/web/drain.rs): the
 *   flush goes out after the stop's tap, then the session's last Push,
 *   then its Finish.
 * - A worklet that never answers the flush (a Blob-URL processor that
 *   ignores it) still lets the dictation finish, about 500 ms after the
 *   stop.
 * - A drain still waiting when the next session's stop starts another
 *   (the first session's engine failed during its drain, the user tapped
 *   again and stopped) ends the first drain and runs the second: the web
 *   host once panicked here on a RefCell it still held borrowed.
 * - A provider removed mid-dictation (the demo's `dictation_set_enabled`)
 *   terminates its worker, stops its track and closes its AudioContext,
 *   once each, and no callback reaches a dropped closure.
 *
 * Needs the dictation worker and model in the served site and the clip, as
 * dictation.test.ts does.
 *
 * Run:  cd demo && bunx playwright test dictation_drain
 */

import { test, expect, type Page } from "@playwright/test";
import { existsSync } from "fs";
import { join } from "path";

import {
  FIELD,
  KEYBOARD_SETTLES_MS,
  MIC,
  PHONE,
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
      `--use-file-for-fake-audio-capture=${CLIP}%noloop`,
    ],
  },
});

/** A message the provider posted to its worker. */
type Post = { type: string; session?: number; t: number };

/**
 * Record the provider's traffic: its posts to the worker (`__posts`), its
 * flushes to the worklet (`__flushes`), the worker itself (`__worker`), and
 * the time of the first touchend once `__armed` (`__stopAt`).
 */
async function recordTraffic(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__posts = [];
    win.__flushes = [];
    win.__stopAt = 0;
    win.__armed = false;
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (this: Worker, msg: any, ...rest: any[]) {
      if (msg && typeof msg === "object" && "type" in msg) {
        win.__posts.push({ type: msg.type, session: msg.session, t: performance.now() });
      }
      return (post as any).call(this, msg, ...rest);
    };
    const RealWorker = window.Worker;
    win.Worker = class extends RealWorker {
      constructor(url: string | URL, options?: WorkerOptions) {
        super(url, options);
        win.__worker = this;
      }
    };
    const portPost = MessagePort.prototype.postMessage;
    MessagePort.prototype.postMessage = function (this: MessagePort, msg: any, ...rest: any[]) {
      if (msg === "flush") win.__flushes.push(performance.now());
      return (portPost as any).call(this, msg, ...rest);
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

/** The capture worklet replaced by one that posts its quanta but never
 * answers "flush". */
async function lazyWorklet(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const source = `
      class Lazy extends AudioWorkletProcessor {
        process(inputs) {
          const channel = inputs[0] && inputs[0][0];
          if (channel && channel.length > 0) this.port.postMessage(channel.slice(0));
          return true;
        }
      }
      registerProcessor("agg-gui-whistle-capture", Lazy);`;
    const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    const addModule = AudioWorklet.prototype.addModule;
    AudioWorklet.prototype.addModule = function (this: AudioWorklet, _url: string, options?: any) {
      return addModule.call(this, url, options);
    };
  });
}

async function boot(page: Page, errors: string[]): Promise<void> {
  page.on("console", (m) => {
    console.log(`[page ${m.type()}] ${m.text()}`);
    if (m.type() === "error") errors.push(m.text());
  });
  page.on("pageerror", (e) => errors.push(String(e)));
  await seedTextEdit(page);
  await page.goto(BASE);
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.touchscreen.tap(FIELD.x, FIELD.y);
  await waitForState(page, /^Ready$/);
  await page.waitForTimeout(KEYBOARD_SETTLES_MS);
}

async function posts(page: Page): Promise<Post[]> {
  return page.evaluate(() => (window as unknown as Record<string, any>).__posts as Post[]);
}

async function startListening(page: Page): Promise<number> {
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/, 30_000);
  const start = (await posts(page)).filter((p) => p.type === "start").pop();
  expect(start, "a start was posted").toBeTruthy();
  return start!.session!;
}

/** Tap stop, timing the tap in the page; its time. */
async function tapStop(page: Page): Promise<number> {
  await page.evaluate(() => {
    const win = window as unknown as Record<string, any>;
    win.__stopAt = 0;
    win.__armed = true;
  });
  await page.touchscreen.tap(MIC.x, MIC.y);
  return page.evaluate(() => (window as unknown as Record<string, any>).__stopAt as number);
}

/** No panic, no dropped closure called, no uncaught error. */
function expectNoErrors(errors: string[]): void {
  const bad = errors.filter((e) =>
    /panicked|already (mutably )?borrowed|closure invoked|RuntimeError|unreachable/i.test(e),
  );
  expect(bad, bad.join("\n")).toEqual([]);
}

test("a stop drains the worklet: flush, then the session's last push, then its finish", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(120_000);
  const errors: string[] = [];
  await recordTraffic(page);
  await boot(page, errors);
  const session = await startListening(page);
  // Mid-clip: speech is arriving.
  await page.waitForTimeout(2_000);
  const stopAt = await tapStop(page);
  await waitForState(page, /^Ready$/, 30_000);
  const traffic = (await posts(page)).filter((p) => p.session === session);
  const flushes = await page.evaluate(
    () => (window as unknown as Record<string, any>).__flushes as number[],
  );
  const finishAt = traffic.findIndex((p) => p.type === "finish");
  expect(finishAt, "a finish was posted").toBeGreaterThan(0);
  const finish = traffic[finishAt];
  const flush = flushes.find((t) => t >= stopAt);
  expect(flush, "a flush went to the worklet after the stop").toBeDefined();
  const lastPush = traffic
    .slice(0, finishAt)
    .filter((p) => p.type === "push")
    .pop();
  expect(lastPush, "audio before the finish").toBeDefined();
  console.log(
    `stop ${stopAt.toFixed(1)}, flush +${(flush! - stopAt).toFixed(1)}, ` +
      `last push +${(lastPush!.t - stopAt).toFixed(1)}, finish +${(finish.t - stopAt).toFixed(1)} ms`,
  );
  expect(lastPush!.t).toBeGreaterThanOrEqual(flush!);
  expect(finish.t).toBeGreaterThanOrEqual(lastPush!.t);
  expect(traffic.slice(finishAt + 1).some((p) => p.type === "push"), "no push after").toBe(false);
  expectNoErrors(errors);
});

test("a worklet that never answers the flush still finishes, after the drain's wait", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(120_000);
  const errors: string[] = [];
  await recordTraffic(page);
  await lazyWorklet(page);
  await boot(page, errors);
  const session = await startListening(page);
  await page.waitForTimeout(2_000);
  const stopAt = await tapStop(page);
  await waitForState(page, /^Ready$/, 30_000);
  const finish = (await posts(page)).find((p) => p.type === "finish" && p.session === session);
  expect(finish, "a finish was posted").toBeDefined();
  const waited = finish!.t - stopAt;
  console.log(`finish posted ${waited.toFixed(1)} ms after the stop`);
  expect(waited).toBeGreaterThanOrEqual(450);
  expect(waited).toBeLessThan(1_500);
  expectNoErrors(errors);
});

test("a drain still waiting when the next stop starts another ends first, without a panic", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(120_000);
  const errors: string[] = [];
  await recordTraffic(page);
  await lazyWorklet(page);
  // Stretch the drain's 500 ms wait so the next dictation fits inside it.
  await page.addInitScript(() => {
    const setTimeout = window.setTimeout;
    (window as any).setTimeout = function (handler: any, ms?: number, ...rest: any[]) {
      return (setTimeout as any).call(window, handler, ms === 500 ? 4_000 : ms, ...rest);
    };
  });
  await boot(page, errors);
  const first = await startListening(page);
  await page.waitForTimeout(1_500);
  await tapStop(page);
  await waitForState(page, /^Finishing$/, 5_000);
  // The first session's engine fails while its drain waits.
  await page.evaluate((session: number) => {
    const worker = (window as unknown as Record<string, any>).__worker as Worker;
    worker.dispatchEvent(
      new MessageEvent("message", { data: { type: "failed", session, detail: "test" } }),
    );
  }, first);
  await waitForState(page, /^Error/, 5_000);
  await page.waitForTimeout(300);
  // Tap again (a retry), and stop while the first drain still waits.
  const second = await startListening(page);
  expect(second).not.toBe(first);
  await tapStop(page);
  await waitForState(page, /^Ready$/, 30_000);
  const traffic = await posts(page);
  const finishes = traffic.filter((p) => p.type === "finish").map((p) => p.session);
  expect(finishes).toEqual([first, second]);
  expectNoErrors(errors);
});

test("a provider removed mid-dictation releases its worker, track and audio once each", async ({
  page,
}) => {
  test.skip(!existsSync(CLIP), `no clip at ${CLIP}`);
  test.setTimeout(120_000);
  const errors: string[] = [];
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__calls = { terminate: 0, stop: 0, close: 0 };
    const spy = (proto: any, name: string, key: string) => {
      const real = proto[name];
      proto[name] = function (...args: any[]) {
        win.__calls[key]++;
        return real.apply(this, args);
      };
    };
    spy(Worker.prototype, "terminate", "terminate");
    spy(MediaStreamTrack.prototype, "stop", "stop");
    spy(AudioContext.prototype, "close", "close");
  });
  await boot(page, errors);
  await page.touchscreen.tap(MIC.x, MIC.y);
  await waitForState(page, /^Listening/, 30_000);
  await page.waitForTimeout(1_000);
  await page.evaluate(() =>
    (window as unknown as Record<string, any>).__wasm["dictation_set_enabled"](false),
  );
  // Quanta and worker messages already queued must find no dropped closure.
  await page.waitForTimeout(1_000);
  const calls = await page.evaluate(() => (window as unknown as Record<string, any>).__calls);
  const state = await page.evaluate(() =>
    (window as unknown as Record<string, any>).__wasm["dictation_state"](),
  );
  console.log("calls:", JSON.stringify(calls), "state:", state);
  // No provider: the export returns undefined (Rust's None).
  expect(state ?? null).toBeNull();
  expect(calls).toEqual({ terminate: 1, stop: 1, close: 1 });
  expectNoErrors(errors);
});
