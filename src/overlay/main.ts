// Capture overlay: one instance per monitor. Paints the frozen frame, lets the user
// drag a region, click a window or a pixel, and hands the result to Rust.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { cancelCapture, copyText, elementChain, finishCapture, overlayIdle, overlayInit, overlayReady } from "$lib/ipc";
import { fetchRawToCanvas } from "$lib/image";
import { rectContains, rectEdges, rectEquals, rectFromPoints, rectIntersect, rectRound, snapPoint, type Edge } from "$lib/geometry";
import { rgbToHex } from "$lib/image";
import type { OverlayInit, Rect, WindowInfo } from "$lib/types";
import "./overlay.css";

const win = getCurrentWindow();
const label = win.label;

interface Point {
  x: number;
  y: number;
}

// "reduce motion" in the OS: no gliding or fading, and a still (not travelling) border
const REDUCED_MOTION = matchMedia("(prefers-reduced-motion: reduce)").matches;
const DIM = 0.38;
const DIM_COLOR_MODE = 0.08;
const FADE_MS = REDUCED_MOTION ? 1 : 150;
const GLIDE_MS = REDUCED_MOTION ? 1 : 120;
const FLASH_MS = 140;
const SNAP_PX = 8;
/** Pause this long over a window before asking for its parts (UI Automation is cross-process). */
const ELEMENT_DELAY_MS = 60;
/** Smallest part picked automatically (px at 100% scale); smaller ones are a wheel-scroll away. */
const MIN_PART = { w: 40, h: 20 };
const GRADIENT = ["#4c8dff", "#7b5cff", "#35d0ff"];

const easeOutCubic = (t: number) => 1 - Math.pow(1 - t, 3);
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const lerpRect = (a: Rect, b: Rect, t: number): Rect => ({
  x: lerp(a.x, b.x, t),
  y: lerp(a.y, b.y, t),
  width: lerp(a.width, b.width, t),
  height: lerp(a.height, b.height, t),
});

/** Short synthesized shutter click (no audio asset): band-passed noise with a fast decay. */
function playShutter() {
  try {
    const ac = new AudioContext();
    const len = Math.floor(ac.sampleRate * 0.09);
    const buf = ac.createBuffer(1, len, ac.sampleRate);
    const data = buf.getChannelData(0);
    for (let i = 0; i < len; i++) data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 3);
    const src = ac.createBufferSource();
    src.buffer = buf;
    const band = ac.createBiquadFilter();
    band.type = "bandpass";
    band.frequency.value = 2400;
    band.Q.value = 0.8;
    const gain = ac.createGain();
    gain.gain.value = 0.45;
    src.connect(band).connect(gain).connect(ac.destination);
    src.start();
    setTimeout(() => void ac.close(), 400);
  } catch {
    // audio unavailable: the flash is enough
  }
}

/** Theme colours for labels and the hint pill (the dim layer itself stays dark). */
function chromeColors() {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    bg: v("--chrome-bg", "rgba(20,22,26,0.9)"),
    fg: v("--chrome-fg", "#ffffff"),
    border: v("--chrome-border", "rgba(255,255,255,0.14)"),
    font: getComputedStyle(document.body).fontFamily,
  };
}

class Overlay {
  private canvas = document.createElement("canvas");
  private ctx: CanvasRenderingContext2D;
  /** CPU-side copy, only for reading pixel colours. */
  private frame!: HTMLCanvasElement;
  private frameCtx!: CanvasRenderingContext2D;
  /** GPU-friendly copies for drawing every frame: the frozen screen, and the same dimmed. */
  private clean!: HTMLCanvasElement;
  private dimmed!: HTMLCanvasElement;
  private init!: OverlayInit;
  /**
   * Frame pixels per CSS pixel. Measured from the canvas rather than read once from
   * devicePixelRatio: the window may still be settling onto a monitor with different scaling
   * when the page loads, and a stale ratio sends every hover to the wrong place.
   */
  private get dpr(): number {
    const css = this.canvas.clientWidth || window.innerWidth;
    return css > 0 && this.canvas.width > 0 ? this.canvas.width / css : window.devicePixelRatio || 1;
  }
  private chrome!: ReturnType<typeof chromeColors>;
  private cursor: Point | null = null; // local physical px
  private dragStart: Point | null = null; // global physical px
  private selection: Rect | null = null; // global physical px, while dragging
  private remoteSelection: Rect | null = null; // selection drawn on another monitor
  private hoverWindow: WindowInfo | null = null;
  private finished = false;
  private raf = 0;
  private looping = false;
  private unlisten: UnlistenFn[] = [];
  /** Removes every listener this overlay added to the (reused) page. */
  private ac = new AbortController();
  private disposed = false;
  private lastEmit = 0;
  private hint = "";
  // animation state
  private fadeStart = 0; // 0 = not faded in yet (dim stays off until the overlay is shown)
  private shown: Rect | null = null; // highlight as drawn (local px), gliding toward the target
  private glideFrom: Rect | null = null;
  private glideTarget: Rect | null = null;
  private glideStart = 0;
  private flashStart = 0;
  private snapGuide: { x: number | null; y: number | null } = { x: null, y: null };
  private vEdges: Edge[] = [];
  private hEdges: Edge[] = [];
  // UI element detection: nested parts of the hovered window, outermost (the window) first
  private chain: Rect[] | null = null;
  private chainWindow: number | null = null;
  private level = 0;
  /** depth chosen with the wheel; kept while moving within the same window */
  private pinnedDepth: number | null = null;
  private querySeq = 0;
  private queryTimer = 0;

  constructor() {
    this.ctx = this.canvas.getContext("2d", { alpha: false })!;
    document.getElementById("app")!.appendChild(this.canvas);
  }

  async start() {
    this.chrome = chromeColors(); // stylesheets are applied by now
    this.init = await overlayInit(label);
    this.frame = await fetchRawToCanvas(this.init.monitor.frameUrl);
    if (this.disposed) return;
    this.frameCtx = this.frame.getContext("2d", { willReadFrequently: true })!;
    const { width, height } = this.frame;
    this.canvas.width = width;
    this.canvas.height = height;
    // One canvas pixel per screen pixel, or the frozen screen looks soft. At fractional
    // scaling (125%, 150%) "100vw" rounds to whole CSS pixels and the canvas gets resampled;
    // an exact CSS size avoids that (and pixelated scaling keeps any remaining fraction sharp).
    const ratio = window.devicePixelRatio || 1;
    const [cssW, cssH] = [width / ratio, height / ratio];
    if (Math.abs(cssW - window.innerWidth) <= 2 && Math.abs(cssH - window.innerHeight) <= 2) {
      this.canvas.style.width = `${cssW}px`;
      this.canvas.style.height = `${cssH}px`;
    }
    this.clean = document.createElement("canvas");
    this.clean.width = width;
    this.clean.height = height;
    this.clean.getContext("2d")!.drawImage(this.frame, 0, 0);
    this.dimmed = document.createElement("canvas");
    this.dimmed.width = width;
    this.dimmed.height = height;
    const dctx = this.dimmed.getContext("2d")!;
    dctx.drawImage(this.frame, 0, 0);
    dctx.fillStyle = `rgba(0,0,0,${this.init.mode === "color" ? DIM_COLOR_MODE : DIM})`;
    dctx.fillRect(0, 0, width, height);
    for (const r of [...this.init.windows.map((w) => w.rect), ...this.init.monitors]) {
      const e = rectEdges(r);
      this.vEdges.push(...e.vertical);
      this.hEdges.push(...e.horizontal);
    }
    this.hint = this.hintText();
    this.render(); // undimmed first frame: identical to the live screen when shown
    this.bind();
    if (this.disposed) return;
    await overlayReady(label);
    this.fadeStart = performance.now();
    this.schedule();
  }

  private hintText(): string {
    switch (this.init.mode) {
      case "window":
        return "Click a window or part of one · scroll for bigger/smaller parts · drag for a region · Esc cancels";
      case "ocr":
        return "Select text to copy · Esc cancels";
      case "pin":
        return "Select a region to pin · Esc cancels";
      case "color":
        return "Click to copy HEX · Shift+click for RGB · Esc cancels";
      case "qr":
        return "Select a QR code or barcode to copy its contents · Esc cancels";
      case "watch":
        return "Select a region, or click a window or part of one, to watch for changes · Esc cancels";
      case "scroll":
        return "Select the part of the page that scrolls, or click it · QuickShot scrolls it for you · Esc cancels";
      case "record":
        return "Select the area to record as a GIF (one screen) · Esc cancels";
      default:
        return "Drag a region (Alt: no snapping) · click a window or part · scroll for bigger/smaller parts · Esc cancels";
    }
  }

  // ---- coordinate helpers ----
  private toLocal(e: MouseEvent): Point {
    return { x: e.clientX * this.dpr, y: e.clientY * this.dpr };
  }
  private toGlobal(p: Point): Point {
    return { x: p.x + this.init.monitor.x, y: p.y + this.init.monitor.y };
  }
  private toLocalRect(r: Rect): Rect {
    return { x: r.x - this.init.monitor.x, y: r.y - this.init.monitor.y, width: r.width, height: r.height };
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

  /** What hovering highlights: the chosen part of the window when known, else the window. */
  private hoverRect(): Rect | null {
    const w = this.hoverWindow;
    if (!w) return null;
    if (this.chain && this.chainWindow === w.id) return this.chain[this.level] ?? w.rect;
    return w.rect;
  }

  /** Ask (debounced) for the parts of the hovered window under the pointer. */
  private queueElementQuery(global: Point) {
    clearTimeout(this.queryTimer);
    const w = this.hoverWindow;
    if (!w || this.dragStart || this.init.mode === "color") return;
    if (this.chainWindow !== w.id) {
      // new window: start from the whole window until its parts arrive
      this.chain = null;
      this.chainWindow = w.id;
      this.pinnedDepth = null;
    }
    this.queryTimer = window.setTimeout(async () => {
      const seq = ++this.querySeq;
      let chain: Rect[];
      try {
        chain = await elementChain(w.id, global.x, global.y);
      } catch {
        return;
      }
      if (seq !== this.querySeq || this.hoverWindow?.id !== w.id || this.dragStart) return; // stale
      // never let a part be bigger than the window it's in (level 0 is the window as listed)
      const parts = chain
        .slice(1)
        .map((r) => rectIntersect(r, w.rect))
        .filter((r): r is Rect => !!r && !rectEquals(r, w.rect))
        .filter((r, i, all) => i === 0 || !rectEquals(r, all[i - 1]!));
      this.chain = parts.length ? [w.rect, ...parts] : null;
      this.level = this.chain ? this.pickLevel(this.chain) : 0;
      this.schedule();
    }, ELEMENT_DELAY_MS);
  }

  /** Deepest part that isn't tiny, or the depth the user picked with the wheel. */
  private pickLevel(chain: Rect[]): number {
    if (this.pinnedDepth !== null) return Math.min(this.pinnedDepth, chain.length - 1);
    for (let i = chain.length - 1; i > 0; i--) {
      const r = chain[i]!;
      if (r.width >= MIN_PART.w * this.dpr && r.height >= MIN_PART.h * this.dpr) return i;
    }
    return 0;
  }

  private onWheel(e: WheelEvent) {
    if (!this.chain || this.dragStart || this.finished) return;
    e.preventDefault();
    // wheel up = bigger (towards the whole window), down = smaller (deeper)
    const next = Math.max(0, Math.min(this.chain.length - 1, this.level + (e.deltaY > 0 ? 1 : -1)));
    if (next === this.level) return;
    this.level = next;
    this.pinnedDepth = next;
    this.schedule();
  }

  private windowAt(global: Point): WindowInfo | null {
    // windows are sorted topmost first
    for (const w of this.init.windows) {
      if (rectContains(w.rect, global.x, global.y)) return w;
    }
    return null;
  }

  /** Snap a global point to nearby window/monitor edges unless Alt is held. */
  private snap(p: Point, e: MouseEvent | null): Point {
    if (e?.altKey || this.init.mode === "color") {
      this.snapGuide = { x: null, y: null };
      return p;
    }
    const s = snapPoint(p.x, p.y, this.vEdges, this.hEdges, SNAP_PX * this.dpr);
    this.snapGuide = { x: s.snapX, y: s.snapY };
    return { x: s.x, y: s.y };
  }

  // ---- events ----
  private bind() {
    const c = this.canvas;
    const signal = this.ac.signal;
    c.addEventListener("mousemove", (e) => this.onMove(e), { signal });
    c.addEventListener("mousedown", (e) => this.onDown(e));
    c.addEventListener("wheel", (e) => this.onWheel(e), { passive: false });
    window.addEventListener("mouseup", (e) => this.onUp(e), { signal });
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
    window.addEventListener("keydown", (e) => this.onKey(e), { signal });
    window.addEventListener(
      "blur",
      () => {
        // keyboard focus moved to another overlay; keep our visuals but drop hover
        if (!this.dragStart) {
          this.hoverWindow = null;
          this.schedule();
        }
      },
      { signal },
    );
    void listen<{ from: string; rect: Rect | null }>("overlay://selection", (ev) => {
      if (ev.payload.from === label) return;
      this.remoteSelection = ev.payload.rect;
      this.schedule();
    }).then((u) => (this.disposed ? u() : this.unlisten.push(u)));
  }

  private onMove(e: MouseEvent) {
    const local = this.toLocal(e);
    this.cursor = local;
    const global = this.toGlobal(local);
    if (this.dragStart) {
      const end = this.snap(global, e);
      this.selection = rectRound(rectFromPoints(this.dragStart.x, this.dragStart.y, end.x, end.y));
      this.hoverWindow = null;
      this.broadcast();
    } else if (this.init.mode !== "color") {
      this.hoverWindow = this.windowAt(global);
      this.queueElementQuery(global);
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
    this.dragStart = this.snap(global, e);
    this.selection = null;
  }

  private async onUp(e: MouseEvent) {
    if (e.button !== 0 || !this.dragStart || this.finished) return;
    const start = this.dragStart;
    this.dragStart = null;
    const global = this.toGlobal(this.toLocal(e));
    const end = this.snap(global, e);
    this.snapGuide = { x: null, y: null };
    const sel = rectRound(rectFromPoints(start.x, start.y, end.x, end.y));
    this.broadcast(null);
    if (sel.width < 4 || sel.height < 4) {
      // a click: capture the highlighted part / window under the cursor, else the whole monitor
      const w = this.windowAt(global);
      const part = w && w.id === this.hoverWindow?.id ? this.hoverRect() : null;
      if (w && part && rectContains(part, global.x, global.y)) return this.finish(part, w.id);
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
          void this.finish(this.hoverRect() ?? this.hoverWindow.rect, this.hoverWindow.id);
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
          this.snapGuide = { x: null, y: null }; // keyboard nudges are exact
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
    this.flashStart = performance.now();
    if (this.init.playSound) playShutter();
    this.schedule();
    // the image is already frozen: a brief flash costs nothing and confirms the shot
    await new Promise((r) => setTimeout(r, FLASH_MS));
    try {
      await finishCapture(rectRound(clipped), windowId);
    } catch (err) {
      console.error(err);
      this.finished = false;
      this.flashStart = 0;
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

  /** Tear down before the page is reused for the next capture. */
  dispose() {
    this.disposed = true;
    this.ac.abort();
    for (const u of this.unlisten) u();
    this.unlisten = [];
    cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.looping = false;
    clearTimeout(this.queryTimer);
    this.canvas.remove();
    // full-screen bitmaps: give the memory back while the page waits hidden
    for (const c of [this.canvas, this.frame, this.clean, this.dimmed]) {
      if (c) c.width = c.height = 0;
    }
  }

  // ---- animation loop ----
  /** Render once on the next frame; keeps looping while something is animating. */
  private schedule() {
    if (this.raf) return;
    this.raf = requestAnimationFrame((now) => {
      this.raf = 0;
      this.render(now);
      if (this.looping) this.schedule();
    });
  }

  private activeRect(): Rect | null {
    return this.selection ?? this.remoteSelection ?? this.hoverRect();
  }

  /** Where the highlight is drawn: glides between hovered windows, follows drags exactly. */
  private updateShown(now: number): boolean {
    const active = this.activeRect();
    const target = active ? this.toLocalRect(active) : null;
    const dragging = !!(this.selection || this.remoteSelection);
    if (!target) {
      this.shown = this.glideTarget = this.glideFrom = null;
      return false;
    }
    if (dragging || !this.shown) {
      this.shown = this.glideTarget = target;
      this.glideFrom = null;
      return false;
    }
    if (!rectEquals(target, this.glideTarget)) {
      this.glideFrom = this.shown;
      this.glideTarget = target;
      this.glideStart = now;
    }
    if (!this.glideFrom) return false;
    const t = Math.min(1, (now - this.glideStart) / GLIDE_MS);
    this.shown = lerpRect(this.glideFrom, target, easeOutCubic(t));
    if (t >= 1) this.glideFrom = null;
    return t < 1;
  }

  private render(now = performance.now()) {
    const { ctx, canvas, dpr } = this;
    const W = canvas.width;
    const H = canvas.height;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.imageSmoothingEnabled = false;

    // dimmed screen, fading in once the overlay is visible
    const fade = this.fadeStart ? Math.min(1, (now - this.fadeStart) / FADE_MS) : 0;
    if (fade < 1) ctx.drawImage(this.clean, 0, 0);
    if (fade > 0) {
      ctx.globalAlpha = easeOutCubic(fade);
      ctx.drawImage(this.dimmed, 0, 0);
      ctx.globalAlpha = 1;
    }

    const gliding = this.updateShown(now);
    const active = this.activeRect();
    const shown = this.shown;
    const vis = shown ? rectIntersect(shown, { x: 0, y: 0, width: W, height: H }) : null;
    if (vis && active) {
      // undimmed window/selection, then the animated border
      ctx.drawImage(this.clean, vis.x, vis.y, vis.width, vis.height, vis.x, vis.y, vis.width, vis.height);
      this.drawBorder(vis, now);
      if (this.selection || this.remoteSelection) this.drawCorners(vis);
      this.drawSnapGuides(vis);
      if (!this.finished) {
        this.drawLabel(`${active.width} × ${active.height}`, vis.x, vis.y - 8 * dpr, vis);
        if (this.hoverWindow && !this.selection) {
          const t = this.hoverWindow.title || this.hoverWindow.appName;
          const part = this.chain && this.level > 0;
          const text = part ? `Part of ${t || "window"} · scroll: bigger/smaller` : t;
          if (text) this.drawLabel(text.slice(0, 90), vis.x, vis.y + vis.height + 30 * dpr, vis, true);
        }
      }
    }

    // ghost of the last region
    if (this.init.lastRegion && !active) {
      const l = this.toLocalRect(this.init.lastRegion);
      ctx.setLineDash([6 * dpr, 4 * dpr]);
      ctx.strokeStyle = "rgba(255,255,255,0.45)";
      ctx.lineWidth = dpr;
      ctx.strokeRect(l.x + 0.5, l.y + 0.5, l.width - 1, l.height - 1);
      ctx.setLineDash([]);
    }

    // capture flash
    const flash = this.flashStart ? Math.min(1, (now - this.flashStart) / FLASH_MS) : 1;
    if (flash < 1 && vis) {
      ctx.fillStyle = `rgba(255,255,255,${0.5 * (1 - flash)})`;
      ctx.fillRect(vis.x, vis.y, vis.width, vis.height);
    }

    if (this.cursor && !this.finished) {
      // While dragging, the cursor sits on the frame's corner: full-screen crosshair lines
      // would run along (and hide) its right and bottom edges, so the frame alone guides.
      if (!this.dragStart) this.drawCrosshair(this.cursor);
      if (this.init.showMagnifier || this.init.mode === "color") this.drawMagnifier(this.cursor);
    }

    if (!this.finished) this.drawHint();

    // keep animating while the border is visible (it moves) or something is still easing
    this.looping = (!!vis && !REDUCED_MOTION) || fade < 1 || gliding || flash < 1;
  }

  /** Glow plus a colour sweep that travels around the rectangle, with marching dashes on top. */
  private drawBorder(r: Rect, time: number) {
    const { ctx, dpr } = this;
    const now = REDUCED_MOTION ? 0 : time;
    const x = r.x + 0.5;
    const y = r.y + 0.5;
    const w = Math.max(0, r.width - 1);
    const h = Math.max(0, r.height - 1);
    ctx.save();
    ctx.setLineDash([]);
    ctx.shadowColor = "rgba(90,130,255,0.9)";
    ctx.shadowBlur = 14 * dpr;
    ctx.lineWidth = 2 * dpr;
    ctx.strokeStyle = "rgba(76,141,255,0.85)";
    ctx.strokeRect(x, y, w, h);
    ctx.restore();

    ctx.save();
    ctx.lineWidth = 2.5 * dpr;
    if (typeof ctx.createConicGradient === "function") {
      const g = ctx.createConicGradient((now / 900) % (Math.PI * 2), x + w / 2, y + h / 2);
      g.addColorStop(0, GRADIENT[0]!);
      g.addColorStop(0.33, GRADIENT[1]!);
      g.addColorStop(0.66, GRADIENT[2]!);
      g.addColorStop(1, GRADIENT[0]!);
      ctx.strokeStyle = g;
    } else {
      ctx.strokeStyle = GRADIENT[0]!;
    }
    ctx.strokeRect(x, y, w, h);
    ctx.setLineDash([10 * dpr, 10 * dpr]);
    ctx.lineDashOffset = -((now / 25) % (20 * dpr));
    ctx.lineWidth = 1.2 * dpr;
    ctx.strokeStyle = "rgba(255,255,255,0.6)";
    ctx.strokeRect(x, y, w, h);
    ctx.restore();
  }

  /** Accent lines along edges the selection snapped to. */
  private drawSnapGuides(sel: Rect) {
    const { ctx, dpr, canvas } = this;
    const { x, y } = this.snapGuide;
    if (x === null && y === null) return;
    ctx.save();
    ctx.strokeStyle = "rgba(53,208,255,0.9)";
    ctx.lineWidth = dpr;
    ctx.setLineDash([4 * dpr, 4 * dpr]);
    ctx.beginPath();
    const ext = 40 * dpr;
    if (x !== null) {
      const lx = x - this.init.monitor.x + 0.5;
      ctx.moveTo(lx, Math.max(0, sel.y - ext));
      ctx.lineTo(lx, Math.min(canvas.height, sel.y + sel.height + ext));
    }
    if (y !== null) {
      const ly = y - this.init.monitor.y + 0.5;
      ctx.moveTo(Math.max(0, sel.x - ext), ly);
      ctx.lineTo(Math.min(canvas.width, sel.x + sel.width + ext), ly);
    }
    ctx.stroke();
    ctx.restore();
  }

  private roundRect(x: number, y: number, w: number, h: number, r: number) {
    const { ctx } = this;
    ctx.beginPath();
    if (typeof ctx.roundRect === "function") ctx.roundRect(x, y, w, h, r);
    else ctx.rect(x, y, w, h);
  }

  private drawLabel(text: string, x: number, y: number, anchor: Rect, below = false) {
    const { ctx, dpr, canvas } = this;
    ctx.font = `600 ${12 * dpr}px ${this.chrome.font}`;
    const padX = 8 * dpr;
    const w = ctx.measureText(text).width + padX * 2;
    const h = 22 * dpr;
    let bx = x;
    let by = y - h;
    if (by < 0) by = anchor.y + 4 * dpr;
    if (below) by = Math.min(y - h, canvas.height - h);
    bx = Math.max(0, Math.min(bx, canvas.width - w));
    this.roundRect(bx, by, w, h, 6 * dpr);
    ctx.fillStyle = this.chrome.bg;
    ctx.fill();
    ctx.lineWidth = dpr;
    ctx.strokeStyle = this.chrome.border;
    ctx.stroke();
    ctx.fillStyle = this.chrome.fg;
    ctx.textBaseline = "middle";
    ctx.fillText(text, bx + padX, by + h / 2);
  }

  /** Small square handles on the selection's corners, so the frame reads as a whole box. */
  private drawCorners(r: Rect) {
    const { ctx, dpr } = this;
    const size = 7 * dpr;
    ctx.fillStyle = "#ffffff";
    ctx.strokeStyle = GRADIENT[0]!;
    ctx.lineWidth = 1.5 * dpr;
    ctx.setLineDash([]);
    for (const [x, y] of [
      [r.x, r.y],
      [r.x + r.width, r.y],
      [r.x, r.y + r.height],
      [r.x + r.width, r.y + r.height],
    ] as const) {
      this.roundRect(x - size / 2, y - size / 2, size, size, 2 * dpr);
      ctx.fill();
      ctx.stroke();
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
    const grid = 17; // source pixels per side (odd, so one pixel sits in the centre)
    const size = grid * zoom * dpr;
    const half = Math.floor(grid / 2);
    let x = p.x + 24 * dpr;
    let y = p.y + 24 * dpr;
    const labelH = 26 * dpr;
    if (x + size > canvas.width) x = p.x - 24 * dpr - size;
    if (y + size + labelH > canvas.height) y = p.y - 24 * dpr - size - labelH;
    x = Math.max(0, x);
    y = Math.max(0, y);
    const radius = 8 * dpr;

    ctx.save();
    ctx.shadowColor = "rgba(0,0,0,0.45)";
    ctx.shadowBlur = 16 * dpr;
    this.roundRect(x, y, size, size + labelH, radius);
    ctx.fillStyle = this.chrome.bg;
    ctx.fill();
    ctx.restore();

    ctx.save();
    this.roundRect(x, y, size, size + labelH, radius);
    ctx.clip();
    ctx.imageSmoothingEnabled = false;
    ctx.fillStyle = "#000";
    ctx.fillRect(x, y, size, size);
    ctx.drawImage(this.clean, Math.floor(p.x) - half, Math.floor(p.y) - half, grid, grid, x, y, size, size);
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
    ctx.strokeStyle = GRADIENT[0]!;
    ctx.lineWidth = 2 * dpr;
    ctx.strokeRect(x + c, y + c, zoom * dpr, zoom * dpr);
    // readout
    const [r, g, b] = this.pixelAt(p);
    const gx = Math.floor(p.x) + this.init.monitor.x;
    const gy = Math.floor(p.y) + this.init.monitor.y;
    // swatch first, then "#rrggbb  x, y" so the text never runs under it
    const sw = labelH - 12 * dpr;
    ctx.fillStyle = rgbToHex(r, g, b);
    this.roundRect(x + 7 * dpr, y + size + 6 * dpr, sw, sw, 3 * dpr);
    ctx.fill();
    ctx.strokeStyle = this.chrome.border;
    ctx.lineWidth = dpr;
    ctx.stroke();
    ctx.fillStyle = this.chrome.fg;
    ctx.font = `${10.5 * dpr}px ${this.chrome.font}`;
    ctx.textBaseline = "middle";
    ctx.fillText(`${rgbToHex(r, g, b)}  ${gx}, ${gy}`, x + 7 * dpr + sw + 6 * dpr, y + size + labelH / 2);
    ctx.restore();

    this.roundRect(x + 0.5, y + 0.5, size - 1, size + labelH - 1, radius);
    ctx.strokeStyle = this.chrome.border;
    ctx.lineWidth = dpr;
    ctx.stroke();
  }

  private drawHint() {
    const { ctx, canvas, dpr } = this;
    ctx.font = `${12 * dpr}px ${this.chrome.font}`;
    const padX = 12 * dpr;
    const w = ctx.measureText(this.hint).width + padX * 2;
    const h = 28 * dpr;
    const x = (canvas.width - w) / 2;
    const y = 18 * dpr;
    this.roundRect(x, y, w, h, h / 2);
    ctx.fillStyle = this.chrome.bg;
    ctx.fill();
    ctx.lineWidth = dpr;
    ctx.strokeStyle = this.chrome.border;
    ctx.stroke();
    ctx.fillStyle = this.chrome.fg;
    ctx.textBaseline = "middle";
    ctx.fillText(this.hint, x + padX, y + h / 2);
  }
}

declare global {
  interface Window {
    /** Built ahead of the first capture: wait for a start instead of starting now. */
    __QS_OVERLAY_WARM?: boolean;
  }
}

// The page outlives captures: Rust keeps it loaded (hidden) and sends "start" for the next
// one, which gets a fresh Overlay; "reset" ends the current one when the capture is over.
let current: Overlay | null = null;

function run() {
  current?.dispose();
  const overlay = new Overlay();
  current = overlay;
  overlay.start().catch(async (err) => {
    if (overlay !== current) return; // superseded by a newer capture
    console.error("overlay failed", err);
    await cancelCapture();
  });
}

function reset() {
  current?.dispose();
  current = null;
  void overlayIdle(label);
}

void Promise.all([listen("overlay://start", run), listen("overlay://reset", reset)]).then(() => {
  if (window.__QS_OVERLAY_WARM) void overlayIdle(label);
  else run();
});
