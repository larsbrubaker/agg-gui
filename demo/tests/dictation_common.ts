/**
 * What the dictation tests share (dictation.test.ts, dictation_denied.test.ts,
 * dictation_audio.test.ts): the saved state that opens the TextEdit demo
 * alone, reading the provider's state through the demo's `dictation_state`
 * export, the phone profile, and where to tap.
 *
 * The demo exposes no automation hooks for widget positions (its wasm
 * exports are the input entry points, the clipboard and `dictation_state`),
 * so the taps are positions measured on the Pixel 7 profile below with the
 * TextEdit demo filling the window: keep them here, in one place.
 */

import { devices, type Page } from "@playwright/test";
import { readFileSync } from "fs";

import { DEMO_COUNT, TEST_COUNT, demoIndex } from "./demo_specs";

/** The phone profile every dictation test runs on (Chrome, touch). */
const pixel = devices["Pixel 7"];
export const PHONE = {
  viewport: pixel.viewport,
  deviceScaleFactor: 2,
  isMobile: true,
  hasTouch: true,
  userAgent: pixel.userAgent,
  channel: "chrome",
};

/** The TextEdit demo's multi-line TextArea: a tap brings the keyboard up. */
export const FIELD = { x: 250, y: 340 };
/** The microphone key, in the keyboard's toolbar. */
export const MIC = { x: 380, y: 506 };
/** Above the field, outside it: a tap closes the keyboard. */
export const OUTSIDE = { x: 200, y: 135 };

/**
 * Long enough for the keyboard to finish sliding in after the field's tap,
 * so its keys are where [`MIC`] says. The key reports Ready as soon as the
 * keyboard is up (nothing loads before the first tap on it), so a test that
 * only waited for Ready would tap before the key is there.
 */
export const KEYBOARD_SETTLES_MS = 800;

/** A request for anything of the dictation's: the worker, its module, the
 * capture worklet or the model (`public/whistle/`, or a remote model). */
export const WHISTLE_REQUEST = /\/whistle\/|whistle\.cact/;

/** The capture worklet's module (~1 KB), which the provider prefetches when
 * it is installed so the microphone can be wired the moment it opens: the
 * one request of the dictation's made before the first tap. */
export const CAPTURE_MODULE = /capture-worklet\.js/;

export const TEXT_EDIT_IDX = demoIndex("TextEdit");

/** A saved state with only demo `openIdx` open, filling a `w` x `h` window. */
export function seedState(openIdx: number, w: number, h: number): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s += i === openIdx ? `d${i}=1,0,0,${w},${h},0\n` : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

/** Open the TextEdit demo alone on the next navigation. */
export async function seedTextEdit(page: Page): Promise<void> {
  const vp = page.viewportSize()!;
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(TEXT_EDIT_IDX, vp.width, vp.height));
}

/** The voice provider's state at the keyboard's last poll (Debug format). */
export async function dictationState(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return wasm["dictation_state"]() as string | null;
  });
}

/** Wait for a state matching `pattern`; throws with the last one seen. */
export async function waitForState(
  page: Page,
  pattern: RegExp,
  timeoutMs = 60_000,
): Promise<string> {
  const t0 = Date.now();
  for (;;) {
    const s = (await dictationState(page)) ?? "none";
    if (pattern.test(s)) return s;
    if (Date.now() - t0 > timeoutMs) throw new Error(`state stuck at ${s}, wanted ${pattern}`);
    await page.waitForTimeout(20);
  }
}

/** The focused field's text: select all and copy into the app's clipboard. */
export async function fieldText(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    wasm["on_key_down"]("a", false, true, false, false);
    wasm["on_key_down"]("c", false, true, false, false);
    return wasm["wasm_clipboard_get"]() as string | null;
  });
}

/**
 * Record, in the page, when the dictation worker reports its download's
 * first and last progress and the model loaded (`__load`, in
 * `performance.now()` ms; 0 until then). Playwright reports a dedicated
 * worker's requests to the page but not when their bodies finish, so the
 * worker's own replies time the download.
 */
export async function watchModelLoad(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__load = { firstProgress: 0, lastProgress: 0, loaded: 0 };
    const RealWorker = window.Worker;
    win.Worker = class extends RealWorker {
      constructor(url: string | URL, options?: WorkerOptions) {
        super(url, options);
        this.addEventListener("message", (e: MessageEvent) => {
          const now = performance.now();
          const type = e.data && e.data.type;
          if (type === "progress") {
            if (!win.__load.firstProgress) win.__load.firstProgress = now;
            win.__load.lastProgress = now;
          } else if (type === "loaded") {
            win.__load.loaded = now;
          }
        });
      }
    };
  });
}

/** What [`watchModelLoad`] recorded. */
export async function modelLoad(
  page: Page,
): Promise<{ firstProgress: number; lastProgress: number; loaded: number }> {
  return page.evaluate(() => (window as unknown as Record<string, any>).__load);
}

/** A `getUserMedia` call the page made: when (`performance.now()` ms),
 * whether it was made inside a tap's touchend dispatch (the user gesture),
 * and whether it gave a stream. */
export type MicRequest = { at: number; inTap: boolean; granted: boolean | null };

/**
 * Record, in the page, every `getUserMedia` call (`__gum`) and every track
 * one gave (`__tracks`), so a test can tell when the microphone was asked
 * for and whether any track is still live.
 */
export async function recordMicRequests(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    win.__gum = [];
    win.__tracks = [];
    // Inside a touchend's dispatch: set by the window's capturing listener
    // (first), cleared by its bubbling one (after the page's own handler).
    let inTap = false;
    window.addEventListener("touchend", () => (inTap = true), { capture: true });
    window.addEventListener("touchend", () => (inTap = false));
    const real = MediaDevices.prototype.getUserMedia;
    MediaDevices.prototype.getUserMedia = function (this: MediaDevices, constraints) {
      const call = { at: performance.now(), inTap, granted: null as boolean | null };
      win.__gum.push(call);
      return real.call(this, constraints).then(
        (stream: MediaStream) => {
          call.granted = true;
          win.__tracks.push(...stream.getTracks());
          return stream;
        },
        (err: unknown) => {
          call.granted = false;
          throw err;
        },
      );
    };
  });
}

/** What [`recordMicRequests`] recorded so far. */
export async function micRequests(page: Page): Promise<MicRequest[]> {
  return page.evaluate(() => (window as unknown as Record<string, any>).__gum as MicRequest[]);
}

/** Whether any track `getUserMedia` gave is still live. */
export async function liveTracks(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      ((window as unknown as Record<string, any>).__tracks as MediaStreamTrack[]).filter(
        (t) => t.readyState === "live",
      ).length,
  );
}

/**
 * A user who speaks at the cue: `getUserMedia` returns a stream from a Web
 * Audio graph (the clip at `clipPath` in an `AudioBufferSourceNode`, into a
 * `MediaStreamAudioDestinationNode`) that plays silence until the app
 * reports Listening ("Speak now"), then the clip once. The tests that use it
 * then check what a user who waits for the cue gets, independent of when
 * Chrome's fake device would start its file. The time the clip started is
 * `__cue.startedAt` (`performance.now()` ms); the calls, `__cue.asked`, as
 * [`MicRequest`]s (made inside a tap's touchend dispatch or not).
 */
export async function speakAtCue(page: Page, clipPath: string): Promise<void> {
  const clip = readFileSync(clipPath).toString("base64");
  await page.addInitScript((clip: string) => {
    const win = window as unknown as Record<string, any>;
    win.__cue = { asked: [], startedAt: 0 };
    let inTap = false;
    window.addEventListener("touchend", () => (inTap = true), { capture: true });
    window.addEventListener("touchend", () => (inTap = false));
    const bytes = Uint8Array.from(atob(clip), (c) => c.charCodeAt(0));
    MediaDevices.prototype.getUserMedia = async function () {
      win.__cue.asked.push({ at: performance.now(), inTap, granted: true });
      // The page's own AudioContext, before a test wraps it (the iOS
      // emulation keeps it there).
      const Context = (win.__RealAudioContext ?? window.AudioContext) as typeof AudioContext;
      const ctx = new Context();
      void ctx.resume();
      const buffer = await ctx.decodeAudioData(bytes.slice().buffer);
      const destination = ctx.createMediaStreamDestination();
      const source = ctx.createBufferSource();
      source.buffer = buffer;
      source.connect(destination);
      const stream = destination.stream;
      const watch = setInterval(() => {
        if (stream.getTracks().every((t) => t.readyState === "ended")) {
          clearInterval(watch);
          return;
        }
        const state = win.__wasm?.["dictation_state"]?.();
        if (typeof state === "string" && state.startsWith("Listening")) {
          clearInterval(watch);
          win.__cue.startedAt = performance.now();
          source.start();
        }
      }, 5);
      return stream;
    };
  }, clip);
}

/**
 * Record what the dictation worker is sent and answers, for a failure's
 * diagnosis (`workerTraffic`): message counts by type, the audio pushed
 * (seconds, loudest sample) and the text the worker's updates carried.
 * Test-side only: the provider logs nothing of this.
 */
export async function recordWorkerTraffic(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const win = window as unknown as Record<string, any>;
    const t = (win.__traffic = { sent: {} as Record<string, number>, pushed: 0, peak: 0, replies: {} as Record<string, number>, text: [] as string[] });
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (this: Worker, msg: any, ...rest: any[]) {
      if (msg && typeof msg === "object" && "type" in msg) {
        t.sent[msg.type] = (t.sent[msg.type] ?? 0) + 1;
        if (msg.type === "push" && msg.samples) {
          t.pushed += msg.samples.length / msg.rate;
          for (const v of msg.samples) t.peak = Math.max(t.peak, Math.abs(v));
        }
      }
      return (post as any).call(this, msg, ...rest);
    };
    const Real = window.Worker;
    win.Worker = class extends Real {
      constructor(url: string | URL, options?: WorkerOptions) {
        super(url, options);
        this.addEventListener("message", (e: MessageEvent) => {
          const type = e.data && e.data.type;
          if (typeof type !== "string" || type === "progress") return;
          t.replies[type] = (t.replies[type] ?? 0) + 1;
          const said = [e.data.text, e.data.pending].filter((s) => typeof s === "string" && s);
          if (said.length) t.text.push(said.join(" | "));
        });
      }
    };
  });
}

/** What [`recordWorkerTraffic`] recorded, as one line. */
export async function workerTraffic(page: Page): Promise<string> {
  return page
    .evaluate(() => JSON.stringify((window as unknown as Record<string, any>).__traffic))
    .catch((e) => `unavailable: ${e}`);
}

/** The `getUserMedia` calls the speaking user ([`speakAtCue`]) answered. */
export async function cueRequests(page: Page): Promise<MicRequest[]> {
  return page.evaluate(() => (window as unknown as Record<string, any>).__cue.asked as MicRequest[]);
}
