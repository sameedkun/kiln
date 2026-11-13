# By-hand check: the desktop app (P4-03, ADR 0054 §7)

CI builds the app and runs its unit tests. What it can't check needs a real Windows desktop:
- the tray;
- the windows;
- Windows' own prompts;
- signing in.

Run this in a **normal terminal** (Windows Terminal or PowerShell from the Start menu), not
from the Claude app: commands started there see a different AppData folder.

Note for each step: OK, or what happened instead. Paste the notes back.

## 1. Build

```powershell
cd D:\Code\Rea\laravel-herd\kiln
cargo build --release --workspace --bins
cd apps\desktop
npm ci
npm run build
cd src-tauri
cargo build --release
cd ..\..\..
```

`target\release` now holds `kiln.exe`, `kilnd.exe` and `kiln-desktop.exe`. So that there is
something to see, add Mailpit:

```powershell
.\target\release\kiln.exe service add mailpit
```

## 2. Start and the tray

1. Start `target\release\kiln-desktop.exe` (double-click it in Explorer). **Expected:**
   - The window opens.
   - "Starting Kiln's daemon…" shows for a moment, then Home lists mailpit-1 as running.
2. Find Kiln's icon in the tray: an orange square with a white kiln. If it's in the overflow
   (the ^ arrow), drag it onto the taskbar.
3. **Left click** the icon: the window comes to the front.
4. **Right click** the icon. **Expected:**
   - The popup opens just above the icon, inside the screen.
   - It shows "1 of 1 services running", mailpit-1 with its port, and Open Kiln / Stop all /
     Quit.
5. With the popup open:
   - **right click the icon again:** the popup closes and does *not* reopen;
   - open it, then **click elsewhere** on the desktop: it closes;
   - open it, then **press Esc:** it closes.
6. In the popup, **stop mailpit-1** (the square button). **Expected:**
   - A note says it stopped.
   - In the window, Home shows mailpit-1 as stopped.
   - **Start** it again from Home's row.

## 3. Closing, a second start, quitting

1. Click the window's **×**. **Expected:**
   - A dialog: "Kiln keeps running".
   - Click Got it: the window hides, and the tray icon stays.
2. Left click the tray icon: the window is back. Click **×** again: it hides at once, with no
   dialog.
3. Start `kiln-desktop.exe` again while it runs. **Expected:** the open window comes to the
   front, and there is still only one tray icon.
4. In the popup, click **Quit**. **Expected:**
   - The app and its tray icon go away.
   - `.\target\release\kiln.exe service list` still shows mailpit-1 running.
   - `.\target\release\kiln.exe daemon status` still says the daemon runs.

## 4. Window and look

1. Start the app again, then hover the **maximise** button (□). Do Windows' Snap Layouts
   show? (They may not with Kiln's own title bar: note what you see.)
2. Drag the window by its title bar, and double-click the title bar. Does it move? Does it
   maximise?
3. Windows Settings → Personalization → Colors:
   - switch between **Light** and **Dark**: does the app follow at once?
   - turn **Transparency effects** off: does the sidebar's glass turn solid?

## 5. Starting at sign-in

1. In the app, open **Settings** and turn on **Start Kiln when I sign in**. **Expected:**
   - A dialog shows the change: the `Run\Kiln` value set to
     `"...\kiln-desktop.exe" --background`.
   - Confirm: a note says it's on.
2. `.\target\release\kiln.exe daemon status` should say "Starts at login: Kiln's app".
3. **Sign out and back in** (or restart). **Expected:**
   - Kiln's tray icon appears, with no window.
   - `kiln daemon status` says the daemon runs.
4. Turn the switch off again in Settings (a dialog, then Confirm).

## 6. A Windows prompt from the app (ADR 0053 §5)

The app's jobs run without a window, so check that Windows' certificate prompt still comes to
the front. In the terminal:

```powershell
.\target\release\kiln.exe ca init
.\target\release\kiln.exe daemon call jobs.start '{\"args\":[\"ca\",\"trust\"],\"mode\":\"apply\"}'
```

Expected: Windows' "Security Warning" about installing a certificate appears, in front or
flashing on the taskbar. Note which. Click **Yes**. Then, with the number `jobs.start`
printed (`"job": 7` here; use yours):

```powershell
.\target\release\kiln.exe daemon call jobs.get '{\"job\":7}'
```

`"ok": true` near the end means it worked.

## 7. Clean up

```powershell
.\target\release\kiln.exe uninstall --yes --delete-data
```

Expected:
- "Stopped Kiln's app" (if it ran), then the daemon and the rest.
- Windows asks to remove the certificate: click Yes.
- Afterwards neither `%LOCALAPPDATA%\Kiln` nor `%APPDATA%\Kiln` exists.
- `HKCU\...\Run` has no `Kiln` value.
