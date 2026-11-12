# 0051. The desktop app's visual design: glass over Kiln's own backdrop

- **Status:** accepted; the user approved the mockup ([docs/design/desktop-mockup.html](../design/desktop-mockup.html)) on 2026-11-12. Since then the design values live in `apps/desktop/src/styles/tokens.css` (see "Learned building it")
- **Date:** 2026-11-12
- **Task:** P4-02 · Amends [ADR 0050](0050-desktop-app.md) §2 and §4

## Context

ADR 0050 left the visual design to P4-02. It said "Windows 11's look: Mica background,
Segoe UI Variable".

The user tried three designs:
- Kiln's own mockup.
- A Windows 11 (Fluent) design made with Claude.
- A "liquid glass" variant of that design, after Apple's current look.

They chose glass. Windows' own look felt too plain. They also asked for two things:
- **Don't look generated:** the glass prototype "looks like an AI-generated UI". It has
  coloured blobs behind everything, every surface is a card or a pill, there are big
  number tiles, and iOS blue is everywhere.
- **Herd's tray clicks:** left click opens the main window, right click opens a small
  window. They also want a setting to start straight in the tray.

The prototype also uses things a real Windows app can't or shouldn't:

- **It blurs a painted wallpaper.** In the app, CSS `backdrop-filter` only sees content
  inside the WebView, never the desktop behind the window.
- **Every button, row and card has its own blur.** That many layers stutter when
  scrolling.
- **Its window has a 26 px radius.** That fights the corners Windows rounds itself.
- **It has macOS window controls.** It also names Apple's font and loads Inter from
  Google Fonts.

## Decision

1. **The window paints its own backdrop.** The window is opaque, with no Mica, no Acrylic
   and no transparent window.
   - **The backdrop is a quiet surface:** warm off-white in light mode, warm charcoal in
     dark. A faint ember glow in one corner is the only colour.
   - **Same everywhere:** it looks the same on every machine and wallpaper.
   - **No OS-material lag:** it avoids Acrylic's drag lag and Mica's faint wallpaper tint.
   - **It's not see-through.** The user chose this over Windows' materials.
2. **Glass is only for what floats above content:**
   - the sidebar;
   - each page's header, which content scrolls under;
   - dialogs, menus and toasts;
   - the tray popup.

   The rules for glass:
   - **A floating layer** is blur, saturation, a translucent fill, a one-pixel light rim on
     top and a soft shadow.
   - **Lists are rows with dividers,** not a card per item. Content sits on the backdrop.
   - **At most three blurred layers show at once.** Blur is never nested.
   - **Everything else is a flat translucent fill:** buttons, rows and inputs have no blur
     of their own.
3. **The window behaves like a Windows window:**
   - **Its own title bar.** It holds Kiln's mark and a "Go to…" command field (Ctrl+K).
   - **Windows' caption buttons** (minimise, maximise, close) stay on the right.
   - **Windows draws the corners and shadow.** Large radii are only on panels inside.
   - **P4-03 checks** that hovering maximise still shows Snap Layouts. If it can't, the
     title bar falls back to Windows' own.
4. **Kiln's identity, not a template's:**
   - **Primary actions are "ink":** near-black in light mode, near-white in dark. The one
     main action on a screen stands out, with no default blue.
   - **Ember (a kiln's orange) is the brand colour.** It is used for the logo, the selected
     page's icon and focus rings. It never means a status.
   - **Statuses are green (running), amber (needs attention) and red (failed).** Each
     always has a word next to it, never colour alone.
   - **No number tiles.** The home page starts with one sentence about what matters
     ("Everything's running", "Redis stopped at 14:02"), with the action next to it.
   - **Names stand out; details are quiet.** Site and service names are prominent. Ports,
     versions and paths are smaller, in tabular figures or monospace.
   - **The text is specific:** it names the real site, port or file.
5. **Type and icons are bundled, with no network calls:**
   - **Inter** (SIL OFL) for text, with tabular figures for numbers.
   - **Cascadia Mono** (SIL OFL) for paths, ports and logs.
   - **Fluent System Icons** (MIT) for icons, plus Kiln's own logo and tray icons.
   - **Nothing of Apple's** (SF fonts, SF Symbols) or Herd's. Glass is a public design
     style; Kiln copies the idea, not anyone's assets.
6. **The tray** (amends ADR 0050 §2):
   - **Left click** opens the main window, or brings it to the front.
   - **Right click** opens the tray popup. It is a second, small, frameless window placed
     against the tray icon. It shows:
     - Kiln's state, and anything that needs attention;
     - the sites to open;
     - the services, with start and stop;
     - "Open Kiln", "Stop all" and "Quit".
     - It closes on Esc or a click elsewhere, like Windows' own flyouts.
     - Its surface is Kiln's backdrop, because glass over the desktop, which can be any
       colour, can't keep its text readable ([I-89](../issues.md#i-89)).
     - Because it never moves, Windows' Acrylic could sit behind it, like Windows' own
       flyouts: Acrylic's lag only shows when a window is dragged. P4-03 may try it, but
       only with a fill opaque enough to pass the contrast check over a light and a dark
       desktop.
   - **The keyboard works the same:** Enter on the focused tray icon acts as a left click,
     and the menu key as a right click.
   - **A native menu is the fallback.** It shows the same items, as plain text, in Windows'
     high-contrast mode or if the popup can't open.
   - **The icon shows the state:** running, needs attention (a dot), or stopped.
7. **Starting and closing:**
   - **A setting, "When Kiln starts":** open the window, or stay in the tray.
   - **Starting at login always stays in the tray.**
   - **Closing the window hides it to the tray.** The first time, one notification says
     Kiln is still running there.
   - **One instance:** starting Kiln again shows the open window.
   - **The window remembers** its size, position and last page.
8. **Accessibility is checked, not assumed:**
   - **Contrast** is measured on CI in every mode, against the glass fills, at WCAG AA.
   - **"Transparency effects" off** in Windows makes every glass layer solid. The Rust side
     reads the setting and passes it to the page, because WebView2 may not report
     `prefers-reduced-transparency`.
   - **High contrast** (`forced-colors`) drops glass and uses the system's colours.
   - **Reduced motion** turns animation off.
   - **Full keyboard use,** with visible focus and screen-reader labels.
9. **Motion is short and has a reason:**
   - the selection pill slides to the new page;
   - the tray popup rises from the taskbar;
   - pressed controls give slightly;
   - dialogs open from the control that opened them;
   - each animation lasts at most 200 ms.

   Glass that bends light at its edges (an SVG filter in `backdrop-filter`) is left out of
   the first version.
10. **One source of design values:**
    - colours, radii, spacing, shadows and glass are CSS custom properties in
      `apps/desktop`, in light and dark;
    - Tailwind reads those values;
    - components use only those names.

## Consequences

- **Glass where it shows:** the design keeps the glass look the user wanted, where content
  moves under it, and stays fast because few layers blur.
- **Some HTML effects are gone:** the app won't show the desktop through the window, as
  the prototype's painted wallpaper did.
- **A second window:** the tray popup is a window of its own, with placement and closing
  logic. That is in P4-03.
- **Fonts bundled:** about 1 MB of fonts are added to the app.
- **Before the pages:** the mockup is reviewed with the user before any page is built. The
  tokens and the base components follow, with screenshot and contrast checks on CI (P4-13).

## Learned building it

These came from the user's review of the mockup and from the checks. Each is now a rule.

- **The selected item has its own solid colour.** With transparency off, the selected page
  was the same colour as the sidebar (the user saw it). `--selected-solid` fixes it, and
  the contrast test checks the selected item stands out in every mode.
- **The page header sits beside the scroller, not inside it.** Content fades out under the
  header, so nothing peeks round its corners. A fade on the scroller would also fade a
  header inside it.
- **The window has a minimum size, 880 × 560.** Below that, the title bar and the rows
  squeeze.
- **Measured contrast moved some colours.** Faint text, amber, the "stopped" ring, the light
  sidebar glass and dark red changed from the mockup to reach WCAG AA on every fill, glow
  included. `tokens.css` is the source now, not the mockup.
- **The tray popup sits on Kiln's backdrop, not on glass.** Over a dark desktop, light glass
  left its quieter text near 3:1 ([I-89](../issues.md#i-89)).
- **axe can't measure text on glass.** It reports it as "incomplete", so contrast is
  measured from the design values instead ([I-88](../issues.md#i-88)).
- **Stop and start are filled icons.** An outlined square next to "Running" read as an empty
  checkbox.

## Alternatives considered

- **Windows' Mica or Acrylic behind a transparent window.** It is truly see-through, but
  Mica is only a faint tint of the wallpaper, and Acrylic has lagged when dragging on recent
  Windows 11 builds. The user chose Kiln's own backdrop.
- **Windows 11's own Fluent look.** It is the most native, and the user found it too plain.
- **A native tray menu only.** It can't show a status, a port and a start button on one
  row, so it would be a weaker copy of the main window. It stays as the fallback.
- **The prototype as it is.** Its blur on every element, painted wallpaper, macOS
  controls, Apple font and generic look are what this ADR replaces.
