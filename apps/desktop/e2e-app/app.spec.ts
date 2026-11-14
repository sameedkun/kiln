import { spawn, type ChildProcess } from "node:child_process";
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";

// The built app, end to end (P4-13): kiln-desktop.exe with a real kilnd and kiln beside it,
// driven through WebView2's debugging port. Runs on CI's Windows runner, where KILN_APP_EXE
// names the app; nothing here is a mock.
const exe = process.env.KILN_APP_EXE;
const PORT = 9333;

test.skip(!exe, "KILN_APP_EXE names the built app");

let app: ChildProcess | undefined;
let browser: Browser | undefined;

test.afterAll(async () => {
  await browser?.close().catch(() => {});
  app?.kill();
});

/** The app's main window: its page that isn't the tray popup. */
async function mainWindow(): Promise<Page> {
  for (let i = 0; i < 90; i++) {
    try {
      browser ??= await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const page = browser
        .contexts()
        .flatMap((c) => c.pages())
        .find((p) => !p.url().includes("tray") && !p.url().startsWith("about:"));
      if (page) return page;
    } catch {
      // WebView2 isn't listening yet.
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error("the app's window never appeared on the debugging port");
}

test("the built app starts kilnd and shows Kiln's own answers", async ({}, info) => {
  test.setTimeout(240_000);
  app = spawn(exe!, [], {
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}` },
    stdio: "ignore",
  });
  const page = await mainWindow();

  // The app starts kilnd through kiln, then reads through it.
  await expect(page.getByText("Kiln is running")).toBeVisible({ timeout: 120_000 });

  // A fresh PC: the onboarding opens on its own; "Not now" leaves it.
  await expect(page.getByRole("heading", { name: "Welcome to Kiln" })).toBeVisible({ timeout: 60_000 });
  await page.screenshot({ path: info.outputPath("onboarding.png") });
  await page.getByRole("button", { name: "Not now" }).click();

  // Sites, from sites.list: none on this runner.
  await page.getByRole("button", { name: /^Sites/ }).click();
  await expect(page.getByRole("heading", { name: "Sites", level: 1 })).toBeVisible();

  // Settings, from settings.get: kiln --json settings through kilnd.
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByText(".test", { exact: true })).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole("switch", { name: "Let assistants make changes" })).toBeVisible();

  // History and Doctor answer too.
  await page.getByRole("button", { name: "Doctor" }).click();
  await expect(page.getByText(/checks passed|needs? a look/)).toBeVisible({ timeout: 120_000 });
  await page.screenshot({ path: info.outputPath("doctor.png") });
});
