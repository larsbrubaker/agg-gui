import { defineConfig, devices } from "@playwright/test";

// Tests target real Google Chrome (channel "chrome") rather than the bundled
// chromium-headless-shell.  The bundled shell ships only bare SwiftShader-GL,
// which fails wgpu's shader validation on the demo's 'solid' vertex pipeline
// and kills WASM init before the first frame paints.  Real Chrome routes
// WebGL2 through ANGLE and compiles the wgpu-generated shaders cleanly.
// GitHub-hosted ubuntu-26.04 ships Google Chrome pre-installed, so deploy
// CI just needs Playwright's OS deps (`playwright install-deps chrome`).
const CHROME_USE = {
  ...devices["Desktop Chrome"],
  channel: "chrome",
  headless: true,
};

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  retries: 0,
  timeout: 60_000,

  use: {
    baseURL: "http://localhost:3001",
    ...CHROME_USE,
  },

  projects: [
    {
      name: "chromium",
      use: CHROME_USE,
    },
  ],

  // Start the dev server automatically if not already running.
  webServer: {
    command: "bun run dev",
    port: 3001,
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
