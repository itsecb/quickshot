// Capture overlay: one instance per monitor. Paints the frozen frame, lets the user
// drag a region, click a window or a pixel, and hands the result to Rust.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { cancelCapture, copyText, finishCapture, overlayInit, overlayReady } from "$lib/ipc";
import { fetchRawToCanvas } from "$lib/image";
import { rectContains, rectFromPoints, rectIntersect, rectRound } from "$lib/geometry";
import { rgbToHex } from "$lib/image";
import type { OverlayInit, Rect, WindowInfo } from "$lib/types";
import "./overlay.css";

const win = getCurrentWindow();
const label = win.label;

interface Point {
  x: number;
  y: number;
}

class Overlay {
  private canvas = document.createElement("canvas");
  private ctx: CanvasRenderingContext2D;
  private frame!: HTMLCanvasElement;
  private frameCtx!: CanvasRenderingContext2D;
  private init!: OverlayInit;
  private dpr = window.devicePixelRatio || 1;
  private cursor: Point | null = null; // local physical px
  private dragStart: Point | null = null; // global physical px
  private selection: Rect | null = null; // global physical px, while dragging
  private remoteSelection: Rect | null = null; // selection drawn on another monitor
  private hoverWindow: WindowInfo | null = null;
  private finished = false;
  private raf = 0;
  private unlisten: UnlistenFn[] = [];
  private lastEmit = 0;
  private hint = "";

  constructor() {
    this.ctx = this.canvas.getContext("2d", { alpha: false })!;
    document.getElementById("app")!.appendChild(this.canvas);
  }

  async start() {
    this.init = await overlayInit(label);
    this.frame = await fetchRawToCanvas(this.init.monitor.frameUrl);
    this.frameCtx = this.frame.getContext("2d", { willReadFrequently: true })!;
    this.canvas.width = this.frame.width;
    this.canvas.height = this.frame.height;
    this.hint = this.hintText();
    this.render();
    this.bind();
    await overlayReady(label);
  }

  private hintText(): string {
    switch (this.init.mode) {
      case "window":
        return "Click a window · drag for a region · Esc cancels";
      case "ocr":
        return "Select text to copy · Esc cancels";
      case "pin":
        return "Select a region to pin · Esc cancels";
      case "color":
        return "Click to copy HEX · Shift+click for RGB · Esc cancels";
      case "qr":
        return "Select a QR code or barcode to copy its contents · Esc cancels";
      default:
        return "Drag a region · click a window · Enter = whole screen · Esc cancels";
    }
  }

  // ---- coordinate helpers ----
  private toLocal(e: MouseEvent): Point {
    return { x: e.clientX * this.dpr, y: e.clientY * this.dpr };
  }
  private toGlobal(p: Point): Point {
    return { x: p.x + this.init.monitor.x, y: p.y + this.init.monitor.y };
  }
  private screenBounds(): Rect {
    const ms = this.init.monitors;
    const x0 = Math.min(...ms.map((m) => m.x));
    const y0 = Math.min(...ms.map((m) => m.y));
    const x1 = Math.max(...ms.map((m) => m.x + m.width));
    const y1 = Math.max(...ms.map((m) => m.y + m.height));
    return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
  }
  private monitorRect(): Rect {
    const m = this.init.monitor;
    return { x: m.x, y: m.y, width: m.width, height: m.height };
  }

  private windowAt(global: Point): WindowInfo | null {
    // windows are sorted topmost first
    for (const w of this.init.windows) {
      if (rectContains(w.rect, global.x, global.y)) return w;
    }
    return null;
  }

  // ---- events ----
  private bind() {
    const c = this.canvas;
    c.addEventListener("mousemove", (e) => this.onMove(e));
    c.addEventListener("mousedown", (e) => this.onDown(e));
    window.addEventListener("mouseup", (e) => this.onUp(e));
    c.addEventListener("mouseenter", () => {
      if (!this.dragStart) void win.setFocus();
    });
    c.addEventListener("mouseleave", () => {
      if (!this.dragStart) {
        this.cursor = null;
        this.hoverWindow = null;
        this.schedule();
      }
    });
    c.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      void this.cancel();
    });
    window.addEventListener("keydown", (e) => this.onKey(e));
    window.addEventListener("blur", () => {
      // keyboard focus moved to another overlay; keep our visuals but drop hover
      if (!this.dragStart) {
        this.hoverWindow = null;
        this.schedule();
      }
    });
    void listen<{ from: string; rect: Rect | null }>("overlay://selection", (ev) => {
      if (ev.payload.from === label) return;
      this.remoteSelection = ev.payload.rect;
      this.schedule();
    }).then((u) => this.unlisten.push(u));
  }

  private onMove(e: MouseEvent) {
    const local = this.toLocal(e);
    this.cursor = local;
    const global = this.toGlobal(local);
    if (this.dragStart) {
      this.selection = rectRound(rectFromPoints(this.dragStart.x, this.dragStart.y, global.x, global.y));
      this.hoverWindow = null;
      this.broadcast();
    } else if (this.init.mode !== "color") {
      this.hoverWindow = this.windowAt(global);
    }
    this.schedule();
  }

  private onDown(e: MouseEvent) {
    if (e.button !== 0 || this.finished) return;
    e.preventDefault();
    const global = this.toGlobal(this.toLocal(e));
    if (this.init.mode === "color") {
      this.cursor = this.toLocal(e);
      void this.pickColor(e.shiftKey);
      return;
    }
    this.dragStart = global;
    this.selection = null;
  }

  private async onUp(e: MouseEvent) {
    if (e.button !== 0 || !this.dragStart || this.finished) return;
    const start = this.dragStart;
    this.dragStart = null;
    const global = this.toGlobal(this.toLocal(e));
    const sel = rectRound(rectFromPoints(start.x, start.y, global.x, global.y));
    this.broadcast(null);
    if (sel.width < 4 || sel.height < 4) {
      // a click: capture the window under the cursor, else the whole monitor
      const w = this.windowAt(global);
      if (w) return this.finish(w.rect, w.id);
      if (this.init.mode === "region" || this.init.mode === "window") return this.finish(this.monitorRect());
      this.selection = null;
      this.schedule();
      return;
    }
    return this.finish(sel);
  }

  private onKey(e: KeyboardEvent) {
    if (this.finished) return;
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        void this.cancel();
        break;
      case "Enter":
      case " ": {
        e.preventDefault();
        if (this.selection && this.selection.width >= 4 && this.selection.height >= 4) {
          void this.finish(this.selection);
        } else if (this.hoverWindow) {
          void this.finish(this.hoverWindow.rect, this.hoverWindow.id);
        } else if (this.init.mode !== "color") {
          void this.finish(this.monitorRect());
        }
        break;
      }
      case "l":
      case "L":
        if (this.init.lastRegion && this.init.mode !== "color") {
          e.preventDefault();
          void this.finish(this.init.lastRegion);
        }
        break;
      case "ArrowLeft":
      case "ArrowRight":
      case "ArrowUp":
      case "ArrowDown": {
        if (!this.cursor) break;
        e.preventDefault();
        const step = (e.shiftKey ? 10 : 1) * this.dpr;
        const d: Record<string, Point> = {
          ArrowLeft: { x: -step, y: 0 },
          ArrowRight: { x: step, y: 0 },
          ArrowUp: { x: 0, y: -step },
          ArrowDown: { x: 0, y: step },
        };
        const delta = d[e.key]!;
        this.cursor = { x: this.cursor.x + delta.x, y: this.cursor.y + delta.y };
        if (this.dragStart) {
          const g = this.toGlobal(this.cursor);
          this.selection = rectRound(rectFromPoints(this.dragStart.x, this.dragStart.y, g.x, g.y));
        }
        this.schedule();
        break;
      }
    }
  }

  private broadcast(rect: Rect | null = this.selection) {
    const now = performance.now();
    if (rect !== null && now - this.lastEmit < 16) return;
    this.lastEmit = now;
    void emit("overlay://selection", { from: label, rect });
  }

  private async finish(rect: Rect, windowId?: number) {
    if (this.finished) return;
    const clipped = rectIntersect(rect, this.screenBounds());
    if (!clipped || clipped.width < 1 || clipped.height < 1) return;
    this.finished = true;
    this.selection = clipped;
    this.render();
    try {
      await finishCapture(rectRound(clipped), windowId);
    } catch (err) {
      console.error(err);
      this.finished = false;
    }
  }

  private async cancel() {
    if (this.finished) return;
    this.finished = true;
    await cancelCapture();
  }

  private async pickColor(asRgb: boolean) {
    if (!this.cursor) return;
    const [r, g, b] = this.pixelAt(this.cursor);
    const text = asRgb ? `rgb(${r}, ${g}, ${b})` : rgbToHex(r, g, b);
    this.finished = true;
    try {
      await copyText(text);
    } finally {
      await cancelCapture();
    }
  }

  private pixelAt(p: Point): [number, number, number] {
    const x = Math.max(0, Math.min(this.frame.width - 1, Math.floor(p.x)));
    const y = Math.max(0, Math.min(this.frame.height - 1, Math.floor(p.y)));
    const d = this.frameCtx.getImageData(x, y, 1, 1).data;
    return [d[0]!, d[1]!, d[2]!];
  }

  // ---- drawing ----
  private schedule() {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => {
      this.raf = 0;
      this.render();
    });
  }

  private render() {
    const { ctx, canvas, dpr } = this;
    const W = canvas.width;
    const H = canvas.height;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(this.frame, 0, 0);

    const mon = this.init.monitor;
    const toLocalRect = (r: Rect): Rect => ({ x: r.x - mon.x, y: r.y - mon.y, width: r.width, height: r.height });

    // dim everything, then punch out the active area
    const active: Rect | null = this.selection ?? this.remoteSelection ?? this.hoverWindow?.rect ?? null;
    ctx.fillStyle = this.init.mode === "color" ? "rgba(0,0,0,0.08)" : "rgba(0,0,0,0.38)";
    ctx.fillRect(0, 0, W, H);
    if (active) {
      const l = toLocalRect(active);
      const vis = rectIntersect(l, { x: 0, y: 0, width: W, height: H });
      if (vis) {
        ctx.drawImage(this.frame, vis.x, vis.y, vis.width, vis.height, vis.x, vis.y, vis.width, vis.height);
        const selecting = !!(this.selection || this.remoteSelection);
        ctx.lineWidth = (selecting ? 2 : 1.5) * dpr;
        ctx.strokeStyle = selecting ? "#4c8dff" : "#ffcc00";
        ctx.setLineDash([]);
        ctx.strokeRect(vis.x + 0.5, vis.y + 0.5, vis.width - 1, vis.height - 1);
        if (selecting) this.drawCorners(vis);
        this.drawLabel(`${active.width} × ${active.height}`, vis.x, vis.y - 8 * dpr, vis);
        if (this.hoverWindow && !this.selection) {
          const t = this.hoverWindow.title || this.hoverWindow.appName;
          if (t) this.drawLabel(t.slice(0, 80), vis.x, vis.y + vis.height + 22 * dpr, vis, true);
        }
      }
    }

    // ghost of the last region
    if (this.init.lastRegion && !active) {
      const l = toLocalRect(this.init.lastRegion);
      ctx.setLineDash([6 * dpr, 4 * dpr]);
      ctx.strokeStyle = "rgba(255,255,255,0.45)";
      ctx.lineWidth = dpr;
      ctx.strokeRect(l.x + 0.5, l.y + 0.5, l.width - 1, l.height - 1);
      ctx.setLineDash([]);
    }

    if (this.cursor && !this.finished) {
      // While dragging, the cursor sits on the frame's corner: full-screen crosshair lines
      // would run along (and hide) its right and bottom edges, so the frame alone guides.
      if (!this.dragStart) this.drawCrosshair(this.cursor);
      if (this.init.showMagnifier || this.init.mode === "color") this.drawMagnifier(this.cursor);
    }

    if (!this.finished) this.drawHint();
  }

  private drawLabel(text: string, x: number, y: number, anchor: Rect, below = false) {
    const { ctx, dpr, canvas } = this;
    ctx.font = `${12 * dpr}px ${getComputedStyle(document.body).fontFamily}`;
    const padX = 6 * dpr;
    const w = ctx.measureText(text).width + padX * 2;
    const h = 20 * dpr;
    let bx = x;
    let by = y - h;
    if (by < 0) by = anchor.y + 4 * dpr;
    if (below) by = Math.min(y - h, canvas.height - h);
    bx = Math.max(0, Math.min(bx, canvas.width - w));
    ctx.fillStyle = "rgba(20,22,26,0.9)";
    ctx.fillRect(bx, by, w, h);
    ctx.fillStyle = "#fff";
    ctx.textBaseline = "middle";
    ctx.fillText(text, bx + padX, by + h / 2);
  }

  /** Small square handles on the selection's corners, so the frame reads as a whole box. */
  private drawCorners(r: Rect) {
    const { ctx, dpr } = this;
    const size = 6 * dpr;
    ctx.fillStyle = "#ffffff";
    ctx.strokeStyle = "#4c8dff";
    ctx.lineWidth = 1.5 * dpr;
    for (const [x, y] of [
      [r.x, r.y],
      [r.x + r.width, r.y],
      [r.x, r.y + r.height],
      [r.x + r.width, r.y + r.height],
    ] as const) {
      ctx.fillRect(x - size / 2, y - size / 2, size, size);
      ctx.strokeRect(x - size / 2, y - size / 2, size, size);
    }
  }

  private drawCrosshair(p: Point) {
    const { ctx, canvas, dpr } = this;
    const x = Math.round(p.x) + 0.5;
    const y = Math.round(p.y) + 0.5;
    ctx.setLineDash([]);
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(canvas.width, y);
    ctx.moveTo(x, 0);
    ctx.lineTo(x, canvas.height);
    // dark halo under a light line: visible on white pages and dark consoles alike
    ctx.strokeStyle = "rgba(0,0,0,0.55)";
    ctx.lineWidth = 3 * dpr;
    ctx.stroke();
    ctx.strokeStyle = "rgba(255,255,255,0.95)";
    ctx.lineWidth = dpr;
    ctx.stroke();
  }

  private drawMagnifier(p: Point) {
    const { ctx, canvas, dpr } = this;
    const zoom = 8;
    const grid = 15; // source pixels per side
    const size = grid * zoom * dpr;
    const half = Math.floor(grid / 2);
    let x = p.x + 24 * dpr;
    let y = p.y + 24 * dpr;
    const labelH = 26 * dpr;
    if (x + size > canvas.width) x = p.x - 24 * dpr - size;
    if (y + size + labelH > canvas.height) y = p.y - 24 * dpr - size - labelH;
    x = Math.max(0, x);
    y = Math.max(0, y);

    ctx.save();
    ctx.imageSmoothingEnabled = false;
    ctx.fillStyle = "#000";
    ctx.fillRect(x, y, size, size + labelH);
    ctx.drawImage(
      this.frame,
      Math.floor(p.x) - half,
      Math.floor(p.y) - half,
      grid,
      grid,
      x,
      y,
      size,
      size,
    );
    // pixel grid
    ctx.strokeStyle = "rgba(255,255,255,0.12)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let i = 1; i < grid; i++) {
      const o = i * zoom * dpr;
      ctx.moveTo(x + o + 0.5, y);
      ctx.lineTo(x + o + 0.5, y + size);
      ctx.moveTo(x, y + o + 0.5);
      ctx.lineTo(x + size, y + o + 0.5);
    }
    ctx.stroke();
    // centre pixel
    const c = half * zoom * dpr;
    ctx.strokeStyle = "#4c8dff";
    ctx.lineWidth = 2 * dpr;
    ctx.strokeRect(x + c, y + c, zoom * dpr, zoom * dpr);
    // border + readout
    ctx.strokeStyle = "rgba(255,255,255,0.6)";
    ctx.lineWidth = dpr;
    ctx.strokeRect(x + 0.5, y + 0.5, size - 1, size + labelH - 1);
    const [r, g, b] = this.pixelAt(p);
    const gx = Math.floor(p.x) + this.init.monitor.x;
    const gy = Math.floor(p.y) + this.init.monitor.y;
    ctx.fillStyle = "#fff";
    ctx.font = `${11 * dpr}px ${getComputedStyle(document.body).fontFamily}`;
    ctx.textBaseline = "middle";
    ctx.fillText(`${rgbToHex(r, g, b)}   ${gx}, ${gy}`, x + 8 * dpr, y + size + labelH / 2);
    ctx.fillStyle = rgbToHex(r, g, b);
    ctx.fillRect(x + size - 22 * dpr, y + size + 5 * dpr, 16 * dpr, labelH - 10 * dpr);
    ctx.restore();
  }

  private drawHint() {
    const { ctx, canvas, dpr } = this;
    ctx.font = `${12 * dpr}px ${getComputedStyle(document.body).fontFamily}`;
    const padX = 10 * dpr;
    const w = ctx.measureText(this.hint).width + padX * 2;
    const h = 26 * dpr;
    const x = (canvas.width - w) / 2;
    const y = 18 * dpr;
    ctx.fillStyle = "rgba(20,22,26,0.82)";
    ctx.fillRect(x, y, w, h);
    ctx.fillStyle = "#e8e9ec";
    ctx.textBaseline = "middle";
    ctx.fillText(this.hint, x + padX, y + h / 2);
  }
}

const overlay = new Overlay();
overlay.start().catch(async (err) => {
  console.error("overlay failed", err);
  await cancelCapture();
});
