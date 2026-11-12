// The one place that sets the page's appearance (ADR 0051 §8): `data-theme` and `data-solid`
// on <html>. Windows decides both by default; the Rust side will pass Windows' own
// "Transparency effects" setting in P4-03, since WebView2 may not report it to CSS.

export type Theme = "light" | "dark";

export interface Appearance {
  /** Light or dark, or follow Windows. */
  theme: Theme | "system";
  /** Glass turned solid, or follow Windows' "Transparency effects" setting. */
  solid: boolean | "system";
}

let current: Appearance = { theme: "system", solid: "system" };

function query(media: string): MediaQueryList | undefined {
  return typeof window.matchMedia === "function" ? window.matchMedia(media) : undefined;
}
const prefersDark = query("(prefers-color-scheme: dark)");
const prefersSolid = query("(prefers-reduced-transparency: reduce)");

function apply(): void {
  const root = document.documentElement;
  const theme = current.theme === "system" ? (prefersDark?.matches ? "dark" : "light") : current.theme;
  const solid = current.solid === "system" ? Boolean(prefersSolid?.matches) : current.solid;
  root.dataset.theme = theme;
  root.toggleAttribute("data-solid", solid);
}

/** Changes the appearance; anything not given stays as it was. */
export function setAppearance(next: Partial<Appearance>): void {
  current = { ...current, ...next };
  apply();
}

export function appearance(): Appearance {
  return { ...current };
}

/** Applies the appearance now and follows Windows' changes from then on. */
export function startAppearance(initial?: Partial<Appearance>): void {
  prefersDark?.addEventListener("change", apply);
  prefersSolid?.addEventListener("change", apply);
  setAppearance(initial ?? {});
}
