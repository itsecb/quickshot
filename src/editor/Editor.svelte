<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import {
    copyImage,
    copyImageRich,
    copyText,
    currentLabel,
    editorInit,
    exportTempPng,
    getSettings,
    guidePushStep,
    ocrPng,
    pinImage,
    redactCapture,
    saveImage,
    scanCodes,
    setSettings,
  } from "$lib/ipc";
  import { canvasToDataUrl, canvasToPng, fetchRawToCanvas } from "$lib/image";
  import { isTypingTarget, primaryMod } from "$lib/keys";
  import type { Beautify, EditorInit } from "$lib/types";
  import { emptyDocument, newId, type BlurShape, type Document, type ToolId } from "./document";
  import { BACKGROUNDS, backgroundCss } from "./beautify";
  import { History } from "./history";
  import { EditorStage, type Style } from "./stage";
  import { icon } from "./icons";

  const TOOLS: { id: ToolId; label: string }[] = [
    { id: "select", label: "Select / move" },
    { id: "arrow", label: "Arrow" },
    { id: "line", label: "Line" },
    { id: "rect", label: "Rectangle" },
    { id: "ellipse", label: "Ellipse" },
    { id: "pen", label: "Pen" },
    { id: "highlighter", label: "Highlighter" },
    { id: "text", label: "Text" },
    { id: "badge", label: "Numbered step" },
    { id: "blur", label: "Blur / pixelate" },
    { id: "crop", label: "Crop" },
    { id: "measure", label: "Measure" },
  ];

  let host = $state<HTMLDivElement | null>(null);
  let init = $state<EditorInit | null>(null);
  let stage: EditorStage | null = null;
  let history: History | null = null;
  let tool = $state<ToolId>("select");
  let style = $state<Style>({
    stroke: "#ff3b30",
    strokeWidth: 4,
    fontSize: 22,
    fontFamily: "Segoe UI, sans-serif",
    shadow: true,
    blurAmount: 12,
    blurMode: "pixelate",
    badgeSize: 28,
    fill: false,
  });
  let palette = $state<string[]>([]);
  let shortcuts = $state<Record<string, string>>({});
  let canUndo = $state(false);
  let canRedo = $state(false);
  let zoom = $state(1);
  let status = $state("");
  let toast = $state<{ text: string; error: boolean } | null>(null);
  let dirty = $state(false);
  let hasCrop = $state(false);
  let selectedId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let beautifyOpts = $state<Beautify>({ enabled: false, padding: 48, background: "ocean", radius: 10, shadow: true });
  let showBeautify = $state(false);
  let previewCanvas = $state<HTMLCanvasElement | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  const win = getCurrentWindow();

  // Width is remembered per tool, so a thick arrow doesn't mean a thick rectangle.
  const WIDTH_TOOLS = new Set<string>(["arrow", "line", "rect", "ellipse", "pen", "highlighter", "measure"]);
  const WIDTHS_KEY = "quickshot.toolWidths";
  let toolWidths: Record<string, number> = {};
  let defaultWidth = 4;

  function loadWidths() {
    try {
      const saved = JSON.parse(localStorage.getItem(WIDTHS_KEY) ?? "{}");
      toolWidths = saved && typeof saved === "object" ? saved : {};
    } catch {
      toolWidths = {};
    }
  }

  /** The tool whose width the slider currently edits: the active drawing tool, or the selected shape's type. */
  function widthOwner(): string | null {
    if (tool !== "select") return WIDTH_TOOLS.has(tool) ? tool : null;
    const shape = stage?.selectedShape();
    return shape && WIDTH_TOOLS.has(shape.type) ? shape.type : null;
  }

  function setWidth(w: number) {
    style.strokeWidth = Math.max(1, Math.min(40, Math.round(w)));
    applyStyle();
    const owner = widthOwner();
    if (!owner) return;
    toolWidths[owner] = style.strokeWidth;
    try {
      localStorage.setItem(WIDTHS_KEY, JSON.stringify(toolWidths));
    } catch {
      // storage unavailable: the width still applies for this session
    }
  }

  /** Show the selected shape's own width in the toolbar without changing the shape. */
  function showSelectedWidth() {
    const shape = stage?.selectedShape();
    if (!shape || !WIDTH_TOOLS.has(shape.type)) return;
    style.strokeWidth = shape.type === "highlighter" ? Math.max(1, Math.round(shape.strokeWidth / 4)) : shape.strokeWidth;
    stage?.syncStyle($state.snapshot(style));
  }

  function showToast(text: string, isError = false) {
    toast = { text, error: isError };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), isError ? 6000 : 2500);
  }

  function keyFor(id: string): string {
    return shortcuts[id] ?? "";
  }

  function syncHistoryFlags() {
    canUndo = history?.canUndo ?? false;
    canRedo = history?.canRedo ?? false;
    hasCrop = !!history?.current.crop;
  }

  onMount(async () => {
    try {
      init = await editorInit(currentLabel());
      const s = init.settings.editor;
      style = {
        stroke: s.strokeColor,
        strokeWidth: s.strokeWidth,
        fontSize: s.fontSize,
        fontFamily: s.fontFamily,
        shadow: s.shadow,
        blurAmount: s.blurAmount,
        blurMode: "pixelate",
        badgeSize: s.badgeSize,
        fill: false,
      };
      palette = s.palette;
      shortcuts = s.shortcuts;
      defaultWidth = s.strokeWidth;
      loadWidths();
      if (s.beautify) beautifyOpts = s.beautify;
      const image = await fetchRawToCanvas(init.frameUrl);
      const doc: Document = emptyDocument(image.width, image.height);
      history = new History(doc);
      stage = new EditorStage(host!, image, doc, $state.snapshot(style), {
        onCommit: (d) => {
          history!.commit(d);
          dirty = true;
          syncHistoryFlags();
        },
        onPreview: () => {},
        onSelect: (id) => {
          selectedId = id;
          if (id) showSelectedWidth();
        },
        onStatus: (t) => (status = t),
        onZoom: (z) => (zoom = z),
      });
      stage.setBeautify($state.snapshot(beautifyOpts));
      setTool("arrow");
      syncHistoryFlags();
      // a per-app rule asked for this: boxes stay editable, Ctrl+Z removes them all
      if (init.autoRedact) void doRedact();
      await win.onCloseRequested(async (ev) => {
        if (!dirty) return;
        const ok = await confirm("Close without saving your markup?", { title: "QuickShot", kind: "warning", okLabel: "Close" });
        if (!ok) ev.preventDefault();
      });
    } catch (e) {
      error = String(e);
    }
  });

  // ---- actions ----
  function setTool(t: ToolId) {
    tool = t;
    stage?.setTool(t);
    if (WIDTH_TOOLS.has(t)) {
      style.strokeWidth = toolWidths[t] ?? defaultWidth;
      applyStyle(); // switching to a drawing tool deselects, so no shape is changed
    }
  }

  function applyStyle() {
    stage?.setStyle($state.snapshot(style));
  }

  function setColor(c: string) {
    style.stroke = c;
    applyStyle();
  }

  function undo() {
    if (!history || !stage || !history.undo()) return;
    stage.setDocument(history.current);
    syncHistoryFlags();
  }

  function redo() {
    if (!history || !stage || !history.redo()) return;
    stage.setDocument(history.current);
    syncHistoryFlags();
  }

  /** `raw` = without the beautify backdrop (for OCR). */
  async function renderPng(raw = false): Promise<Uint8Array> {
    if (!stage) throw new Error("not ready");
    return canvasToPng(stage.render(raw));
  }

  function stem(): string {
    const t = init?.source.title?.trim();
    const base = t ? t.replace(/[<>:"/\\|?*]+/g, "").slice(0, 60) : "Screenshot";
    const d = new Date();
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${base} ${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}-${pad(d.getMinutes())}-${pad(d.getSeconds())}`;
  }

  async function guarded(label: string, fn: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try {
      await fn();
    } catch (e) {
      showToast(`${label} failed: ${e}`, true);
    } finally {
      busy = false;
    }
  }

  const doCopy = () =>
    guarded("Copy", async () => {
      await copyImage(await renderPng());
      dirty = false;
      showToast("Copied to clipboard");
    });

  const doSave = () =>
    guarded("Save", async () => {
      const path = await saveImage(await renderPng(), { stem: stem() });
      dirty = false;
      showToast(`Saved ${path}`);
    });

  const doSaveAs = () =>
    guarded("Save", async () => {
      const path = await saveDialog({
        defaultPath: `${stem()}.png`,
        filters: [
          { name: "PNG", extensions: ["png"] },
          { name: "JPEG", extensions: ["jpg", "jpeg"] },
        ],
      });
      if (!path) return;
      await saveImage(await renderPng(), { path });
      dirty = false;
      showToast(`Saved ${path}`);
    });

  const doPin = () =>
    guarded("Pin", async () => {
      const pos = await win.outerPosition();
      await pinImage(await renderPng(), pos.x + 40, pos.y + 80);
      showToast("Pinned");
    });

  const doOcr = () =>
    guarded("OCR", async () => {
      showToast("Recognising text…");
      const out = await ocrPng(await renderPng(true));
      if (!out.text.trim()) return showToast("No text found", true);
      await copyText(out.text);
      showToast(`Copied ${out.text.length} characters of text`);
    });

  const doCopyRich = () =>
    guarded("Copy for ticket", async () => {
      const caption = await copyImageRich(await renderPng(), init!.id);
      dirty = false;
      showToast(caption ? `Copied with caption: ${caption}` : "Copied");
    });

  const REDACT_LABELS: Record<string, string> = { IP: "IP", IPv6: "IPv6", email: "email", hostname: "hostname" };

  const doRedact = () =>
    guarded("Redact", async () => {
      if (!stage || !init) return;
      showToast("Looking for sensitive text…");
      const hits = await redactCapture(init.id);
      if (!hits.length) return showToast("Nothing sensitive found");
      const shapes: BlurShape[] = hits.map((h) => ({
        id: newId(),
        type: "blur",
        x: h.rect.x,
        y: h.rect.y,
        width: h.rect.width,
        height: h.rect.height,
        mode: "pixelate",
        amount: Math.max(8, style.blurAmount),
        stroke: style.stroke,
        strokeWidth: 0,
        opacity: 1,
        shadow: false,
      }));
      stage.addShapes(shapes);
      const counts = new Map<string, number>();
      for (const h of hits) counts.set(h.kind, (counts.get(h.kind) ?? 0) + 1);
      const summary = [...counts].map(([k, n]) => `${n} ${REDACT_LABELS[k] ?? k}`).join(", ");
      showToast(`Redacted ${hits.length} (${summary}). Review them; Ctrl+Z undoes`);
    });

  const doScan = () =>
    guarded("Scan", async () => {
      if (!init) return;
      const codes = await scanCodes(init.id);
      if (!codes.length) return showToast("No QR code or barcode found", true);
      await copyText(codes.map((c) => c.text).join("\n"));
      const first = codes[0]!.text;
      showToast(codes.length > 1 ? `Copied ${codes.length} codes` : `Copied: ${first.length > 60 ? first.slice(0, 60) + "…" : first}`);
    });

  // ---- beautify ----
  function applyBeautify() {
    stage?.setBeautify($state.snapshot(beautifyOpts));
    dragCache = null; // the drag image includes the backdrop
    drawPreview();
  }

  function drawPreview() {
    if (!stage || !previewCanvas) return;
    const full = stage.render();
    const scale = Math.min(1, 260 / full.width, 150 / full.height);
    previewCanvas.width = Math.max(1, Math.round(full.width * scale));
    previewCanvas.height = Math.max(1, Math.round(full.height * scale));
    const ctx = previewCanvas.getContext("2d")!;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(full, 0, 0, previewCanvas.width, previewCanvas.height);
  }

  /** Remember the look for next time (only when the panel closes, to avoid churn). */
  async function persistBeautify() {
    try {
      const s = await getSettings();
      s.editor.beautify = $state.snapshot(beautifyOpts);
      await setSettings(s);
    } catch (e) {
      console.warn("could not save beautify settings", e);
    }
  }

  function toggleBeautify() {
    beautifyOpts.enabled = !beautifyOpts.enabled;
    applyBeautify();
    void persistBeautify();
    showToast(beautifyOpts.enabled ? "Beautify on: copy, save, drag and pin include the backdrop" : "Beautify off");
  }

  function openBeautify() {
    showBeautify = !showBeautify;
    if (showBeautify) requestAnimationFrame(drawPreview);
    else void persistBeautify();
  }

  function closeBeautify() {
    if (!showBeautify) return;
    showBeautify = false;
    void persistBeautify();
  }

  const doGuide = () =>
    guarded("Add to guide", async () => {
      await guidePushStep(await renderPng(), init?.source.title ?? "");
      showToast("Added as next guide step");
    });

  function fit() {
    stage?.fit();
  }

  function zoom100() {
    stage?.setZoom(1);
  }

  // Native drags must start inside the mousedown, so the temp file is prepared when the
  // pointer enters the handle and reused until the document changes.
  let dragCache: { doc: Document; path: string; icon: string } | null = null;
  let dragPrep: Promise<void> | null = null;

  function prepareDrag() {
    if (!stage || !history) return;
    if (dragCache?.doc === history.current || dragPrep) return;
    const doc = history.current;
    dragPrep = (async () => {
      try {
        const canvas = stage!.render();
        const png = await canvasToPng(canvas);
        const path = await exportTempPng(png, stem());
        dragCache = { doc, path, icon: canvasToDataUrl(canvas, 200) };
      } catch (err) {
        console.warn("drag prepare failed", err);
      } finally {
        dragPrep = null;
      }
    })();
  }

  function onDragStart(e: MouseEvent) {
    if (e.button !== 0 || !stage || !history) return;
    e.preventDefault();
    const cached = dragCache?.doc === history.current ? dragCache : null;
    if (!cached) {
      prepareDrag();
      showToast("Preparing image, drag again");
      return;
    }
    startDrag({ item: [cached.path], icon: cached.icon, mode: "copy" }).catch((err) => showToast(`Drag failed: ${err}`, true));
  }

  async function closeWindow() {
    await win.close();
  }

  // ---- keyboard ----
  function onKey(e: KeyboardEvent) {
    if (!stage || stage.isEditingText || isTypingTarget(e)) return;
    const mod = primaryMod(e);
    const k = e.key.toLowerCase();

    if (mod) {
      switch (k) {
        case "z":
          e.preventDefault();
          e.shiftKey ? redo() : undo();
          return;
        case "y":
          e.preventDefault();
          redo();
          return;
        case "c":
          e.preventDefault();
          void (e.altKey ? doCopyRich() : doCopy());
          return;
        case "b":
          e.preventDefault();
          toggleBeautify();
          return;
        case "x":
          if (e.shiftKey) {
            e.preventDefault();
            void doRedact();
          }
          return;
        case "s":
          e.preventDefault();
          e.shiftKey ? void doSaveAs() : void doSave();
          return;
        case "p":
          if (e.shiftKey) {
            e.preventDefault();
            void doPin();
          }
          return;
        case "e":
          e.preventDefault();
          void doGuide();
          return;
        case "d":
          e.preventDefault();
          stage.duplicateSelected();
          return;
        case "a":
          e.preventDefault();
          return;
        case "0":
          e.preventDefault();
          fit();
          return;
        case "1":
          e.preventDefault();
          zoom100();
          return;
        case "=":
        case "+":
          e.preventDefault();
          stage.setZoom(stage.zoom * 1.25);
          return;
        case "-":
          e.preventDefault();
          stage.setZoom(stage.zoom / 1.25);
          return;
        case "w":
          e.preventDefault();
          void closeWindow();
          return;
      }
      return;
    }

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        if (showBeautify) {
          closeBeautify();
        } else if (stage.selectedId) {
          stage.select(null);
        } else if (tool !== "select") {
          setTool("select");
        } else {
          void closeWindow();
        }
        return;
      case "Delete":
      case "Backspace":
        e.preventDefault();
        stage.deleteSelected();
        return;
      case "Tab":
        e.preventDefault();
        stage.selectNext(e.shiftKey ? -1 : 1);
        return;
      case "ArrowLeft":
      case "ArrowRight":
      case "ArrowUp":
      case "ArrowDown": {
        if (!stage.selectedId) return;
        e.preventDefault();
        const step = e.shiftKey ? 10 : 1;
        const d = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }[e.key]!;
        stage.nudgeSelected(d[0]!, d[1]!);
        return;
      }
      case "[":
        e.preventDefault();
        setWidth(style.strokeWidth - 1);
        return;
      case "]":
        e.preventDefault();
        setWidth(style.strokeWidth + 1);
        return;
    }

    if (/^[1-9]$/.test(e.key)) {
      const c = palette[Number(e.key) - 1];
      if (c) {
        e.preventDefault();
        setColor(c);
      }
      return;
    }

    for (const t of TOOLS) {
      if (shortcuts[t.id] && shortcuts[t.id]!.toLowerCase() === k) {
        e.preventDefault();
        setTool(t.id);
        return;
      }
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if error}
  <div class="loading">Could not open capture: {error}</div>
{:else}
  <div class="editor">
    <div class="toolbar">
      <div class="group">
        {#each TOOLS as t (t.id)}
          <button
            class="tool"
            class:active={tool === t.id}
            title={`${t.label}${keyFor(t.id) ? ` (${keyFor(t.id).toUpperCase()})` : ""}`}
            onclick={() => setTool(t.id)}
          >
            {@html icon(t.id)}
            {#if keyFor(t.id)}<span class="key">{keyFor(t.id)}</span>{/if}
          </button>
        {/each}
      </div>

      <div class="group right" aria-label="Enhance">
        <button class="tool" onclick={doRedact} title="Redact: pixelate IPs, emails, hostnames, secrets… (Ctrl+Shift+X)">{@html icon("redact")}</button>
        <div class="popover-anchor">
          <button
            class="tool"
            class:active={beautifyOpts.enabled}
            onclick={openBeautify}
            title="Beautify: backdrop, padding, rounded corners, shadow (Ctrl+B toggles)">{@html icon("beautify")}</button
          >
          {#if showBeautify}
            <div class="popover-backdrop" role="presentation" onclick={closeBeautify}></div>
            <div class="popover">
              <label class="row"><input type="checkbox" bind:checked={beautifyOpts.enabled} onchange={applyBeautify} /> Apply to exports</label>
              <canvas class="preview" bind:this={previewCanvas}></canvas>
              <div class="swatches">
                {#each BACKGROUNDS as b (b.id)}
                  <button
                    class="bg-swatch"
                    class:active={beautifyOpts.background === b.id}
                    style={`background:${backgroundCss(b.id)}`}
                    title={b.label}
                    aria-label={b.label}
                    onclick={() => ((beautifyOpts.background = b.id), (beautifyOpts.enabled = true), applyBeautify())}
                  ></button>
                {/each}
              </div>
              <label class="row">Padding <input type="range" min="0" max="160" bind:value={beautifyOpts.padding} oninput={applyBeautify} /> <span>{beautifyOpts.padding}</span></label>
              <label class="row">Corners <input type="range" min="0" max="40" bind:value={beautifyOpts.radius} oninput={applyBeautify} /> <span>{beautifyOpts.radius}</span></label>
              <label class="row"><input type="checkbox" bind:checked={beautifyOpts.shadow} onchange={applyBeautify} /> Shadow</label>
            </div>
          {/if}
        </div>
        <button class="tool" onclick={doOcr} title="Copy text from the image (OCR)">{@html icon("ocr")}</button>
        <button class="tool" onclick={doScan} title="Read QR codes / barcodes">{@html icon("qr")}</button>
      </div>

      <div class="group" aria-label="Share">
        <button class="tool" onclick={doPin} title="Pin on top of everything (Ctrl+Shift+P)">{@html icon("pin")}</button>
        <button class="tool" onclick={doGuide} title="Add as next step in the guide (Ctrl+E)">{@html icon("guide")}</button>
        <button class="tool" onclick={doCopyRich} title="Copy for ticket: image + caption with window, time and PC (Ctrl+Alt+C)">{@html icon("ticket")}</button>
        <div
          class="tool drag"
          role="button"
          tabindex="-1"
          onmouseenter={prepareDrag}
          onmousedown={onDragStart}
          title="Drag the image into Teams, Outlook, a browser or Explorer"
        >
          {@html icon("drag")}
        </div>
      </div>

      <div class="group main-actions">
        <button class="action" onclick={doSave} title="Save to folder (Ctrl+S) · Save as (Ctrl+Shift+S)">{@html icon("save")}<span class="label">Save</span></button>
        <button class="action primary" onclick={doCopy} title="Copy to clipboard (Ctrl+C)">{@html icon("copy")}<span class="label">Copy</span></button>
      </div>
    </div>

    <div class="propbar">
      <span class="tool-name">{TOOLS.find((t) => t.id === tool)?.label ?? ""}</span>
      <div class="group">
        {#each palette as c, i (i)}
          <button
            class="swatch"
            class:active={style.stroke.toLowerCase() === c.toLowerCase()}
            style={`background:${c}`}
            title={`${c} (${i + 1})`}
            onclick={() => setColor(c)}
          >
            <span class="key">{i + 1}</span>
          </button>
        {/each}
        <input type="color" bind:value={style.stroke} onchange={applyStyle} title="Custom colour" style="width:26px;height:24px;padding:0;border:0;background:transparent" />
      </div>

      <div class="group">
        {#if WIDTH_TOOLS.has(tool) || (tool === "select" && selectedId)}
        <label class="width" title="Stroke width, remembered per tool ( [ and ] )">
          <span class="muted">Width</span>
          <input type="range" min="1" max="30" value={style.strokeWidth} oninput={(e) => setWidth(+e.currentTarget.value)} />
          <span style="width:2ch">{style.strokeWidth}</span>
        </label>
        {/if}
        {#if tool === "text" || selectedId}
          <label class="width" title="Font size">
            <span class="muted">Text</span>
            <input type="range" min="10" max="96" bind:value={style.fontSize} onchange={applyStyle} />
          </label>
        {/if}
        {#if tool === "blur"}
          <select class="select-sm" bind:value={style.blurMode} onchange={applyStyle} title="Blur mode">
            <option value="pixelate">Pixelate</option>
            <option value="blur">Blur</option>
          </select>
          <input type="range" min="4" max="40" bind:value={style.blurAmount} onchange={applyStyle} title="Strength" style="width:60px" />
        {/if}
        {#if tool === "rect" || tool === "ellipse"}
          <label class="width"><input type="checkbox" bind:checked={style.fill} onchange={applyStyle} /> Fill</label>
        {/if}
        <label class="width" title="Drop shadow"><input type="checkbox" bind:checked={style.shadow} onchange={applyStyle} /> Shadow</label>
      </div>

      <span class="spacer"></span>
      <div class="group">
        <button class="tool" onclick={undo} disabled={!canUndo} title="Undo (Ctrl+Z)">{@html icon("undo")}</button>
        <button class="tool" onclick={redo} disabled={!canRedo} title="Redo (Ctrl+Shift+Z)">{@html icon("redo")}</button>
        {#if hasCrop}
          <button class="action" onclick={() => stage?.clearCrop()} title="Remove crop">{@html icon("clear")} Uncrop</button>
        {/if}
      </div>

    </div>

    <div class="canvas-host" bind:this={host}></div>

    <div class="statusbar">
      {#if init}
        <span>{init.width} × {init.height}</span>
        {#if init.source.title}<span class="muted">{init.source.appName ? `${init.source.appName} · ` : ""}{init.source.title}</span>{/if}
      {/if}
      <span>{status}</span>
      <span class="spacer"></span>
      {#if toast}{#key toast}<span class="toast" class:error={toast.error}>{toast.text}</span>{/key}{/if}
      <button class="tool" onclick={fit} title="Fit (Ctrl+0)" style="height:22px;width:26px">{@html icon("fit")}</button>
      <span title="Zoom (Ctrl+wheel, Ctrl+1 = 100%)">{Math.round(zoom * 100)}%</span>
    </div>
  </div>
{/if}
