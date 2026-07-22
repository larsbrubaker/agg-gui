/**
 * End-to-end keyboard-input tests for the WASM demo (desktop path).
 *
 * Exercises the full desktop typing + clipboard-chord pipeline:
 * real, trusted keystrokes from `page.keyboard` → the hidden 1px textarea
 * (`mobileTextInput`) that `src/app.ts` focuses whenever an in-app text
 * widget gains focus (`syncMobileKeyboard`) → `forwardKeyDown(e, fromMobileTextInput=true)`
 * and the textarea's `beforeinput`/`input`/`copy` handlers → the wasm-bindgen
 * bindings → agg-gui core, where the focused editor inserts text and services
 * Ctrl+A / Ctrl+C.
 *
 * Guards two regressions that only reproduce under trusted browser input:
 *  1. Typing must NOT be swallowed: `forwardKeyDown` must not `preventDefault`
 *     printable keys on the textarea, or the browser's default insertion (which
 *     is what fires `beforeinput`/`input`) never runs and the character is lost.
 *  2. Ctrl+C must reach the SYSTEM clipboard: `forwardKeyDown` must forward the
 *     copy chord to wasm and then let the browser's copy command run, so the
 *     `copy` DOM event fires and `handleCopy` mirrors the app's internal buffer
 *     onto `navigator.clipboard`. Canceling the keydown kills the copy command
 *     and the system-clipboard leg silently breaks.
 *
 * Determinism: like touch.test.ts, we preseed `localStorage` so exactly one
 * demo window — the Code Editor — is open at a known rect, then click its
 * centre to focus the editor.
 *
 * Run:  cd demo && bunx playwright test keyboard
 */

import { test, expect, type Page } from "@playwright/test";

const W = 1200;
const H = 800;

// Must match the DEMOS / TESTS arrays in demo-ui/src/specs.rs.
const DEMO_COUNT = 34;
const TEST_COUNT = 13;
// Index of the "Code Editor" demo in DEMOS (a large editable text widget —
// clicking its centre reliably lands in the editor body, clear of any gutter).
const CODE_EDITOR_IDX = 10;

// The single open window's rect in the app's Y-up coordinates.
const WIN = { x: 300, y: 120, w: 600, h: 560 };
// Centre of the window in CSS (Y-down) coordinates — well below the title bar,
// safely inside the editor body.
const CLICK_X = WIN.x + WIN.w / 2; // 600
const CLICK_Y = H - (WIN.y + WIN.h / 2); // 800 - 400 = 400

// A distinctive marker unlikely to appear in the editor's seed content. All
// characters are single, unmodified printable keys so each flows through the
// textarea's insertText path exactly once.
const TYPED = "aggkbdprobe1234567890";

/** Serialized SavedState with exactly one demo window open. */
function seedState(openIdx: number): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s +=
      i === openIdx
        ? `d${i}=1,${WIN.x},${WIN.y},${WIN.w},${WIN.h},0\n`
        : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

async function bootWithState(page: Page, openIdx: number): Promise<void> {
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(openIdx));
  await page.setViewportSize({ width: W, height: H });
  await page.goto("/");
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 20_000 });
  await page.waitForTimeout(300);
}

test("desktop typing lands in the editor and Ctrl+C copies to both clipboards", async ({
  page,
  context,
}) => {
  // clipboard-read/write are required for the navigator.clipboard.readText()
  // assertion (the leg bug 2 breaks) to resolve instead of rejecting.
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);

  await bootWithState(page, CODE_EDITOR_IDX);

  // Click the editor body. The app focuses the text widget, and
  // syncMobileKeyboard focuses the hidden textarea to back IME/dead-key input.
  await page.mouse.click(CLICK_X, CLICK_Y);
  await page.waitForTimeout(300);

  // The desktop IME-focus path must have moved focus to the hidden textarea —
  // trusted keystrokes only reach forwardKeyDown(..., true) through it.
  const active = await page.evaluate(
    () => (document.activeElement as HTMLElement | null)?.tagName ?? null,
  );
  expect(active, "click into a text widget must focus the hidden textarea").toBe(
    "TEXTAREA",
  );

  // Trusted printable input. With bug 1 present these keydowns are
  // preventDefault'd, no beforeinput/input fires, and nothing is inserted.
  await page.keyboard.type(TYPED, { delay: 20 });
  await page.waitForTimeout(200);

  // Select all + copy. Ctrl+C forwards to wasm (fills the internal buffer) and
  // then lets the browser copy command fire the 'copy' event.
  await page.keyboard.press("Control+a");
  await page.waitForTimeout(100);
  await page.keyboard.press("Control+c");
  await page.waitForTimeout(200);

  // Internal buffer: proves the typed text actually landed in the editor.
  const internal = await page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return wasm["wasm_clipboard_get"]() as string | null;
  });
  expect(internal ?? "", "typed text must reach the editor's internal clipboard").toContain(
    TYPED,
  );

  // System clipboard: the leg bug 2 breaks — reachable only if the 'copy' DOM
  // event fired (i.e. Ctrl+C did NOT preventDefault the browser copy command).
  const system = await page.evaluate(() => navigator.clipboard.readText());
  expect(system, "Ctrl+C must push the copied text to the system clipboard").toContain(
    TYPED,
  );
});
