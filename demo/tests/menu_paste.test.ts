/**
 * The right-click menu's Paste reads the system clipboard (the WASM demo on
 * desktop Chrome), as the touch toolbar's Paste does (touch_paste.test.ts).
 *
 * Text copied in another app is on the system clipboard only, never in the
 * page's own clipboard, so the menu's Paste reads it with
 * `navigator.clipboard.readText()` inside the click
 * (`agg-gui/src/clipboard/system_paste.rs`) and pastes it into the field
 * when the promise resolves. This grants clipboard access, writes text with
 * `navigator.clipboard.writeText`, right-clicks the TextEdit demo's editor,
 * clicks Paste and reads the field back (`focused_text`).
 *
 * Determinism: localStorage is preseeded so exactly the TextEdit window is
 * open at a known rect; the coordinates are CSS pixels on that layout.
 *
 * Run:  cd demo && bunx playwright test menu_paste
 */

import { test, expect, type Page } from "@playwright/test";
import { DEMO_COUNT, TEST_COUNT, demoIndex } from "./demo_specs";

const W = 1200;
const H = 800;
const TEXT_EDIT_IDX = demoIndex("TextEdit");

/** Inside the multi-line editor, below its one line of text. */
const EDITOR = { x: 500, y: 360 };
/** The menu opens at the click; Paste is its third row. */
const PASTE = { x: EDITOR.x + 48, y: EDITOR.y + 10 };

function seedState(openIdx: number): string {
  let s = `version=1\ndemos=${DEMO_COUNT}\ntests=${TEST_COUNT}\n`;
  for (let i = 0; i < DEMO_COUNT; i++) {
    s += i === openIdx ? `d${i}=1,300,120,600,560,0\n` : `d${i}=0,0,0,0,0,0\n`;
  }
  for (let i = 0; i < TEST_COUNT; i++) s += `t${i}=0,0,0,0,0,0\n`;
  s += "about=0,0,0,0,0,0\nbackend=0\nsnap=1\ntheme=light\n";
  return s;
}

async function focusedText(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return wasm["focused_text"]() as string | null;
  });
}

test("Paste in the right-click menu pastes what the system clipboard holds", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.addInitScript((state: string) => {
    localStorage.setItem("agg-gui-demo-state", state);
  }, seedState(TEXT_EDIT_IDX));
  await page.setViewportSize({ width: W, height: H });
  await page.goto("/");
  await page.locator("#loading").waitFor({ state: "hidden", timeout: 20_000 });
  await page.waitForTimeout(300);
  // Copied "in another app": on the system clipboard only.
  await page.evaluate(() => navigator.clipboard.writeText("pasted"));

  await page.mouse.click(EDITOR.x, EDITOR.y); // the caret at the end
  expect(await focusedText(page)).toBe("Edit this text");
  await page.mouse.click(EDITOR.x, EDITOR.y, { button: "right" });
  await page.waitForTimeout(200);
  await page.mouse.click(PASTE.x, PASTE.y);
  await expect.poll(() => focusedText(page)).toBe("Edit this textpasted");
});
