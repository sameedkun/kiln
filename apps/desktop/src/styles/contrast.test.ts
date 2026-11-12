// @vitest-environment node
//
// Contrast is measured, not assumed (ADR 0051 §8). Browser checkers can't measure text on
// translucent glass (they report it as "incomplete"), so this test does it from the
// design values: every fill is alpha-blended over Kiln's backdrop, with and without the
// ember glow at its strongest, then each text colour used on it must reach WCAG AA (4.5:1),
// and each dot, icon or focus ring 3:1. In light, dark, and with transparency off.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

type Rgba = [number, number, number, number];
type Tokens = Record<string, string>;

const css = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");

function block(selector: string): Tokens {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no ${selector} block in tokens.css`);
  const body = css.slice(start, css.indexOf("}", start));
  const out: Tokens = {};
  for (const [, name, value] of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) out[name] = value.trim();
  return out;
}

const light = block(":root");
const dark = { ...light, ...block(':root[data-theme="dark"]') };
const solidOverrides = block(":root[data-solid]");

function resolve(tokens: Tokens, name: string): string {
  const value = tokens[name];
  if (value === undefined) throw new Error(`${name} isn't defined`);
  const ref = value.match(/^var\((--[\w-]+)\)$/);
  return ref ? resolve(tokens, ref[1]) : value;
}

function parse(value: string): Rgba {
  const hex = value.match(/^#([0-9a-f]{6})$/i);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  const rgba = value.match(/^rgba?\(([^)]+)\)$/);
  if (rgba) {
    const [r, g, b, a = 1] = rgba[1].split(",").map(Number);
    return [r, g, b, a];
  }
  throw new Error(`can't read the colour ${value}`);
}

/** `top` blended over an opaque `under`. */
function over(top: Rgba, under: Rgba): Rgba {
  const a = top[3];
  return [0, 1, 2].map((i) => top[i] * a + under[i] * (1 - a)).concat(1) as Rgba;
}

function luminance([r, g, b]: Rgba): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function ratio(a: Rgba, b: Rgba): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

interface Mode {
  name: string;
  tokens: Tokens;
}
const modes: Mode[] = [
  { name: "light", tokens: light },
  { name: "dark", tokens: dark },
  { name: "light, transparency off", tokens: { ...light, ...solidOverrides } },
  { name: "dark, transparency off", tokens: { ...dark, ...solidOverrides } },
];

/** The surfaces text sits on, each over the plain backdrop and over the glow. */
function surfaces(t: Tokens): Record<string, Rgba> {
  const c = (name: string) => parse(resolve(t, name));
  const out: Record<string, Rgba> = {};
  for (const [where, base] of [
    ["", c("--bg")],
    [" (glow)", over(c("--bg-glow"), c("--bg"))],
  ] as const) {
    const glass = over(c("--glass"), base);
    out[`backdrop${where}`] = base;
    out[`row hover${where}`] = over(c("--hover"), base);
    out[`glass${where}`] = glass;
    out[`selected item on glass${where}`] = over(c("--glass-strong"), glass);
    out[`hover on glass${where}`] = over(c("--hover"), glass);
    out[`attention notice${where}`] = over(c("--warn-soft"), base);
    out[`attention row on glass${where}`] = over(c("--warn-soft"), glass);
    out[`failed notice${where}`] = over(c("--bad-soft"), base);
  }
  return out;
}

const text = ["--text", "--text-2", "--text-3"];
/** Status words shown as text, and where they appear. */
const statusText: Record<string, (surface: string) => boolean> = {
  "--warn": (s) => !s.startsWith("failed"),
  "--bad": (s) => !s.startsWith("attention"),
};
/** Marks that aren't text: dots, the selected page's icon, focus rings. */
const marks = ["--ember", "--focus", "--ok", "--warn", "--bad", "--off"];

describe.each(modes)("contrast, $name", ({ tokens }) => {
  const c = (name: string) => parse(resolve(tokens, name));
  const all = surfaces(tokens);

  it("text reaches 4.5:1 on every surface", () => {
    const low: string[] = [];
    for (const [surface, bg] of Object.entries(all)) {
      for (const name of text) {
        const r = ratio(over(c(name), bg), bg);
        if (r < 4.5) low.push(`${name} on ${surface}: ${r.toFixed(2)}`);
      }
      for (const [name, used] of Object.entries(statusText)) {
        if (!used(surface)) continue;
        const r = ratio(over(c(name), bg), bg);
        if (r < 4.5) low.push(`${name} on ${surface}: ${r.toFixed(2)}`);
      }
    }
    expect(low).toEqual([]);
  });

  it("dots, icons and focus rings reach 3:1", () => {
    const low: string[] = [];
    for (const [surface, bg] of Object.entries(all)) {
      for (const name of marks) {
        const r = ratio(over(c(name), bg), bg);
        if (r < 3) low.push(`${name} on ${surface}: ${r.toFixed(2)}`);
      }
    }
    expect(low).toEqual([]);
  });

  it("the main action's text reaches 4.5:1 on ink", () => {
    expect(ratio(c("--on-ink"), c("--ink"))).toBeGreaterThanOrEqual(4.5);
  });

  it("the selected page stands out from the sidebar", () => {
    // Not a WCAG rule, but the bug the user found: with transparency off, the selected
    // pill was the same colour as the sidebar.
    const glass = all["glass"];
    const selected = all["selected item on glass"];
    expect(ratio(glass, selected)).toBeGreaterThan(1.08);
  });
});
