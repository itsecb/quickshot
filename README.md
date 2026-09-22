# QuickShot

A fast, lightweight screenshot tool for Windows and macOS with a keyboard-driven markup editor and a
step-by-step guide builder. Built with Tauri v2 (Rust) and Svelte.

## Features

* **Capture**: region, window (click), full screen, repeat last region. Multi-monitor and mixed DPI aware.
  Frozen-frame overlay with crosshair, magnifier, pixel colour and coordinates.
* **Markup editor** with Shottr-style single-key tools:
  `A` arrow · `L` line · `R` rectangle · `O` ellipse · `P` pen · `H` highlighter · `T` text ·
  `N` numbered step badge · `B` blur/pixelate · `C` crop · `M` measure · `V` select ·
  `1`–`9` colours · `[` `]` width · `Ctrl+Z` undo · `Tab` next shape · arrows nudge
* **Share**: `Ctrl+C` copy, `Ctrl+S` save with a filename pattern, `Ctrl+Shift+S` save as,
  drag the image straight into Teams/Outlook/browser/Explorer, `Ctrl+Shift+P` pin on top of everything.
  Optionally copy every capture to the clipboard while the editor opens.
* **Pins**: always-on-top floating screenshots. Mouse wheel (or `↑` `↓`) changes opacity,
  `Ctrl`+wheel zooms, double-click or `Esc` closes.
* **History** (`Ctrl+Shift+H`): every capture is kept locally, grouped by day. Search finds text *inside*
  screenshots (background OCR), app, window title, date, notes and `#tags`. Star (`S`) to keep forever,
  add notes (`N`). Reopen in the editor, pin, copy, save, or drag out. Retention is configurable
  (default 500 captures / 30 days; starred are exempt).
* **Delayed capture** (`Ctrl+Shift+5`, or tray: 3/5/10 s): time to open hover and right-click menus.
  The countdown never takes focus and is gone before the screen is captured.
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
`Ctrl+Shift+H` history · `Ctrl+Shift+5` delayed region · `Ctrl+Shift+Q` QR/barcode.

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
