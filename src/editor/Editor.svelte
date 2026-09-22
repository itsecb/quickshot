<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import {
    copyImage,
    copyText,
    currentLabel,
    editorInit,
    exportTempPng,
    guidePushStep,
    ocrPng,
    pinImage,
    saveImage,
  } from "$lib/ipc";
  import { canvasToDataUrl, canvasToPng, fetchRawToCanvas } from "$lib/image";
  import { isTypingTarget, primaryMod } from "$lib/keys";
  import type { EditorInit } from "$lib/types";
  import { emptyDocument, type Document, type ToolId } from "./document";
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
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  const win = getCurrentWindow();

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
        onSelect: (id) => (selectedId = id),
        onStatus: (t) => (status = t),
        onZoom: (z) => (zoom = z),
      });
      stage.setTool("arrow");
      tool = "arrow";
      syncHistoryFlags();
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

  async function renderPng(): Promise<Uint8Array> {
    if (!stage) throw new Error("not ready");
    return canvasToPng(stage.render());
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
      const out = await ocrPng(await renderPng());
      if (!out.text.trim()) return showToast("No text found", true);
      await copyText(out.text);
      showToast(`Copied ${out.text.length} characters of text`);
    });

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

  async function onDragStart(e: MouseEvent) {
    if (e.button !== 0 || !stage) return;
    e.preventDefault();
    try {
      const canvas = stage.render();
      const png = await canvasToPng(canvas);
      const path = await exportTempPng(png, stem());
      await startDrag({ item: [path], icon: canvasToDataUrl(canvas, 200), mode: "copy" });
    } catch (err) {
      showToast(`Drag failed: ${err}`, true);
    }
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
          void doCopy();
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
        if (stage.selectedId) {
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
        style.strokeWidth = Math.max(1, style.strokeWidth - 1);
        applyStyle();
        return;
      case "]":
        e.preventDefault();
        style.strokeWidth = Math.min(40, style.strokeWidth + 1);
        applyStyle();
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

      <div class="group">
        {#each palette as c, i (c)}
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
        <label class="width" title="Stroke width ( [ and ] )">
          <span class="muted">Width</span>
          <input type="range" min="1" max="30" bind:value={style.strokeWidth} onchange={applyStyle} oninput={applyStyle} />
          <span style="width:2ch">{style.strokeWidth}</span>
        </label>
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

      <div class="group">
        <button class="tool" onclick={undo} disabled={!canUndo} title="Undo (Ctrl+Z)">{@html icon("undo")}</button>
        <button class="tool" onclick={redo} disabled={!canRedo} title="Redo (Ctrl+Shift+Z)">{@html icon("redo")}</button>
        {#if hasCrop}
          <button class="action" onclick={() => stage?.clearCrop()} title="Remove crop">{@html icon("clear")} Uncrop</button>
        {/if}
      </div>

      <div class="group">
        <button class="action primary" onclick={doCopy} title="Copy to clipboard (Ctrl+C)">{@html icon("copy")} Copy</button>
        <button class="action" onclick={doSave} title="Save to folder (Ctrl+S) · Save as (Ctrl+Shift+S)">{@html icon("save")} Save</button>
        <button class="action" onclick={doPin} title="Pin to screen (Ctrl+Shift+P)">{@html icon("pin")}</button>
        <button class="action" onclick={doOcr} title="Copy text via OCR">{@html icon("ocr")}</button>
        <button class="action" onclick={doGuide} title="Add as next step in the guide (Ctrl+E)">{@html icon("guide")} Guide</button>
        <div class="drag-handle" role="button" tabindex="-1" onmousedown={onDragStart} title="Drag the image into Teams, Outlook, a browser or Explorer">
          {@html icon("drag")} Drag
        </div>
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
      {#if toast}<span class="toast" class:error={toast.error}>{toast.text}</span>{/if}
      <button class="tool" onclick={fit} title="Fit (Ctrl+0)" style="height:22px;width:26px">{@html icon("fit")}</button>
      <span title="Zoom (Ctrl+wheel, Ctrl+1 = 100%)">{Math.round(zoom * 100)}%</span>
    </div>
  </div>
{/if}
