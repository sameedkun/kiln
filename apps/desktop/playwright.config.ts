import { defineConfig } from "@playwright/test";

// The gallery's screenshot and accessibility checks (ADR 0051 §8), in Microsoft Edge: the
// same Chromium engine as WebView2, already on Windows, so nothing is downloaded.
export default defineConfig({
  testDir: "e2e",
  outputDir: "test-results",
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:4173",
    channel: "msedge",
    viewport: { width: 1280, height: 900 },
  },
  webServer: {
    command: "npm run preview",
    url: "http://localhost:4173",
    reuseExistingServer: !process.env.CI,
  },
});
