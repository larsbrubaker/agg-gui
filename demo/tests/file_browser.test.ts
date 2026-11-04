/**
 * End-to-end mouse tests for the File Browser demo under real, trusted
 * browser input: `page.mouse` events → `src/app.ts`'s canvas listeners →
 * demo-wasm's `on_mouse_*` bindings → agg-gui-file-browser.
 *
 * Guards the two things the browser binding must hand agg-gui with every
 * press, which the file browser's gestures read:
 *  1. The click count. A double click on a folder in the main list opens it
 *     (`is_double_click()` on the second press); a binding that reports every
 *     press as a first click leaves double clicks doing nothing.
 *  2. The event's modifier keys. Shift+click extends the selection from the
 *     anchor to the clicked item; a binding that drops modifiers turns it into
 *     a plain click that selects only one.
 *
 * State is read through demo-wasm's `widget_nodes` export (the widget
 * inspector's walk): the `FileBrowser` widget's `path` and `selected`
 * properties, and the list tiles' names and rects to aim the clicks.
 *
 * Determinism: like keyboard.test.ts, we preseed `localStorage` so exactly
 * one demo window — the File Browser — is open at a known rect.
 *
 * Run:  cd demo && bunx playwright test file_browser
 */

import { test, expect, type Page } from "@playwright/test";
import { DEMO_COUNT, TEST_COUNT, demoIndex } from "./demo_specs";

const W = 1200;
const H = 800;

const FILE_BROWSER_IDX = demoIndex("File Browser");

// The single open window's rect in the app's Y-up coordinates.
const WIN = { x: 150, y: 60, w: 900, h: 680 };

type Node = {
  x: number;
  y: number;
  w: number;
  h: number;
  visible: boolean;
  props: Record<string, string>;
};

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

async function nodes(page: Page, type: string): Promise<Node[]> {
  return page.evaluate((t: string) => {
    const wasm = (window as unknown as Record<string, any>).__wasm;
    return wasm["widget_nodes"](t) as Node[];
  }, type);
}

/** The one File Browser widget's properties. */
async function browser(page: Page): Promise<Record<string, string>> {
  const found = (await nodes(page, "FileBrowser")).filter((n) => n.visible);
  expect(found.length, "exactly one File Browser is shown").toBe(1);
  return found[0].props;
}

/** The shown list tiles (icon view) or rows (list view) inside the window. */
async function listItems(page: Page): Promise<Node[]> {
  const inWindow = (n: Node) =>
    n.visible &&
    n.w > 0 &&
    n.x >= WIN.x &&
    n.x + n.w <= WIN.x + WIN.w &&
    n.y >= WIN.y &&
    n.y + n.h <= WIN.y + WIN.h;
  const tiles = (await nodes(page, "BrowserTile")).filter(inWindow);
  return tiles.length > 0 ? tiles : (await nodes(page, "BrowserRow")).filter(inWindow);
}

/** The CSS (Y-down) centre of a node's Y-up rect. */
function centre(n: Node): [number, number] {
  return [n.x + n.w / 2, H - (n.y + n.h / 2)];
}

/** Wait until the list shows something to click (pictures and folders load async). */
async function waitForItems(page: Page): Promise<Node[]> {
  for (let i = 0; i < 50; i++) {
    const items = await listItems(page);
    if (items.length >= 2) return items;
    await page.waitForTimeout(100);
  }
  return listItems(page);
}

test("double-clicking a folder in the main list opens it", async ({ page }) => {
  await bootWithState(page, FILE_BROWSER_IDX);
  const items = await waitForItems(page);
  const folder = items.find((n) => n.props["is_container"] === "true");
  expect(folder, `a folder in the list: ${JSON.stringify(items.map((n) => n.props["name"]))}`)
    .toBeTruthy();
  const before = (await browser(page))["path"];
  const name = folder!.props["name"];

  const [x, y] = centre(folder!);
  await page.mouse.dblclick(x, y);
  await page.waitForTimeout(400);

  const after = (await browser(page))["path"];
  expect(after, `double-clicking "${name}" opens it (was "${before}")`).toBe(
    before === "" ? name : `${before} > ${name}`,
  );
});

test("Shift+click extends the list selection from the clicked item", async ({ page }) => {
  await bootWithState(page, FILE_BROWSER_IDX);
  const items = await waitForItems(page);
  expect(items.length, "the list shows at least two items").toBeGreaterThanOrEqual(2);
  const first = items[0];
  const second = items[1];

  const [x1, y1] = centre(first);
  await page.mouse.click(x1, y1);
  await page.waitForTimeout(200);
  expect((await browser(page))["selected"]).toBe(first.props["name"]);

  const [x2, y2] = centre(second);
  await page.keyboard.down("Shift");
  await page.mouse.click(x2, y2);
  await page.keyboard.up("Shift");
  await page.waitForTimeout(200);

  const selected = (await browser(page))["selected"].split(", ").sort();
  expect(selected, "Shift+click selects both items").toEqual(
    [first.props["name"], second.props["name"]].sort(),
  );
});
