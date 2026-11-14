import { defineConfig } from "@playwright/test";

// The built app end to end (P4-13): no web server and no browser download; the test attaches
// to the app's own WebView2.
export default defineConfig({
  testDir: "e2e-app",
  outputDir: "test-results/app",
  reporter: [["list"]],
  workers: 1,
});
