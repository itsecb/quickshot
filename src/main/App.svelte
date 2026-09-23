<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { appPaths, getSettings, hideMain, historyList, openWindow, setSettings, triggerCapture } from "$lib/ipc";
  import type { AppPaths, CaptureMode, Rule, Settings } from "$lib/types";
  import { acceleratorFromEvent } from "./hotkey";

  type Page = "home" | "capture" | "output" | "rules" | "editor" | "about";
  let page = $state<Page>("home");
  let settings = $state<Settings | null>(null);
  let paths = $state<AppPaths | null>(null);
  let recording = $state<keyof Settings["hotkeys"] | null>(null);
  let conflicts = $state<string[]>([]);
  let saved = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let knownApps = $state<string[]>([]);

  async function openRules() {
    page = "rules";
    try {
      const items = await historyList();
      knownApps = [...new Set(items.map((i) => i.source.appName).filter(Boolean))].sort();
    } catch {
      knownApps = [];
    }
  }

  function addRule() {
    if (!settings) return;
    const rule: Rule = {
      enabled: true,
      name: "",
      app: "",
      title: "",
      skipHistory: false,
      autoRedact: true,
      autoCopy: false,
      saveDir: null,
    };
    settings.rules = [...settings.rules, rule];
  }

  async function pickRuleFolder(i: number) {
    if (!settings) return;
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir === "string") settings.rules[i]!.saveDir = dir;
  }

  const TOOL_NAMES: Record<string, string> = {
    select: "Select / move",
    arrow: "Arrow",
    line: "Line",
    rect: "Rectangle",
    ellipse: "Ellipse",
    pen: "Pen",
    text: "Text",
    highlighter: "Highlighter",
    blur: "Blur / pixelate",
    badge: "Numbered step",
    crop: "Crop",
    measure: "Measure",
  };

  const HOTKEY_LABELS: { key: keyof Settings["hotkeys"]; label: string; mode: CaptureMode | "history" | "delayed" }[] = [
    { key: "region", label: "Capture region", mode: "region" },
    { key: "window", label: "Capture window", mode: "window" },
    { key: "fullscreen", label: "Capture full screen", mode: "fullscreen" },
    { key: "repeatLast", label: "Repeat last region", mode: "repeatLast" },
    { key: "delayedRegion", label: "Capture region after delay", mode: "delayed" },
    { key: "ocr", label: "Copy text (OCR)", mode: "ocr" },
    { key: "pin", label: "Pin region to screen", mode: "pin" },
    { key: "color", label: "Pick a colour", mode: "color" },
    { key: "qr", label: "Read QR code / barcode", mode: "qr" },
    { key: "history", label: "Open history", mode: "history" },
  ];

  onMount(async () => {
    try {
      settings = await getSettings();
      paths = await appPaths();
    } catch (e) {
      error = String(e);
    }
  });

  async function save() {
    if (!settings) return;
    saving = true;
    try {
      const r = await setSettings($state.snapshot(settings));
      conflicts = r.hotkeyConflicts;
      paths = await appPaths();
      saved = true;
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function onRecordKey(e: KeyboardEvent) {
    if (!recording || !settings) return;
    e.preventDefault();
    if (e.key === "Escape") {
      recording = null;
      return;
    }
    if (e.key === "Backspace" || e.key === "Delete") {
      settings.hotkeys[recording] = "";
      recording = null;
      return;
    }
    const acc = acceleratorFromEvent(e);
    if (acc) {
      settings.hotkeys[recording] = acc;
      recording = null;
    }
  }

  async function pickFolder(target: "saveDir" | "guidesDir") {
    if (!settings) return;
    const dir = await openDialog({ directory: true, multiple: false, defaultPath: target === "saveDir" ? paths?.saveDir : paths?.guidesDir });
    if (typeof dir === "string") settings[target] = dir;
  }

  async function capture(mode: CaptureMode | "history" | "delayed") {
    if (mode === "history") return openWindow("history");
    await hideMain();
    if (mode === "delayed") return void triggerCapture("region", settings?.captureDelaySecs ?? 3);
    setTimeout(() => void triggerCapture(mode), 150);
  }
</script>

<svelte:window onkeydown={onRecordKey} />

{#if error}
  <div style="padding:20px;color:var(--danger)">{error}</div>
{:else if settings}
  <div class="app">
    <nav>
      <div class="brand">QuickShot</div>
      <button class:active={page === "home"} onclick={() => (page = "home")}>Home</button>
      <button class:active={page === "capture"} onclick={() => (page = "capture")}>Hotkeys</button>
      <button class:active={page === "output"} onclick={() => (page = "output")}>Output</button>
      <button class:active={page === "rules"} onclick={openRules}>Rules</button>
      <button class:active={page === "editor"} onclick={() => (page = "editor")}>Editor</button>
      <button class:active={page === "about"} onclick={() => (page = "about")}>About</button>
      <div class="bottom">
        <button onclick={() => openWindow("history")}>History…</button>
        <button onclick={() => openWindow("guide")}>Guides…</button>
        <button onclick={hideMain}>Hide to tray</button>
      </div>
    </nav>

    <main>
      {#if page === "home"}
        <h2>Capture</h2>
        <p class="muted">QuickShot lives in the tray. Use the hotkeys from anywhere, or start a capture here.</p>
        <div class="quick">
          {#each HOTKEY_LABELS as h (h.key)}
            <button onclick={() => capture(h.mode)}>
              <span>{h.label}</span>
              {#if settings.hotkeys[h.key]}<kbd>{settings.hotkeys[h.key]}</kbd>{/if}
            </button>
          {/each}
        </div>
        <h3>In the capture overlay</h3>
        <p class="muted">
          Drag for a region · click a window · <kbd>Enter</kbd> whole screen · <kbd>L</kbd> last region · arrows nudge the cursor ·
          <kbd>Esc</kbd> cancels. The magnifier shows the pixel colour and coordinates.
        </p>
        <h3>In the editor</h3>
        <p class="muted">
          Single keys switch tools ({Object.entries(settings.editor.shortcuts)
            .map(([t, k]) => `${k.toUpperCase()} ${TOOL_NAMES[t] ?? t}`)
            .join(", ")}). <kbd>1</kbd>–<kbd>9</kbd> pick colours, <kbd>[</kbd> <kbd>]</kbd> change width,
          <kbd>Ctrl+C</kbd> copy, <kbd>Ctrl+S</kbd> save, <kbd>Ctrl+E</kbd> add to guide, <kbd>Ctrl+Shift+P</kbd> pin, <kbd>Esc</kbd> deselect / close.
        </p>
      {:else if page === "capture"}
        <h2>Global hotkeys</h2>
        <p class="muted">Click a field and press the combination. <kbd>Backspace</kbd> clears, <kbd>Esc</kbd> cancels.</p>
        {#each HOTKEY_LABELS as h (h.key)}
          <div class="row">
            <label for={"hk-" + h.key}>{h.label}</label>
            <div class="inline">
              <input
                id={"hk-" + h.key}
                type="text"
                class="hotkey"
                class:recording={recording === h.key}
                readonly
                value={recording === h.key ? "Press keys…" : settings.hotkeys[h.key] || "(disabled)"}
                onfocus={() => (recording = h.key)}
                onblur={() => (recording = null)}
              />
            </div>
          </div>
        {/each}
        {#if conflicts.length}
          <div class="conflict">Some hotkeys could not be registered (already used by another app): {conflicts.join("; ")}</div>
        {/if}
        <div class="row">
          <label for="delay">Delay for delayed capture</label>
          <div class="inline">
            <input id="delay" type="number" min="1" max="60" bind:value={settings.captureDelaySecs} style="width:70px" /> seconds
          </div>
          <div class="hint">Time to open a hover or right-click menu before the screen is captured. Press the hotkey again to cancel.</div>
        </div>
        <h3>Behaviour</h3>
        <div class="row">
          <label for="mag">Magnifier in overlay</label>
          <input id="mag" type="checkbox" bind:checked={settings.showMagnifier} />
        </div>
        <div class="row">
          <label for="sound">Shutter sound</label>
          <input id="sound" type="checkbox" bind:checked={settings.playSound} />
        </div>
        <div class="row">
          <label for="auto">Start with the system</label>
          <input id="auto" type="checkbox" bind:checked={settings.autostart} />
        </div>
        <div class="row">
          <label for="ocrlang">OCR language</label>
          <input id="ocrlang" type="text" placeholder="system default (e.g. en-US)" bind:value={settings.ocrLanguage} />
          <div class="hint">Windows uses the installed language packs' OCR feature; macOS uses Vision.</div>
        </div>
      {:else if page === "output"}
        <h2>Output</h2>
        <div class="row">
          <label for="after">After a capture</label>
          <select id="after" bind:value={settings.afterCapture}>
            <option value="editor">Open the editor</option>
            <option value="editorAndCopy">Open the editor and copy</option>
            <option value="copy">Copy to clipboard</option>
            <option value="save">Save to folder</option>
            <option value="copyAndSave">Copy and save</option>
          </select>
        </div>
        <div class="row">
          <label for="dir">Save folder</label>
          <div class="inline">
            <input id="dir" type="text" placeholder={paths?.saveDir} bind:value={settings.saveDir} />
            <button onclick={() => pickFolder("saveDir")}>Browse…</button>
            <button onclick={() => paths && openPath(paths.saveDir)}>Open</button>
          </div>
        </div>
        <div class="row">
          <label for="pattern">File name pattern</label>
          <input id="pattern" type="text" bind:value={settings.filePattern} />
          <div class="hint">Tokens: <code>{"{date}"}</code> <code>{"{time}"}</code> <code>{"{datetime}"}</code> <code>{"{app}"}</code> <code>{"{title}"}</code> <code>{"{w}"}</code> <code>{"{h}"}</code></div>
        </div>
        <div class="row">
          <label for="fmt">Format</label>
          <div class="inline">
            <select id="fmt" bind:value={settings.imageFormat}>
              <option value="png">PNG (lossless)</option>
              <option value="jpeg">JPEG</option>
            </select>
            {#if settings.imageFormat === "jpeg"}
              <input type="number" min="10" max="100" bind:value={settings.jpegQuality} style="width:70px" /> quality
            {/if}
          </div>
        </div>
        <div class="row">
          <label for="cos">Also copy when saving</label>
          <input id="cos" type="checkbox" bind:checked={settings.copyOnSave} />
        </div>
        <div class="row">
          <label for="thumb">Floating thumbnail</label>
          <input id="thumb" type="checkbox" bind:checked={settings.showThumbnail} />
          <div class="hint">After a copy or save, a preview slides into the corner with Edit, Pin, Copy and drag-out. Off: a Windows notification instead.</div>
        </div>
        <div class="row">
          <label for="gdir">Guides folder</label>
          <div class="inline">
            <input id="gdir" type="text" placeholder={paths?.guidesDir} bind:value={settings.guidesDir} />
            <button onclick={() => pickFolder("guidesDir")}>Browse…</button>
          </div>
        </div>
        <h3>Sharing</h3>
        <div class="row">
          <label for="caption">"Copy for ticket" caption</label>
          <input id="caption" type="text" bind:value={settings.ticketCaption} />
          <div class="hint">
            <kbd>Ctrl+Alt+C</kbd> in the editor copies the image with this caption above it. Tokens: <code>{"{title}"}</code> <code>{"{app}"}</code> <code>{"{datetime}"}</code>
            <code>{"{date}"}</code> <code>{"{time}"}</code> <code>{"{host}"}</code> <code>{"{user}"}</code> <code>{"{w}"}</code> <code>{"{h}"}</code>. Empty parts are left out.
          </div>
        </div>
        <div class="row">
          <label for="redact">Also redact</label>
          <textarea
            id="redact"
            rows="3"
            placeholder={"one per line, e.g.\ncontoso.com\nSRV-\nre:INC\\d{6}"}
            value={settings.redactPatterns.join("\n")}
            oninput={(e) => settings && (settings.redactPatterns = (e.currentTarget as HTMLTextAreaElement).value.split("\n").map((l) => l.trim()).filter(Boolean))}
          ></textarea>
          <div class="hint">
            Redact (<kbd>Ctrl+Shift+X</kbd> in the editor) always finds IPs, MACs, emails, GUIDs, SIDs, internal hostnames and passwords/keys/tokens.
            Add your domains or server prefixes here (the whole word containing them is hidden), or <code>re:</code> + a regular expression.
          </div>
        </div>
        <h3>History</h3>
        <div class="row">
          <label for="hist">Keep captures in history</label>
          <div class="inline">
            <input id="hist" type="checkbox" bind:checked={settings.historyEnabled} />
            <button onclick={() => openWindow("history")}>Open history</button>
          </div>
        </div>
        <div class="row">
          <label for="hocr">Make screenshot text searchable</label>
          <input id="hocr" type="checkbox" bind:checked={settings.historyOcr} />
          <div class="hint">Reads the text in each capture in the background (Windows OCR) so History can find error codes, hostnames and so on.</div>
        </div>
        <div class="row">
          <label for="hmax">Keep at most</label>
          <div class="inline">
            <input id="hmax" type="number" min="0" max="100000" bind:value={settings.historyMaxItems} style="width:90px" /> screenshots
          </div>
        </div>
        <div class="row">
          <label for="hdays">Delete after</label>
          <div class="inline">
            <input id="hdays" type="number" min="0" max="3650" bind:value={settings.historyMaxDays} style="width:90px" /> days
          </div>
          <div class="hint">0 means no limit. Starred screenshots are always kept. History is stored only on this PC, separate from your save folder.</div>
        </div>
      {:else if page === "rules"}
        <h2>Per-app rules</h2>
        <p class="muted">
          When a capture comes from a matching app or window, these apply on top of your normal settings. Matching is by
          part of the app name or window title, case-insensitive; separate alternatives with commas.
        </p>
        <datalist id="known-apps">
          {#each knownApps as a (a)}<option value={a}></option>{/each}
        </datalist>
        {#each settings.rules as rule, i (i)}
          <div class="rule" class:off={!rule.enabled}>
            <div class="rule-head">
              <input type="checkbox" bind:checked={rule.enabled} title="Enabled" />
              <input type="text" class="rule-name" placeholder="Rule name" bind:value={rule.name} />
              <button class="danger" onclick={() => settings && (settings.rules = settings.rules.filter((_, j) => j !== i))}>Delete</button>
            </div>
            <div class="row">
              <label for={"rule-app-" + i}>App name contains</label>
              <input id={"rule-app-" + i} type="text" list="known-apps" placeholder="e.g. KeePass, mmc, Chrome" bind:value={rule.app} />
            </div>
            <div class="row">
              <label for={"rule-title-" + i}>Window title contains</label>
              <input id={"rule-title-" + i} type="text" placeholder="e.g. Azure, Grafana (optional)" bind:value={rule.title} />
            </div>
            <div class="checks">
              <label><input type="checkbox" bind:checked={rule.skipHistory} /> Don't keep in history</label>
              <label><input type="checkbox" bind:checked={rule.autoRedact} /> Auto-redact</label>
              <label><input type="checkbox" bind:checked={rule.autoCopy} /> Always copy</label>
            </div>
            <div class="row">
              <label for={"rule-dir-" + i}>Also save to</label>
              <div class="inline">
                <input id={"rule-dir-" + i} type="text" placeholder="(no extra folder)" bind:value={rule.saveDir} />
                <button onclick={() => pickRuleFolder(i)}>Browse…</button>
              </div>
            </div>
          </div>
        {/each}
        <button onclick={addRule}>+ Add rule</button>
        <p class="muted">
          Auto-redact pixelates what the Redact tool finds before the image goes to history, the clipboard or a folder; in
          the editor the boxes stay editable. Region captures match the window under the centre of the selection.
        </p>
      {:else if page === "editor"}
        <h2>Editor defaults</h2>
        <div class="row">
          <label for="sw">Stroke width</label>
          <input id="sw" type="number" min="1" max="40" bind:value={settings.editor.strokeWidth} style="width:80px" />
        </div>
        <div class="row">
          <label for="fs">Font size</label>
          <input id="fs" type="number" min="10" max="96" bind:value={settings.editor.fontSize} style="width:80px" />
        </div>
        <div class="row">
          <label for="bs">Badge size</label>
          <input id="bs" type="number" min="14" max="80" bind:value={settings.editor.badgeSize} style="width:80px" />
        </div>
        <div class="row">
          <label for="ba">Pixelate strength</label>
          <input id="ba" type="number" min="4" max="40" bind:value={settings.editor.blurAmount} style="width:80px" />
        </div>
        <div class="row">
          <label for="sh">Drop shadow</label>
          <input id="sh" type="checkbox" bind:checked={settings.editor.shadow} />
        </div>
        <div class="row">
          <label for="pal">Palette (keys 1–9)</label>
          <div class="palette" id="pal">
            {#each settings.editor.palette as _, i (i)}
              <input type="color" bind:value={settings.editor.palette[i]} />
            {/each}
          </div>
        </div>
        <div class="row">
          <label for="dc">Default colour</label>
          <input id="dc" type="color" bind:value={settings.editor.strokeColor} />
        </div>
        <h3>Tool shortcuts</h3>
        <table class="keys">
          <tbody>
            {#each Object.keys(TOOL_NAMES) as t (t)}
              <tr>
                <td>{TOOL_NAMES[t]}</td>
                <td><input type="text" maxlength="1" bind:value={settings.editor.shortcuts[t]} /></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <h2>About</h2>
        <p>QuickShot {paths?.version} · {paths?.platform}</p>
        <div class="row"><span>Settings file</span><span class="inline"><code>{paths?.configDir}</code><button onclick={() => paths && revealItemInDir(paths.configDir)}>Show</button></span></div>
        <div class="row"><span>Screenshots</span><span class="inline"><code>{paths?.saveDir}</code><button onclick={() => paths && openPath(paths.saveDir)}>Open</button></span></div>
        <div class="row"><span>Guides</span><span class="inline"><code>{paths?.guidesDir}</code><button onclick={() => paths && openPath(paths.guidesDir)}>Open</button></span></div>
        <p class="muted">Command line: <code>quickshot --capture region|window|fullscreen|ocr|pin|color|qr [--delay N]</code>, <code>--settings</code>, <code>--guides</code>, <code>--history</code>.</p>
        <p class="muted">Scriptable capture (no windows, real exit codes): <code>quickshot --out C:\shots\ [--window "Grafana" | --monitor all | --rect x,y,w,h]</code>. Run <code>quickshot --help</code> for all options.</p>
      {/if}

      {#if page !== "home" && page !== "about"}
        <div class="footer">
          <button class="primary" onclick={save} disabled={saving}>Save settings</button>
          {#if saved}<span class="ok">Saved and applied.</span>{/if}
        </div>
      {/if}
    </main>
  </div>
{:else}
  <div style="padding:20px" class="muted">Loading…</div>
{/if}
