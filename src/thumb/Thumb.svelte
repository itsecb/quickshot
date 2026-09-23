<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { copyCapture, currentLabel, thumbAction, thumbDragPath, thumbInit, type ThumbInit } from "$lib/ipc";
  import { icon } from "../editor/icons";

  type Init = { status: string; file?: { url: string; path: string } };
  const init = (window as unknown as { __QS_THUMB?: Init }).__QS_THUMB;
  const status = init?.status ?? "Captured";
  /** A saved file that isn't a capture (a recorded GIF): no editor, copied as a file. */
  const file = init?.file ?? null;
  const LIFETIME_MS = file ? 12000 : 6000;
  const win = getCurrentWindow();

  let info = $state<ThumbInit | null>(null);
  let note = $state(status);
  let leaving = $state(false);
  let hovering = $state(false);
  let remaining = $state(LIFETIME_MS);
  let dragPath: string | null = file?.path ?? null;
  let dragIcon: string | null = null;

  onMount(() => {
    void (async () => {
      if (file) return;
      try {
        info = await thumbInit(currentLabel());
        dragPath = await thumbDragPath(info.id);
      } catch (e) {
        note = String(e);
      }
    })();
    // count down only while the pointer is away, so reading it never makes it vanish
    let last = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      if (!hovering && !leaving) remaining -= now - last;
      last = now;
      if (remaining <= 0) void close();
      else raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });

  async function close() {
    if (leaving) return;
    leaving = true;
    await new Promise((r) => setTimeout(r, 180)); // slide out
    await win.close();
  }

  async function act(action: "edit" | "pin") {
    if (!info) return;
    try {
      await thumbAction(info.id, action);
      await close();
    } catch (e) {
      note = String(e);
    }
  }

  async function copy() {
    if (!info && !file) return;
    try {
      if (file) await invoke("copy_file", { path: file.path });
      else if (info) await copyCapture(info.id);
      note = "Copied to clipboard";
      remaining = Math.max(remaining, 2500);
    } catch (e) {
      note = String(e);
    }
  }

  function makeIcon(img: HTMLImageElement) {
    try {
      const scale = Math.min(1, 160 / Math.max(img.naturalWidth, img.naturalHeight));
      const c = document.createElement("canvas");
      c.width = Math.max(1, Math.round(img.naturalWidth * scale));
      c.height = Math.max(1, Math.round(img.naturalHeight * scale));
      c.getContext("2d")!.drawImage(img, 0, 0, c.width, c.height);
      dragIcon = c.toDataURL("image/png");
    } catch {
      dragIcon = null; // tainted canvas: fall back to the file itself as the drag image
    }
  }

  function onDragStart(e: MouseEvent) {
    if (e.button !== 0 || !dragPath) return;
    e.preventDefault();
    startDrag({ item: [dragPath], icon: dragIcon ?? dragPath, mode: "copy" }).catch((err) => (note = `Drag failed: ${err}`));
  }
</script>

<div
  class="card"
  class:leaving
  role="dialog"
  aria-label="Capture"
  tabindex="-1"
  onmouseenter={() => (hovering = true)}
  onmouseleave={() => (hovering = false)}
>
  <div class="head">
    <span class="check" aria-hidden="true">✓</span>
    <span class="note" title={note}>{note}</span>
    <button class="icon-btn" onclick={close} title="Dismiss" aria-label="Dismiss">✕</button>
  </div>
  <div
    class="shot"
    role="button"
    tabindex="-1"
    title={file ? "Drag into any app · double-click to open" : "Drag into any app · double-click to edit"}
    onmousedown={onDragStart}
    ondblclick={() => (file ? openPath(file.path).catch(() => {}) : act("edit"))}
  >
    {#if file}
      <img src={file.url} alt="Recording" crossorigin="anonymous" draggable="false" onload={(e) => makeIcon(e.currentTarget as HTMLImageElement)} />
    {:else if info}
      <img src={info.pngUrl} alt="Capture" crossorigin="anonymous" draggable="false" onload={(e) => makeIcon(e.currentTarget as HTMLImageElement)} />
    {/if}
  </div>
  <div class="actions">
    {#if file}
      <button onclick={copy} title="Copy the file (paste into Teams, Outlook or a folder)">{@html icon("copy")} Copy</button>
      <button onclick={() => revealItemInDir(file.path).catch((e) => (note = String(e)))} title="Show in folder">{@html icon("folder")} Folder</button>
    {:else}
      <button onclick={() => act("edit")} title="Open in the editor">{@html icon("pen")} Edit</button>
      <button onclick={() => act("pin")} title="Pin on top of everything">{@html icon("pin")} Pin</button>
      <button onclick={copy} title="Copy again">{@html icon("copy")} Copy</button>
    {/if}
    <span class="drag-hint">{@html icon("drag")} drag</span>
  </div>
  <div class="life" style={`transform:scaleX(${Math.max(0, remaining / LIFETIME_MS)})`}></div>
</div>
