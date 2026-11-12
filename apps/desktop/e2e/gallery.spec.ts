import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

// The design gallery in every appearance (ADR 0051 §8): axe finds no accessibility
// violations, and screenshots are kept for review. Text contrast on glass is measured by
// src/styles/contrast.test.ts, because axe can't measure text on translucent layers: it
// files those under "incomplete", which is allowed here only for that reason.

const modes = [
  { name: "light", query: "?theme=light" },
  { name: "dark", query: "?theme=dark" },
  { name: "light-solid", query: "?theme=light&solid" },
  { name: "dark-solid", query: "?theme=dark&solid" },
];

/**
 * Why axe may leave a contrast check unfinished: it can't see through layers, or the text is
 * a single character (a count). Both are measured from the design values instead.
 */
const layered = new Set(["bgOverlap", "bgGradient", "bgImage", "bgFilter", "pseudoContent", "elmPartiallyObscured", "nonBmp", "shortTextContent"]);

async function scan(page: Page) {
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze();
  const unexplained = result.incomplete
    .filter((rule) => rule.id === "color-contrast")
    .flatMap((rule) => rule.nodes)
    .filter((node) => !node.any.some((check) => layered.has((check.data as { messageKey?: string } | null)?.messageKey ?? "")))
    .map((node) => `${node.target.join(" ")}: ${node.any.map((c) => c.message).join("; ")}`);
  return { violations: result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`), unexplained };
}

for (const mode of modes) {
  test(`gallery, ${mode.name}`, async ({ page }, info) => {
    await page.goto(`/${mode.query}`);
    await expect(page.getByRole("heading", { name: "Home", level: 1 })).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const { violations, unexplained } = await scan(page);
    expect(violations).toEqual([]);
    expect(unexplained).toEqual([]);

    // The selected page stands out from the sidebar in every mode.
    const pill = await page.locator("nav[aria-label=Pages] > span[aria-hidden]").evaluate((el) => getComputedStyle(el).backgroundColor);
    const glass = await page.locator("nav[aria-label=Pages]").evaluate((el) => getComputedStyle(el).backgroundColor);
    expect(pill).not.toBe(glass);

    for (const shown of ["Home", "Tray popup (right-click on the tray icon)", "Components"]) {
      await page.locator(`[data-shown="${shown}"]`).screenshot({ path: info.outputPath(`${mode.name}-${shown.split(" ")[0].toLowerCase()}.png`) });
    }
  });
}

test("scrolled content goes under the page header's glass", async ({ page }) => {
  await page.goto("/?theme=light");
  const header = page.getByRole("heading", { name: "Home", level: 1 }).locator("..");
  await expect(header).not.toHaveClass(/\bglass\b/);
  await page.locator("main > div[tabindex='-1']").evaluate((el) => el.scrollTo(0, 200));
  await expect(header).toHaveClass(/\bglass\b/);
});

test("the dialog, menu and toast pass too, open", async ({ page }) => {
  await page.goto("/?theme=dark");
  await page.getByRole("button", { name: "Unlink shop.test…" }).click();
  await expect(page.getByRole("dialog", { name: "Unlink shop.test?" })).toBeVisible();
  let found = await scan(page);
  expect(found.violations).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Unlink shop.test?" })).toHaveCount(0);

  await page.locator('[data-shown="Components"]').getByRole("button", { name: "More for shop.test" }).click();
  await expect(page.getByRole("menu")).toBeVisible();
  found = await scan(page);
  expect(found.violations).toEqual([]);
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Show a toast" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Linked api.test" })).toBeVisible();
  found = await scan(page);
  expect(found.violations).toEqual([]);
});

test("high contrast: no glass, and the selected page and focus still show", async ({ page }, info) => {
  await page.emulateMedia({ forcedColors: "active" });
  await page.goto("/?theme=light");
  const nav = page.getByRole("navigation", { name: "Pages" });
  const outline = await nav.getByRole("button", { name: "Home" }).evaluate((el) => getComputedStyle(el).outlineStyle);
  expect(outline).not.toBe("none");
  const filter = await nav.evaluate((el) => getComputedStyle(el).backdropFilter);
  expect(filter).toBe("none");
  const sites = nav.getByRole("button", { name: /^Sites/ });
  await sites.focus();
  expect(await sites.evaluate((el) => getComputedStyle(el).outlineStyle)).not.toBe("none");
  await page.locator('[data-shown="Home"]').screenshot({ path: info.outputPath("high-contrast-home.png") });
});
