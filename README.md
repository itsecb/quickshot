# QuickShot

A fast, lightweight screenshot tool for Windows and macOS with a keyboard-driven markup editor and a
step-by-step guide builder. Built with Tauri v2 (Rust) and Svelte.

## Features

* **Capture**: region, window (click), full screen, repeat last region. Multi-monitor and mixed DPI aware.
  Frozen-frame overlay with crosshair, magnifier, pixel colour and coordinates, an animated border that
  glides between windows as you hover, Snagit-style detection of the parts inside windows (panels,
  toolbars, dialogs, buttons; scroll the wheel for a bigger or smaller part), and edges that snap to
  windows while you drag (hold `Alt` to
  disable). Optional shutter sound.
* **Floating thumbnail** after a copy or save: Edit, Pin, Copy again, or drag it straight into a chat.
* **Markup editor** with Shottr-style single-key tools:
  `A` arrow · `L` line · `R` rectangle · `O` ellipse · `P` pen · `H` highlighter · `T` text ·
  `N` numbered step badge · `B` blur/pixelate · `C` crop · `M` measure · `V` select ·
  `1`–`9` colours · `[` `]` width · `Ctrl+Z` undo · `Tab` next shape · arrows nudge
  Numbered steps come in any size, as circles, rounded or square, and can have a tail: drag from
  the thing the step is about to where the number goes. With any tool, clicking an existing shape
  of that kind picks it up to move, resize or restyle it (text and callouts open for editing).
* **Share**: `Ctrl+C` copy, `Ctrl+S` save with a filename pattern, `Ctrl+Shift+S` save as,
  drag the image straight into Teams/Outlook/browser/Explorer, `Ctrl+Shift+P` pin on top of everything.
  Optionally copy every capture to the clipboard while the editor opens.
* **Pins**: always-on-top floating screenshots. Mouse wheel (or `↑` `↓`) changes opacity,
  `Ctrl`+wheel zooms, double-click or `Esc` closes.
* **History** (`Ctrl+Shift+H`): every capture is kept locally, grouped by day. Search finds text *inside*
  screenshots (background OCR), app, window title, date, notes and `#tags`. Star (`S`) to keep forever,
  add notes (`N`). Reopen in the editor, pin, copy, save, or drag out. Retention is configurable
  (default 500 captures / 30 days; starred are exempt).
* **Before/after compare**: select two captures in History (or one, to compare with the previous
  capture of the same window) and press `C`. Auto-aligns shifted captures, then shows changed areas,
  side by side, a swipe slider, or blink.
* **Watch a region** (`Ctrl+Shift+W`): point at a dashboard tile, status page or log window and
  QuickShot checks it every few seconds, alerting when it changes or when text like *Failed*
  appears/disappears. Alerts show the change and open a before/after compare; both images go to
  History (`#watch`). Manage watches in the Watches window.
* **Step recorder** (`Ctrl+Shift+R`, Windows): every click captures the clicked window with the
  spot circled and a title like *Click the “Save” button in Notepad*; stop and the steps open as a
  new guide ready to export to Word/HTML/Markdown. No keystrokes are recorded.
* **Scrolling capture** (`Ctrl+Shift+L`, Windows): select the part of a page that scrolls and
  QuickShot scrolls it for you, stitching one tall image (sticky headers and footers appear once).
  Stops at the end of the page, at 20 000 px, or when you press Esc / Done.
* **GIF recording** (`Ctrl+Shift+G`, press again to stop): select an area, and after a 3-second
  countdown it records up to 60 s at 12 fps into the screenshots folder. The thumbnail plays it
  and copies it as a file (pastes into Teams/Outlook) or drags it anywhere. QuickShot's own bars
  never appear in recordings.
* **Per-app rules** (Settings → Rules): skip history, auto-redact, always copy, or also save to a folder
  based on the app or window title. Ships with a rule that keeps password managers out of history.
* **Scriptable capture**: `quickshot --out <file|folder>` captures with no UI and a real exit code
  (see below).
* **Delayed capture** (`Ctrl+Shift+5`, or tray: 3/5/10 s): time to open hover and right-click menus.
  The countdown never takes focus and is gone before the screen is captured.
* **Editor power tools**: Spotlight (`S`, dims everything but a box), Magnify (`Z`, a zoomed
  inset of a small area), Callout (`K`, a speech bubble pointing at something), and **Copy table**
  (`Ctrl+Shift+T`): OCR a grid, console listing or report into rows and columns that paste into
  Excel/Sheets/Outlook as real cells (Shift+click the button to save a CSV).
* **Auto-redact** (`Ctrl+Shift+X` in the editor): finds IPs, MACs, emails, GUIDs, SIDs, internal
  hostnames, passwords/keys/tokens (plus your own domains or patterns) and pixelates them in one
  undoable step.
* **Copy for ticket** (`Ctrl+Alt+C`): image plus a caption (window, app, time, PC) as rich clipboard
  content that pastes into ServiceNow, Jira, Teams, Outlook and email.
* **Beautify** (`Ctrl+B`): backdrop, padding, rounded corners and shadow for docs and slides.
* **QR codes & barcodes** (`Ctrl+Shift+Q`): select one on screen and its contents are copied.
* **OCR**: copy text from any region (`Ctrl+Shift+O`) using Windows OCR / macOS Vision.
* **Colour picker** (`Ctrl+Shift+C`) copies HEX or RGB.
* **Guides**: press `Ctrl+E` in the editor to add the image as the next step; write the instructions and
  export as Markdown, self-contained HTML or Word (.docx).
* Lives in the tray, optional autostart, all hotkeys configurable.

Default global hotkeys: `Ctrl+Shift+1` region · `Ctrl+Shift+2` window · `Ctrl+Shift+3` full screen ·
`Ctrl+Shift+4` repeat last · `Ctrl+Shift+O` OCR · `Ctrl+Shift+P` pin · `Ctrl+Shift+C` colour ·
`Ctrl+Shift+H` history · `Ctrl+Shift+5` delayed region · `Ctrl+Shift+Q` QR/barcode ·
`Ctrl+Shift+W` watch · `Ctrl+Shift+R` record steps · `Ctrl+Shift+L` scrolling capture ·
`Ctrl+Shift+G` GIF recording.

## Scriptable capture

Any command line with `--out` runs without the tray app or any window, writes the file, prints its
path and exits (0 saved, 2 bad arguments, 3 monitor/window not found, 4 capture or write failed).
`quickshot --help` lists every option.

The installer doesn't add QuickShot to `PATH`; in PowerShell (add it to your `$PROFILE` to keep it):

```powershell
Set-Alias quickshot "$env:LOCALAPPDATA\QuickShot\QuickShot.exe"   # default per-user install location
```

```powershell
# whole primary screen into a folder (unique name from the pattern)
quickshot --out C:\Evidence\ | Out-Null

# a specific window (title or app contains the text), as JPEG, also on the clipboard
quickshot --window "Grafana" --out D:\Dashboards\grafana.jpg --copy | Out-Null

# every monitor, or monitor 2, or an exact desktop rectangle, after a 5 s delay
quickshot --monitor all --out .\all.png | Out-Null
quickshot --monitor 2 --delay 5 --out .\m2.png | Out-Null
quickshot --rect 0,0,1280,720 --out .\corner.png | Out-Null

# exit code and printed path in a script
$p = Start-Process quickshot -ArgumentList '--out','C:\Evidence\' -Wait -PassThru -NoNewWindow
if ($p.ExitCode -ne 0) { throw "capture failed ($($p.ExitCode))" }
```

QuickShot is a Windows GUI app, so PowerShell and cmd don't wait for it on their own: pipe to
`Out-Null` or use `Start-Process -Wait` as above. Task Scheduler always waits. Example scheduled
snapshot every 15 minutes:

```powershell
$a = New-ScheduledTaskAction -Execute "$env:LOCALAPPDATA\QuickShot\QuickShot.exe" `
      -Argument '--window "NOC Dashboard" --out D:\NOC\ --name "noc {datetime}"'
$t = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 15)
Register-ScheduledTask -TaskName "QuickShot NOC snapshot" -Action $a -Trigger $t
```

## Building

Prerequisites: Node 22+, Rust stable (1.85+), and the Tauri v2 prerequisites for your OS
(https://v2.tauri.app/start/prerequisites/).

Linux (Ubuntu 24.04+; X11 sessions only for global hotkeys):

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev pkg-config libclang-dev libxcb1-dev libxrandr-dev \
  libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev
```

```bash
npm install
npm run tauri dev        # run
npm run tauri build      # installers in src-tauri/target/release/bundle
npm run check            # svelte-check
cargo test --manifest-path src-tauri/Cargo.toml
```

Command line: `quickshot --capture region|window|fullscreen|ocr|pin|color|qr [--delay N]`, `--settings`, `--guides`, `--history`, `--hidden`.

## Releases

Pushing a `v*` tag builds unsigned Windows (NSIS + MSI) and macOS (Apple Silicon + Intel) installers
as a draft GitHub release. Every push to `main` also uploads a Windows debug installer as a CI artifact.

* Windows: SmartScreen shows "unknown publisher" for unsigned installers. Choose *More info → Run anyway*.
* macOS: run `xattr -dr com.apple.quarantine /Applications/QuickShot.app` once, then allow
  Screen Recording in System Settings → Privacy & Security.

## Layout

```
src/            Svelte + TypeScript frontend (one entry per window: overlay, editor, pin, main, guide)
src-tauri/src/  Rust backend: capture (xcap), overlays, hotkeys, tray, clipboard, OCR, files
```
