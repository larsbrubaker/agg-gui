/**
 * The demo's window list, read from `demo-ui/src/specs.rs` so the tests'
 * saved-state seeds never go stale when a demo is added: how many demo and
 * test windows there are (a seed must list exactly that many, or the app
 * ignores it) and where a demo sits in the list, by its title.
 */

import * as fs from "fs";
import * as path from "path";

const SPECS = fs.readFileSync(
  path.join(__dirname, "..", "..", "demo-ui", "src", "specs.rs"),
  "utf8",
);

/** The window titles of the `name` array (`DEMOS` or `TESTS`), in order. */
function titles(name: string): string[] {
  const start = SPECS.indexOf(`const ${name}: &[DemoSpec] = &[`);
  if (start < 0) throw new Error(`specs.rs has no ${name} array`);
  const end = SPECS.indexOf("\n];", start);
  const body = SPECS.slice(start, end);
  return [...body.matchAll(/^\s*title:\s*"([^"]*)"/gm)].map((m) => m[1]);
}

const DEMO_TITLES = titles("DEMOS");
const TEST_TITLES = titles("TESTS");

export const DEMO_COUNT = DEMO_TITLES.length;
export const TEST_COUNT = TEST_TITLES.length;

/** The index in DEMOS of the demo whose title ends with `name`
 *  (titles start with a Font Awesome icon escape). */
export function demoIndex(name: string): number {
  const i = DEMO_TITLES.findIndex((t) => t.endsWith(` ${name}`));
  if (i < 0) throw new Error(`no demo titled "${name}"`);
  return i;
}
