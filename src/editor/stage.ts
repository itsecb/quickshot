// Konva stage: renders a Document, handles selection, drawing tools, zoom and export.
import Konva from "konva";
import type { Beautify, Rect } from "$lib/types";
import { beautify } from "./beautify";
import { rectFromPoints, rectIntersect, rectRound } from "$lib/geometry";
import {
  addShape,
  hitShape,
  moveShape,
  newId,
  nextBadgeNumber,
  removeShape,
  updateShape,
  withShapes,
  type ArrowShape,
  type BadgeKind,
  type BadgeShape,
  type BlurShape,
  type CalloutShape,
  type MagnifyShape,
  type Document,
  type LineShape,
  type Shape,
  type TextShape,
  type ToolId,
} from "./document";

export interface Style {
  stroke: string;
  strokeWidth: number;
  fontSize: number;
  fontFamily: string;
  shadow: boolean;
  blurAmount: number;
  blurMode: "pixelate" | "blur";
  badgeSize: number;
  badgeKind: BadgeKind;
  fill: boolean;
  /** spotlight: how dark everything outside the box gets (0-1) */
  dim: number;
  /** magnify: inset scale */
  zoom: number;
}

export interface StageEvents {
  onCommit: (doc: Document) => void;
  onPreview: (doc: Document) => void;
  onSelect: (id: string | null) => void;
  onStatus: (text: string) => void;
  onZoom: (zoom: number) => void;
}

interface Point {
  x: number;
  y: number;
}

type BoxShape = Extract<Shape, { type: "rect" | "ellipse" | "blur" | "spotlight" }>;
const isBox = (s: Shape): s is BoxShape =>
  s.type === "rect" || s.type === "ellipse" || s.type === "blur" || s.type === "spotlight";

/** Dark or light text, whichever reads better on `hex`. */
function textColorFor(hex: string): string {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(hex);
  if (!m) return "#ffffff";
  const [r, g, b] = [m[1], m[2], m[3]].map((v) => parseInt(v!, 16) / 255);
  return 0.299 * r! + 0.587 * g! + 0.114 * b! > 0.62 ? "#1d1f23" : "#ffffff";
}

const CALLOUT_PAD = 10;

/**
 * One dim layer for every spotlight, cut out with even-odd holes. It reads the spotlight
 * nodes' live geometry, so holes follow while a spotlight is dragged or resized.
 */
function spotlightDim(layer: Konva.Layer, width: number, height: number, dim: number): Konva.Shape {
  return new Konva.Shape({
    name: "spot-dim",
    listening: false,
    sceneFunc: (ctx) => {
      const c = (ctx as unknown as { _context: CanvasRenderingContext2D })._context;
      c.save();
      c.beginPath();
      c.rect(0, 0, width, height);
      for (const n of layer.find(".spot")) {
        const w = n.width() * n.scaleX();
        const h = n.height() * n.scaleY();
        const r = Math.min((n as Konva.Rect).cornerRadius() as number, w / 2, h / 2);
        c.roundRect(n.x(), n.y(), w, h, r);
      }
      c.fillStyle = `rgba(0,0,0,${dim})`;
      c.fill("evenodd");
      c.restore();
    },
  });
}

/** Source outline and connector for a magnify inset; follows the inset while it's dragged. */
function magnifyGuide(layer: Konva.Layer, s: MagnifyShape): Konva.Shape {
  return new Konva.Shape({
    name: "magnify-guide",
    listening: false,
    sceneFunc: (ctx) => {
      const c = (ctx as unknown as { _context: CanvasRenderingContext2D })._context;
      const inset = layer.findOne(`#${s.id}`);
      const ix = inset ? inset.x() : s.x;
      const iy = inset ? inset.y() : s.y;
      const iw = s.src.width * s.scale * (inset ? inset.scaleX() : 1);
      const ih = s.src.height * s.scale * (inset ? inset.scaleY() : 1);
      c.save();
      c.strokeStyle = s.stroke;
      c.lineWidth = 2;
      c.setLineDash([6, 4]);
      c.strokeRect(s.src.x, s.src.y, s.src.width, s.src.height);
      // connector between the nearest points of the two boxes' centres
      const sx = s.src.x + s.src.width / 2;
      const sy = s.src.y + s.src.height / 2;
      const tx = ix + iw / 2;
      const ty = iy + ih / 2;
      c.beginPath();
      c.moveTo(Math.min(Math.max(tx, s.src.x), s.src.x + s.src.width), Math.min(Math.max(ty, s.src.y), s.src.y + s.src.height));
      c.lineTo(Math.min(Math.max(sx, ix), ix + iw), Math.min(Math.max(sy, iy), iy + ih));
      c.stroke();
      c.restore();
    },
  });
}

const SHADOW = { shadowColor: "rgba(0,0,0,0.55)", shadowBlur: 6, shadowOffset: { x: 2, y: 2 }, shadowOpacity: 1 };

function shadowProps(on: boolean) {
  return on ? SHADOW : {};
}

/** Build Konva nodes for a document. Shared by the live stage and the export stage. */
export function buildShapeNodes(layer: Konva.Layer, doc: Document, image: HTMLCanvasElement) {
  layer.destroyChildren();
  // The screenshot lives in the same layer so blend modes (highlighter multiply) see it.
  layer.add(new Konva.Image({ image, x: 0, y: 0, listening: false, name: "bg" }));
  // spotlights dim everything under the annotations, so they sit right above the screenshot
  const spots = doc.shapes.filter((s) => s.type === "spotlight");
  if (spots.length) layer.add(spotlightDim(layer, image.width, image.height, Math.max(...spots.map((s) => s.dim))));
  for (const s of doc.shapes) {
    if (s.type === "magnify") layer.add(magnifyGuide(layer, s));
    const node = buildNode(s, image);
    if (node) layer.add(node as Konva.Group);
  }
}

export function buildNode(s: Shape, image: HTMLCanvasElement): Konva.Node | null {
  const common = { id: s.id, name: "shape", opacity: s.opacity };
  switch (s.type) {
    case "rect":
      return new Konva.Rect({
        ...common,
        x: s.x,
        y: s.y,
        width: s.width,
        height: s.height,
        stroke: s.stroke,
        strokeWidth: s.strokeWidth,
        cornerRadius: s.radius,
        fill: s.fill ?? undefined,
        strokeScaleEnabled: false,
        ...shadowProps(s.shadow),
      });
    case "ellipse":
      return new Konva.Ellipse({
        ...common,
        x: s.x + s.width / 2,
        y: s.y + s.height / 2,
        radiusX: Math.max(1, s.width / 2),
        radiusY: Math.max(1, s.height / 2),
        stroke: s.stroke,
        strokeWidth: s.strokeWidth,
        fill: s.fill ?? undefined,
        strokeScaleEnabled: false,
        ...shadowProps(s.shadow),
      });
    case "line":
      return new Konva.Line({
        ...common,
        points: [s.x1, s.y1, s.x2, s.y2],
        stroke: s.stroke,
        strokeWidth: s.strokeWidth,
        lineCap: "round",
        hitStrokeWidth: Math.max(12, s.strokeWidth * 2),
        ...shadowProps(s.shadow),
      });
    case "arrow":
      return new Konva.Arrow({
        ...common,
        points: [s.x1, s.y1, s.x2, s.y2],
        stroke: s.stroke,
        fill: s.stroke,
        strokeWidth: s.strokeWidth,
        pointerLength: s.headSize,
        pointerWidth: s.headSize * 0.8,
        lineCap: "round",
        lineJoin: "round",
        hitStrokeWidth: Math.max(12, s.strokeWidth * 2),
        ...shadowProps(s.shadow),
      });
    case "pen":
      return new Konva.Line({
        ...common,
        points: s.points,
        stroke: s.stroke,
        strokeWidth: s.strokeWidth,
        tension: 0.35,
        lineCap: "round",
        lineJoin: "round",
        hitStrokeWidth: Math.max(12, s.strokeWidth * 2),
        ...shadowProps(s.shadow),
      });
    case "highlighter":
      return new Konva.Line({
        ...common,
        points: s.points,
        stroke: s.stroke,
        strokeWidth: s.strokeWidth,
        lineCap: "round",
        lineJoin: "round",
        globalCompositeOperation: "multiply",
        opacity: 0.55,
        hitStrokeWidth: Math.max(12, s.strokeWidth),
      });
    case "text": {
      const label = new Konva.Label({ ...common, x: s.x, y: s.y, rotation: s.rotation });
      label.add(
        new Konva.Tag({
          fill: s.background ?? "transparent",
          cornerRadius: 4,
          ...(s.background ? shadowProps(s.shadow) : {}),
        }),
      );
      label.add(
        new Konva.Text({
          text: s.text || " ",
          fontSize: s.fontSize,
          fontFamily: s.fontFamily,
          fontStyle: "bold",
          fill: s.stroke,
          padding: 6,
          lineHeight: 1.25,
          width: s.width ?? undefined,
          ...(s.background ? {} : shadowProps(s.shadow)),
        }),
      );
      return label;
    }
    case "blur": {
      const crop = rectIntersect(
        { x: s.x, y: s.y, width: s.width, height: s.height },
        { x: 0, y: 0, width: image.width, height: image.height },
      );
      if (!crop) return null;
      const node = new Konva.Image({
        ...common,
        image,
        x: crop.x,
        y: crop.y,
        width: crop.width,
        height: crop.height,
        crop: { x: crop.x, y: crop.y, width: crop.width, height: crop.height },
      });
      if (s.mode === "blur") {
        node.filters([Konva.Filters.Blur]);
        node.blurRadius(Math.max(1, s.amount));
      } else {
        node.filters([Konva.Filters.Pixelate]);
        node.pixelSize(Math.max(2, s.amount));
      }
      node.cache({ pixelRatio: 1 });
      return node;
    }
    case "spotlight":
      // invisible but hittable: the visible effect is the shared dim layer
      return new Konva.Rect({
        ...common,
        name: "shape spot",
        x: s.x,
        y: s.y,
        width: s.width,
        height: s.height,
        cornerRadius: s.radius,
        fill: "rgba(0,0,0,0)",
      });
    case "magnify": {
      const src = rectIntersect(s.src, { x: 0, y: 0, width: image.width, height: image.height }) ?? s.src;
      const w = src.width * s.scale;
      const h = src.height * s.scale;
      const g = new Konva.Group({ ...common, x: s.x, y: s.y });
      g.add(new Konva.Rect({ width: w, height: h, fill: "#ffffff", cornerRadius: s.radius, ...shadowProps(s.shadow) }));
      g.add(new Konva.Image({ image, crop: src, width: w, height: h, cornerRadius: s.radius }));
      g.add(new Konva.Rect({ width: w, height: h, stroke: s.stroke, strokeWidth: 3, cornerRadius: s.radius, strokeScaleEnabled: false }));
      return g;
    }
    case "callout": {
      const g = new Konva.Group({ ...common, x: s.x, y: s.y });
      const text = new Konva.Text({
        text: s.text || " ",
        x: CALLOUT_PAD,
        y: CALLOUT_PAD,
        width: Math.max(20, s.width - CALLOUT_PAD * 2),
        fontSize: s.fontSize,
        fontFamily: s.fontFamily,
        fontStyle: "bold",
        lineHeight: 1.25,
        fill: textColorFor(s.stroke),
      });
      const bubble = new Konva.Shape({
        fill: s.stroke,
        ...shadowProps(s.shadow),
        sceneFunc: (ctx, shape) => {
          const w = s.width * g.scaleX();
          const h = text.height() + CALLOUT_PAD * 2;
          const r = Math.min(10, h / 2);
          // tail tip in group coordinates (the group moves; the tip stays where it points)
          const tx = s.tipX - g.x();
          const ty = s.tipY - g.y();
          ctx.beginPath();
          ctx.roundRect(0, 0, w, h, r);
          const inside = tx >= 0 && tx <= w && ty >= 0 && ty <= h;
          if (!inside) {
            const half = Math.min(14, w / 4, h / 4);
            const cx = Math.min(Math.max(tx, r + half), w - r - half);
            const cy = Math.min(Math.max(ty, r + half), h - r - half);
            // leave from the side facing the tip
            const dx = tx < 0 ? -tx : tx > w ? tx - w : 0;
            const dy = ty < 0 ? -ty : ty > h ? ty - h : 0;
            if (dy >= dx) {
              const ey = ty < 0 ? 0 : h;
              ctx.moveTo(cx - half, ey);
              ctx.lineTo(tx, ty);
              ctx.lineTo(cx + half, ey);
            } else {
              const ex = tx < 0 ? 0 : w;
              ctx.moveTo(ex, cy - half);
              ctx.lineTo(tx, ty);
              ctx.lineTo(ex, cy + half);
            }
            ctx.closePath();
          }
          ctx.fillStrokeShape(shape);
        },
      });
      g.add(bubble);
      g.add(text);
      return g;
    }
    case "badge": {
      const g = new Konva.Group({ ...common, x: s.x, y: s.y });
      const border = Math.max(1.5, s.size / 14);
      const tail = badgeTailPoints(s, s.tip);
      if (tail) {
        // drawn first: the badge covers the tail's base, leaving a wedge pointing at the tip
        g.add(
          new Konva.Line({
            name: "badge-tail",
            points: tail,
            closed: true,
            fill: s.stroke,
            stroke: "#ffffff",
            strokeWidth: border,
            lineJoin: "round",
            ...shadowProps(s.shadow),
          }),
        );
      }
      const kind = s.kind ?? "circle";
      const body = { name: "badge-body", fill: s.stroke, stroke: "#ffffff", strokeWidth: border, ...shadowProps(s.shadow) };
      g.add(
        kind === "circle"
          ? new Konva.Circle({ ...body, radius: s.size / 2 })
          : new Konva.Rect({
              ...body,
              x: -s.size / 2,
              y: -s.size / 2,
              width: s.size,
              height: s.size,
              cornerRadius: kind === "rounded" ? s.size * 0.28 : s.size * 0.06,
            }),
      );
      g.add(
        new Konva.Text({
          text: String(s.n),
          fontSize: s.size * (s.n > 99 ? 0.42 : s.n > 9 ? 0.5 : 0.58),
          fontFamily: "Segoe UI, -apple-system, Helvetica, Arial, sans-serif",
          fontStyle: "bold",
          fill: s.textColor,
          width: s.size,
          height: s.size,
          offsetX: s.size / 2,
          offsetY: s.size / 2,
          align: "center",
          verticalAlign: "middle",
          listening: false,
        }),
      );
      return g;
    }
  }
}

/**
 * Tail of a numbered step, relative to its centre: a wedge from a base across the middle of
 * the badge to the tip. Null when there's no tip or it's inside the badge.
 */
export function badgeTailPoints(s: BadgeShape, tip: { x: number; y: number } | null | undefined): number[] | null {
  if (!tip) return null;
  const [dx, dy] = [tip.x - s.x, tip.y - s.y];
  const len = Math.hypot(dx, dy);
  if (len <= s.size / 2) return null;
  const [ux, uy] = [dx / len, dy / len];
  const half = s.size * 0.3;
  return [-uy * half, ux * half, dx, dy, uy * half, -ux * half];
}

/** Render a document to a canvas at exact image pixels (crop applied). */
export function renderDocument(doc: Document, image: HTMLCanvasElement): HTMLCanvasElement {
  const host = document.createElement("div");
  const stage = new Konva.Stage({ container: host, width: doc.imageWidth, height: doc.imageHeight });
  const shapes = new Konva.Layer({ listening: false });
  stage.add(shapes);
  buildShapeNodes(shapes, doc, image);
  const crop = doc.crop ?? { x: 0, y: 0, width: doc.imageWidth, height: doc.imageHeight };
  const out = stage.toCanvas({ x: crop.x, y: crop.y, width: crop.width, height: crop.height, pixelRatio: 1 });
  stage.destroy();
  return out;
}

export class EditorStage {
  stage: Konva.Stage;
  private shapeLayer = new Konva.Layer();
  private uiLayer = new Konva.Layer();
  private transformer: Konva.Transformer;
  private cropMask: Konva.Group;
  private anchors: Konva.Circle[] = [];
  private draft: Konva.Node | null = null;
  private draftLabel: Konva.Label | null = null;
  private drawing: { start: Point; shape: Shape | null; kind: ToolId } | null = null;
  /** An existing shape being dragged while a drawing tool is active (see `pickSameKind`). */
  private moving: { id: string; start: Point; node: Konva.Node; origin: Point; moved: boolean } | null = null;
  private textarea: HTMLTextAreaElement | null = null;
  zoom = 1;
  tool: ToolId = "select";
  selectedId: string | null = null;
  doc: Document;
  style: Style;

  constructor(
    private container: HTMLDivElement,
    private image: HTMLCanvasElement,
    doc: Document,
    style: Style,
    private events: StageEvents,
  ) {
    this.doc = doc;
    this.style = style;
    this.stage = new Konva.Stage({ container, width: container.clientWidth, height: container.clientHeight });
    this.stage.add(this.shapeLayer);
    this.stage.add(this.uiLayer);
    this.transformer = new Konva.Transformer({
      rotateEnabled: false,
      keepRatio: false,
      ignoreStroke: true,
      anchorSize: 9,
      anchorStroke: "#4c8dff",
      anchorFill: "#ffffff",
      borderStroke: "#4c8dff",
      padding: 2,
    });
    this.uiLayer.add(this.transformer);
    this.cropMask = new Konva.Group({ listening: false, visible: false });
    this.uiLayer.add(this.cropMask);
    this.bindStage();
    this.bindTransformer();
    this.rebuild();
    this.fit();
    new ResizeObserver(() => this.resize()).observe(container);
  }

  // ---------- document ----------
  setDocument(doc: Document, preserveSelection = true) {
    this.doc = doc;
    this.rebuild();
    if (!preserveSelection || (this.selectedId && !doc.shapes.some((s) => s.id === this.selectedId))) {
      this.select(null);
    } else {
      this.refreshSelection();
    }
  }

  /** Add many shapes as one undoable step (e.g. auto-redaction). */
  addShapes(shapes: Shape[]) {
    if (!shapes.length) return;
    this.commit(withShapes(this.doc, [...this.doc.shapes, ...shapes]));
  }

  private commit(doc: Document) {
    this.doc = doc;
    this.rebuild();
    this.refreshSelection();
    this.events.onCommit(doc);
  }

  private rebuild() {
    buildShapeNodes(this.shapeLayer, this.doc, this.image);
    this.shapeLayer.find(".shape").forEach((n) => n.draggable(this.tool === "select"));
    this.shapeLayer.listening(this.tool === "select");
    this.updateCropMask();
    this.shapeLayer.batchDraw();
  }

  // ---------- tools ----------
  setTool(tool: ToolId) {
    this.cancelDraft();
    this.tool = tool;
    if (tool !== "select") this.select(null);
    this.shapeLayer.listening(tool === "select");
    this.shapeLayer.find(".shape").forEach((n) => n.draggable(tool === "select"));
    this.container.style.cursor = tool === "select" ? "default" : tool === "text" ? "text" : "crosshair";
    this.updateCropMask();
  }

  /** Update the current style without touching the selected shape (e.g. to show its values). */
  syncStyle(style: Style) {
    this.style = style;
  }

  setStyle(style: Style) {
    const prev = this.style;
    this.style = style;
    // apply only the changed properties to the selected shape
    const s = this.selectedShape();
    if (!s) return;
    const patch: Record<string, unknown> = {};
    if (style.stroke !== prev.stroke) {
      patch.stroke = style.stroke;
      if ((s.type === "rect" || s.type === "ellipse") && s.fill) patch.fill = style.stroke + "33";
    }
    if (style.strokeWidth !== prev.strokeWidth && s.type !== "text" && s.type !== "badge" && s.type !== "blur") {
      patch.strokeWidth = s.type === "highlighter" ? Math.max(14, style.strokeWidth * 4) : style.strokeWidth;
    }
    if (style.shadow !== prev.shadow && s.type !== "blur" && s.type !== "highlighter") patch.shadow = style.shadow;
    if (style.fontSize !== prev.fontSize && (s.type === "text" || s.type === "callout")) patch.fontSize = style.fontSize;
    if (style.dim !== prev.dim && s.type === "spotlight") patch.dim = style.dim;
    if (style.badgeSize !== prev.badgeSize && s.type === "badge") patch.size = style.badgeSize;
    if (style.badgeKind !== prev.badgeKind && s.type === "badge") patch.kind = style.badgeKind;
    if (style.zoom !== prev.zoom && s.type === "magnify") patch.scale = style.zoom;
    if (style.fill !== prev.fill && (s.type === "rect" || s.type === "ellipse")) patch.fill = style.fill ? style.stroke + "33" : null;
    if (s.type === "blur" && (style.blurAmount !== prev.blurAmount || style.blurMode !== prev.blurMode)) {
      patch.amount = style.blurAmount;
      patch.mode = style.blurMode;
    }
    if (Object.keys(patch).length) this.commit(updateShape(this.doc, s.id, patch as Partial<Shape>));
  }

  // ---------- selection ----------
  select(id: string | null) {
    this.selectedId = id;
    this.refreshSelection();
    this.events.onSelect(id);
  }

  selectedShape(): Shape | null {
    return this.doc.shapes.find((s) => s.id === this.selectedId) ?? null;
  }

  private refreshSelection() {
    this.anchors.forEach((a) => a.destroy());
    this.anchors = [];
    const node = this.selectedId ? this.shapeLayer.findOne(`#${this.selectedId}`) : null;
    const shape = this.selectedShape();
    if (!node || !shape) {
      this.transformer.nodes([]);
      this.uiLayer.batchDraw();
      return;
    }
    if (shape.type === "line" || shape.type === "arrow") {
      this.transformer.nodes([]);
      this.addEndpointAnchors(shape);
    } else {
      const resizable = shape.type === "rect" || shape.type === "ellipse" || shape.type === "blur" || shape.type === "spotlight";
      this.transformer.enabledAnchors(
        resizable
          ? ["top-left", "top-right", "bottom-left", "bottom-right", "middle-left", "middle-right", "top-center", "bottom-center"]
          : shape.type === "text" || shape.type === "callout"
            ? ["middle-left", "middle-right"]
            : shape.type === "badge" || shape.type === "magnify"
              ? ["top-left", "top-right", "bottom-left", "bottom-right"]
              : [],
      );
      this.transformer.keepRatio(shape.type === "badge" || shape.type === "magnify");
      this.transformer.rotateEnabled(shape.type === "text");
      // a numbered step resizes around its centre, by its body (not the tail)
      this.transformer.centeredScaling(shape.type === "badge");
      const target = shape.type === "badge" ? ((node as Konva.Group).findOne(".badge-body") ?? node) : node;
      this.transformer.nodes([target]);
      this.transformer.moveToTop();
      if (shape.type === "badge" && shape.tip) this.addTipAnchor(shape);
    }
    this.uiLayer.batchDraw();
  }

  /** Handle at the end of a numbered step's tail: drag to aim it. */
  private addTipAnchor(shape: BadgeShape) {
    const tip = shape.tip!;
    const c = new Konva.Circle({
      x: tip.x,
      y: tip.y,
      radius: 6 / this.zoom,
      fill: "#ffffff",
      stroke: "#4c8dff",
      strokeWidth: 2 / this.zoom,
      draggable: true,
    });
    c.on("dragmove", () => {
      const tail = this.shapeLayer.findOne<Konva.Line>(`#${shape.id} .badge-tail`);
      const pts = badgeTailPoints(shape, { x: c.x(), y: c.y() });
      if (tail && pts) {
        tail.points(pts);
        this.shapeLayer.batchDraw();
      }
    });
    c.on("dragend", () => this.commit(updateShape(this.doc, shape.id, { tip: { x: c.x(), y: c.y() } } as Partial<Shape>)));
    c.on("mouseenter", () => (this.container.style.cursor = "move"));
    c.on("mouseleave", () => (this.container.style.cursor = "default"));
    this.uiLayer.add(c);
    this.anchors.push(c);
  }

  /** Give the selected numbered step a tail (pointing down-right to start with) or remove it. */
  setBadgeTail(on: boolean) {
    const s = this.selectedShape();
    if (s?.type !== "badge" || on === !!s.tip) return;
    const tip = on ? { x: s.x + s.size * 1.8, y: s.y + s.size * 1.8 } : null;
    this.commit(updateShape(this.doc, s.id, { tip } as Partial<Shape>));
  }

  /** Topmost shape of the active tool's kind under `p` (text and callouts count as one kind). */
  private pickSameKind(p: Point): Shape | null {
    const kinds: Partial<Record<ToolId, Shape["type"][]>> = {
      text: ["text", "callout"],
      callout: ["callout", "text"],
      badge: ["badge"],
      rect: ["rect"],
      ellipse: ["ellipse"],
      arrow: ["arrow"],
      line: ["line"],
      blur: ["blur"],
      spotlight: ["spotlight"],
      magnify: ["magnify"],
    };
    const want = kinds[this.tool];
    if (!want) return null;
    const tol = 6 / this.zoom;
    for (let i = this.doc.shapes.length - 1; i >= 0; i--) {
      const s = this.doc.shapes[i]!;
      if (want.includes(s.type) && hitShape(s, p, tol)) return s;
    }
    return null;
  }

  private dragPicked(p: Point) {
    const m = this.moving!;
    const [dx, dy] = [p.x - m.start.x, p.y - m.start.y];
    if (!m.moved && Math.hypot(dx, dy) < 2 / this.zoom) return;
    if (!m.moved) {
      // handles would lag behind the shape: they come back after the drop
      this.anchors.forEach((a) => a.destroy());
      this.anchors = [];
      m.moved = true;
    }
    m.node.position({ x: m.origin.x + dx, y: m.origin.y + dy });
    this.transformer.forceUpdate();
    this.shapeLayer.batchDraw();
    this.uiLayer.batchDraw();
  }

  private dropPicked() {
    const m = this.moving!;
    this.moving = null;
    if (!m.moved) return;
    const shape = this.doc.shapes.find((s) => s.id === m.id);
    if (!shape) return;
    const moved = moveShape(shape, m.node.x() - m.origin.x, m.node.y() - m.origin.y);
    this.commit(updateShape(this.doc, m.id, moved));
  }

  private addEndpointAnchors(shape: LineShape | ArrowShape) {
    const make = (which: "start" | "end") => {
      const c = new Konva.Circle({
        x: which === "start" ? shape.x1 : shape.x2,
        y: which === "start" ? shape.y1 : shape.y2,
        radius: 6 / this.zoom,
        fill: "#ffffff",
        stroke: "#4c8dff",
        strokeWidth: 2 / this.zoom,
        draggable: true,
      });
      c.on("dragmove", () => {
        const node = this.shapeLayer.findOne<Konva.Line>(`#${shape.id}`);
        if (!node) return;
        const pts = node.points();
        if (which === "start") {
          pts[0] = c.x();
          pts[1] = c.y();
        } else {
          pts[2] = c.x();
          pts[3] = c.y();
        }
        node.points(pts);
        this.shapeLayer.batchDraw();
      });
      c.on("dragend", () => {
        const patch = which === "start" ? { x1: c.x(), y1: c.y() } : { x2: c.x(), y2: c.y() };
        this.commit(updateShape(this.doc, shape.id, patch as Partial<Shape>));
      });
      c.on("mouseenter", () => (this.container.style.cursor = "move"));
      c.on("mouseleave", () => (this.container.style.cursor = "default"));
      this.uiLayer.add(c);
      this.anchors.push(c);
    };
    make("start");
    make("end");
  }

  private bindTransformer() {
    this.transformer.on("transformend", () => {
      const node = this.transformer.nodes()[0];
      const shape = this.selectedShape();
      if (!node || !shape) return;
      const sx = node.scaleX();
      const sy = node.scaleY();
      node.scale({ x: 1, y: 1 });
      let patch: Partial<Shape> = {};
      switch (shape.type) {
        case "rect":
        case "blur":
        case "spotlight":
          patch = { x: node.x(), y: node.y(), width: Math.max(2, node.width() * sx), height: Math.max(2, node.height() * sy) };
          break;
        case "magnify":
          patch = { x: node.x(), y: node.y(), scale: Math.min(8, Math.max(1, shape.scale * sx)) } as Partial<MagnifyShape>;
          break;
        case "callout":
          patch = { x: node.x(), y: node.y(), width: Math.max(80, shape.width * sx) } as Partial<CalloutShape>;
          break;
        case "ellipse": {
          const e = node as Konva.Ellipse;
          const w = Math.max(2, e.radiusX() * 2 * sx);
          const h = Math.max(2, e.radiusY() * 2 * sy);
          patch = { x: e.x() - w / 2, y: e.y() - h / 2, width: w, height: h };
          break;
        }
        case "text": {
          const t = (node as Konva.Label).getText();
          patch = { x: node.x(), y: node.y(), rotation: node.rotation(), width: Math.max(40, t.width() * sx) } as Partial<TextShape>;
          break;
        }
        case "badge":
          // the body was scaled around the centre: only the size changes
          patch = { size: Math.round(Math.min(160, Math.max(12, shape.size * sx))) };
          break;
      }
      this.commit(updateShape(this.doc, shape.id, patch));
    });
  }

  private onNodeDragEnd(node: Konva.Node) {
    const id = node.id();
    const shape = this.doc.shapes.find((s) => s.id === id);
    if (!shape) return;
    let patch: Partial<Shape>;
    switch (shape.type) {
      case "ellipse":
        patch = { x: node.x() - shape.width / 2, y: node.y() - shape.height / 2 };
        break;
      case "rect":
      case "blur":
      case "text":
      case "badge":
      case "spotlight":
      case "magnify":
      case "callout":
        // callout: only the bubble moves; the tail keeps pointing at its target
        patch = { x: node.x(), y: node.y() };
        break;
      case "line":
      case "arrow": {
        const dx = node.x();
        const dy = node.y();
        node.position({ x: 0, y: 0 });
        patch = { x1: shape.x1 + dx, y1: shape.y1 + dy, x2: shape.x2 + dx, y2: shape.y2 + dy };
        break;
      }
      case "pen":
      case "highlighter": {
        const dx = node.x();
        const dy = node.y();
        node.position({ x: 0, y: 0 });
        patch = { points: shape.points.map((v, i) => (i % 2 === 0 ? v + dx : v + dy)) };
        break;
      }
    }
    this.commit(updateShape(this.doc, id, patch));
  }

  // ---------- stage events ----------
  private pointer(): Point {
    const p = this.stage.getRelativePointerPosition();
    return p ? { x: p.x, y: p.y } : { x: 0, y: 0 };
  }

  private bindStage() {
    const st = this.stage;
    st.on("mousedown touchstart", (e) => {
      if (e.evt instanceof MouseEvent && e.evt.button !== 0) return;
      if (this.textarea) return; // let the textarea commit first
      const p = this.pointer();
      // The text tool focuses a textarea inside this handler; without this the browser's
      // default mousedown focus change steals focus right back, the empty box blurs and vanishes.
      if (this.tool === "text") e.evt.preventDefault();
      if (this.tool === "select") {
        const target = e.target;
        const node = target.findAncestor(".shape", true) ?? (target.hasName("shape") ? target : null);
        if (node) {
          this.select(node.id());
          return;
        }
        if (target === st || target.hasName("bg")) this.select(null);
        return;
      }
      // a handle of the selected shape (resize, tail tip, line ends): Konva drags it
      if (e.target.getLayer() === this.uiLayer) return;
      // pressing on an existing shape of the tool's own kind edits it instead of drawing anew
      const picked = this.pickSameKind(p);
      if (picked) {
        if (picked.type === "text" || picked.type === "callout") {
          this.editText(picked, false, true);
          return;
        }
        this.select(picked.id);
        const node = this.shapeLayer.findOne(`#${picked.id}`);
        if (node) this.moving = { id: picked.id, start: p, node, origin: node.position(), moved: false };
        return;
      }
      if (this.selectedId) this.select(null);
      this.beginDraw(p, e.evt as MouseEvent);
    });
    st.on("mousemove touchmove", (e) => {
      const p = this.pointer();
      this.events.onStatus(`${Math.round(p.x)}, ${Math.round(p.y)}`);
      if (this.moving) {
        this.dragPicked(p);
        return;
      }
      if (this.drawing) {
        this.updateDraw(p, e.evt as MouseEvent);
        return;
      }
      if (this.tool !== "select" && !this.textarea) {
        // show that a click here picks up the existing shape
        const over = this.pickSameKind(p);
        this.container.style.cursor = over
          ? over.type === "text" || over.type === "callout"
            ? "text"
            : "move"
          : this.tool === "text"
            ? "text"
            : "crosshair";
      }
    });
    st.on("mouseup touchend", (e) => {
      if (this.moving) {
        this.dropPicked();
        return;
      }
      if (this.drawing) this.endDraw(this.pointer(), e.evt as MouseEvent);
    });
    st.on("dblclick dbltap", (e) => {
      if (this.tool !== "select") return;
      const node = e.target.findAncestor(".shape", true) ?? (e.target.hasName("shape") ? e.target : null);
      const shape = node ? this.doc.shapes.find((s) => s.id === node.id()) : null;
      if (shape?.type === "text" || shape?.type === "callout") this.editText(shape);
    });
    st.on("dragend", (e) => {
      const node = e.target.findAncestor(".shape", true) ?? (e.target.hasName("shape") ? e.target : null);
      if (node && node.getLayer() === this.shapeLayer) this.onNodeDragEnd(node);
    });
    st.on("dragstart", (e) => {
      const node = e.target.findAncestor(".shape", true) ?? (e.target.hasName("shape") ? e.target : null);
      if (node) this.select(node.id());
    });
    this.container.addEventListener(
      "wheel",
      (ev) => {
        ev.preventDefault();
        if (ev.ctrlKey || ev.metaKey) {
          const factor = Math.exp(-ev.deltaY * 0.0015);
          this.setZoom(this.zoom * factor, { x: ev.offsetX, y: ev.offsetY });
        } else {
          this.stage.position({ x: this.stage.x() - ev.deltaX, y: this.stage.y() - ev.deltaY });
          this.clampPan();
          this.stage.batchDraw();
        }
      },
      { passive: false },
    );
  }

  // ---------- drawing ----------
  private beginDraw(p: Point, evt: MouseEvent) {
    const st = this.style;
    const base = { id: newId(), stroke: st.stroke, strokeWidth: st.strokeWidth, opacity: 1, shadow: st.shadow };
    switch (this.tool) {
      case "text": {
        const shape: TextShape = {
          ...base,
          type: "text",
          x: p.x,
          y: p.y,
          text: "",
          fontSize: st.fontSize,
          fontFamily: st.fontFamily,
          background: null,
          rotation: 0,
          width: null,
        };
        this.editText(shape, true);
        return;
      }
      case "badge": {
        // a click places it; a drag points a tail at where the drag started
        this.drawing = { start: p, shape: null, kind: "badge" };
        this.replaceDraft(this.shapeForDrag("badge", p, p, evt)!);
        return;
      }
      case "pen":
      case "highlighter": {
        const shape: Shape =
          this.tool === "pen"
            ? { ...base, type: "pen", points: [p.x, p.y] }
            : { ...base, type: "highlighter", strokeWidth: Math.max(14, st.strokeWidth * 4), points: [p.x, p.y] };
        this.drawing = { start: p, shape, kind: this.tool };
        this.draft = buildNode(shape, this.image);
        if (this.draft) this.uiLayer.add(this.draft as Konva.Shape);
        return;
      }
      default:
        this.drawing = { start: p, shape: null, kind: this.tool };
        void evt;
    }
  }

  private shapeForDrag(kind: ToolId, start: Point, p: Point, evt: MouseEvent): Shape | null {
    const st = this.style;
    const base = { id: this.drawing?.shape?.id ?? newId(), stroke: st.stroke, strokeWidth: st.strokeWidth, opacity: 1, shadow: st.shadow };
    let end = p;
    if (evt?.shiftKey && (kind === "line" || kind === "arrow" || kind === "measure")) {
      // snap to 45°
      const dx = p.x - start.x;
      const dy = p.y - start.y;
      const ang = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
      const len = Math.hypot(dx, dy);
      end = { x: start.x + Math.cos(ang) * len, y: start.y + Math.sin(ang) * len };
    }
    let r = rectFromPoints(start.x, start.y, end.x, end.y);
    if (evt?.shiftKey && (kind === "rect" || kind === "ellipse" || kind === "blur" || kind === "crop")) {
      const side = Math.max(r.width, r.height);
      r = { x: end.x < start.x ? start.x - side : start.x, y: end.y < start.y ? start.y - side : start.y, width: side, height: side };
    }
    if (evt?.altKey && (kind === "rect" || kind === "ellipse" || kind === "blur")) {
      r = { x: start.x - r.width, y: start.y - r.height, width: r.width * 2, height: r.height * 2 };
    }
    switch (kind) {
      case "rect":
        return { ...base, type: "rect", ...r, radius: 3, fill: st.fill ? st.stroke + "33" : null };
      case "ellipse":
        return { ...base, type: "ellipse", ...r, fill: st.fill ? st.stroke + "33" : null };
      case "line":
        return { ...base, type: "line", x1: start.x, y1: start.y, x2: end.x, y2: end.y };
      case "arrow":
        return { ...base, type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, headSize: Math.max(10, st.strokeWidth * 3.5) };
      case "blur":
        return { ...base, type: "blur", ...rectRound(r), mode: st.blurMode, amount: st.blurAmount, shadow: false };
      case "spotlight":
        return { ...base, type: "spotlight", ...rectRound(r), radius: 8, dim: st.dim, shadow: false };
      case "magnify": {
        // inset beside the source: right if it fits, else left, kept inside the image
        const src = rectRound(this.clampRect(r));
        const w = src.width * st.zoom;
        const h = src.height * st.zoom;
        const W = this.doc.imageWidth;
        const H = this.doc.imageHeight;
        let x = src.x + src.width + 24;
        if (x + w > W) x = src.x - 24 - w;
        x = Math.max(0, Math.min(x, W - w));
        const y = Math.max(0, Math.min(src.y + src.height / 2 - h / 2, H - h));
        return { ...base, type: "magnify", src, x, y, scale: st.zoom, radius: 8, strokeWidth: 3 };
      }
      case "badge": {
        // drag from the thing you're pointing at to where the number goes
        const far = Math.hypot(p.x - start.x, p.y - start.y) >= Math.max(12, st.badgeSize * 0.75);
        return {
          ...base,
          type: "badge",
          x: far ? p.x : start.x,
          y: far ? p.y : start.y,
          n: nextBadgeNumber(this.doc),
          size: st.badgeSize,
          kind: st.badgeKind,
          tip: far ? { x: start.x, y: start.y } : null,
          textColor: "#ffffff",
        };
      }
      case "callout": {
        // drag from the thing you're pointing at to where the bubble goes
        const width = Math.max(160, st.fontSize * 8);
        const far = Math.hypot(p.x - start.x, p.y - start.y) >= 12;
        const bx = far ? p.x - width / 2 : start.x + 30;
        const by = far ? p.y - st.fontSize : start.y - st.fontSize * 3.5;
        return {
          ...base,
          type: "callout",
          tipX: start.x,
          tipY: start.y,
          x: Math.max(0, bx),
          y: Math.max(0, by),
          width,
          text: "",
          fontSize: st.fontSize,
          fontFamily: st.fontFamily,
        };
      }
      default:
        return null;
    }
  }

  private updateDraw(p: Point, evt: MouseEvent) {
    if (!this.drawing) return;
    const d = this.drawing;
    if (d.kind === "pen" || d.kind === "highlighter") {
      const s = d.shape as Extract<Shape, { points: number[] }>;
      s.points.push(p.x, p.y);
      (this.draft as Konva.Line).points(s.points);
      this.uiLayer.batchDraw();
      return;
    }
    if (d.kind === "crop") {
      const r = rectRound(this.clampRect(rectFromPoints(d.start.x, d.start.y, p.x, p.y)));
      this.doc = { ...this.doc, crop: r.width > 2 && r.height > 2 ? r : null };
      this.updateCropMask();
      this.events.onStatus(`crop ${r.width} × ${r.height}`);
      return;
    }
    if (d.kind === "measure") {
      const shape = this.shapeForDrag("line", d.start, p, evt) as LineShape;
      shape.stroke = "#4c8dff";
      shape.strokeWidth = 1 / this.zoom;
      shape.shadow = false;
      this.replaceDraft(shape);
      const len = Math.hypot(shape.x2 - shape.x1, shape.y2 - shape.y1);
      const text = `${Math.round(Math.abs(shape.x2 - shape.x1))} × ${Math.round(Math.abs(shape.y2 - shape.y1))}  ·  ${Math.round(len)} px`;
      this.showDraftLabel(text, p);
      this.events.onStatus(text);
      return;
    }
    const shape = this.shapeForDrag(d.kind, d.start, p, evt);
    if (!shape) return;
    d.shape = shape;
    this.replaceDraft(shape);
    if (isBox(shape)) this.events.onStatus(`${Math.round(shape.width)} × ${Math.round(shape.height)}`);
  }

  private endDraw(p: Point, evt: MouseEvent) {
    const d = this.drawing;
    if (!d) return;
    this.drawing = null;
    const moved = Math.hypot(p.x - d.start.x, p.y - d.start.y) >= 3;
    if (d.kind === "crop") {
      this.cancelDraft();
      if (!moved) this.doc = { ...this.doc, crop: null };
      this.commit(this.doc);
      return;
    }
    if (d.kind === "measure") {
      this.cancelDraft();
      return;
    }
    if (d.kind === "pen" || d.kind === "highlighter") {
      const s = d.shape as Extract<Shape, { points: number[] }>;
      this.cancelDraft();
      if (s.points.length < 4) return;
      this.commit(addShape(this.doc, s));
      return;
    }
    const shape = this.shapeForDrag(d.kind, d.start, p, evt);
    this.cancelDraft();
    if (shape?.type === "callout") {
      // a click places a bubble too; either way, type its text next
      this.editText(shape, true);
      return;
    }
    if (shape?.type === "badge") {
      this.commit(addShape(this.doc, shape)); // a click places one too
      return;
    }
    if (!shape || !moved) return;
    if (shape.type === "magnify" && (shape.src.width < 4 || shape.src.height < 4)) return;
    if (isBox(shape) && (shape.width < 2 || shape.height < 2)) return;
    this.commit(addShape(this.doc, shape));
  }

  private replaceDraft(shape: Shape) {
    this.draft?.destroy();
    this.draft = buildNode(shape, this.image);
    if (this.draft) {
      this.draft.listening(false);
      this.uiLayer.add(this.draft as Konva.Shape);
    }
    this.uiLayer.batchDraw();
  }

  private showDraftLabel(text: string, p: Point) {
    if (!this.draftLabel) {
      this.draftLabel = new Konva.Label({ listening: false });
      this.draftLabel.add(new Konva.Tag({ fill: "rgba(20,22,26,0.9)", cornerRadius: 3 }));
      this.draftLabel.add(new Konva.Text({ text, fill: "#fff", fontSize: 12 / this.zoom, padding: 4 / this.zoom }));
      this.uiLayer.add(this.draftLabel);
    }
    this.draftLabel.getText().text(text).fontSize(12 / this.zoom).padding(4 / this.zoom);
    this.draftLabel.position({ x: p.x + 12 / this.zoom, y: p.y + 12 / this.zoom });
    this.uiLayer.batchDraw();
  }

  cancelDraft() {
    this.drawing = null;
    this.draft?.destroy();
    this.draft = null;
    this.draftLabel?.destroy();
    this.draftLabel = null;
    this.uiLayer.batchDraw();
  }

  private clampRect(r: Rect): Rect {
    const bounds = { x: 0, y: 0, width: this.doc.imageWidth, height: this.doc.imageHeight };
    return rectIntersect(r, bounds) ?? { x: 0, y: 0, width: 0, height: 0 };
  }

  // ---------- crop mask ----------
  private updateCropMask() {
    this.cropMask.destroyChildren();
    const crop = this.doc.crop;
    if (!crop) {
      this.cropMask.visible(false);
      this.uiLayer.batchDraw();
      return;
    }
    const W = this.doc.imageWidth;
    const H = this.doc.imageHeight;
    const dim = "rgba(0,0,0,0.55)";
    const parts = [
      { x: 0, y: 0, width: W, height: crop.y },
      { x: 0, y: crop.y + crop.height, width: W, height: H - crop.y - crop.height },
      { x: 0, y: crop.y, width: crop.x, height: crop.height },
      { x: crop.x + crop.width, y: crop.y, width: W - crop.x - crop.width, height: crop.height },
    ];
    for (const p of parts) if (p.width > 0 && p.height > 0) this.cropMask.add(new Konva.Rect({ ...p, fill: dim }));
    this.cropMask.add(
      new Konva.Rect({ ...crop, stroke: "#ffffff", strokeWidth: 1 / this.zoom, dash: [6 / this.zoom, 4 / this.zoom] }),
    );
    this.cropMask.visible(true);
    this.cropMask.moveToBottom();
    this.uiLayer.batchDraw();
  }

  clearCrop() {
    this.commit({ ...this.doc, crop: null });
  }

  // ---------- text editing ----------
  /** `caretAtEnd`: continue typing after the existing text (a click with the text tool)
   * rather than selecting it all (double-click to edit). */
  editText(shape: TextShape | CalloutShape, isNew = false, caretAtEnd = false) {
    this.closeTextarea(false);
    const node = isNew ? null : this.shapeLayer.findOne<Konva.Label>(`#${shape.id}`);
    if (node) node.visible(false);
    this.shapeLayer.batchDraw();

    const ta = document.createElement("textarea");
    this.textarea = ta;
    const abs = { x: this.stage.x() + shape.x * this.zoom, y: this.stage.y() + shape.y * this.zoom };
    const callout = shape.type === "callout";
    Object.assign(ta.style, {
      position: "absolute",
      left: `${abs.x}px`,
      top: `${abs.y}px`,
      minWidth: "60px",
      width: shape.width ? `${shape.width * this.zoom}px` : "auto",
      padding: `${(callout ? CALLOUT_PAD : 6) * this.zoom}px`,
      margin: "0",
      border: "1px dashed #4c8dff",
      borderRadius: callout ? `${10 * this.zoom}px` : "4px",
      background: callout ? shape.stroke : (shape.background ?? "rgba(0,0,0,0.25)"),
      color: callout ? textColorFor(shape.stroke) : shape.stroke,
      font: `bold ${shape.fontSize * this.zoom}px ${shape.fontFamily}`,
      lineHeight: "1.25",
      outline: "none",
      resize: "none",
      overflow: "hidden",
      // callouts wrap inside their bubble; text boxes grow with their content
      whiteSpace: callout ? "pre-wrap" : "pre",
      boxSizing: "border-box",
      transformOrigin: "left top",
      transform: callout ? "none" : `rotate(${shape.rotation}deg)`,
      zIndex: "10",
    } as CSSStyleDeclaration);
    if (callout) ta.placeholder = "Type your note…";
    ta.value = shape.text;
    ta.rows = 1;
    const autosize = () => {
      ta.style.height = "auto";
      ta.style.height = `${ta.scrollHeight}px`;
      if (!shape.width) {
        ta.style.width = "auto";
        ta.style.width = `${Math.max(60, ta.scrollWidth + 4)}px`;
      }
    };
    ta.addEventListener("input", autosize);
    ta.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        this.closeTextarea(false, shape, isNew);
      } else if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        this.closeTextarea(true, shape, isNew);
      }
    });
    ta.addEventListener("blur", () => this.closeTextarea(true, shape, isNew));
    this.container.appendChild(ta);
    autosize();
    ta.focus();
    if (caretAtEnd) ta.setSelectionRange(ta.value.length, ta.value.length);
    else ta.select();
    // belt and braces: if anything else grabbed focus during this click, take it back
    requestAnimationFrame(() => {
      if (this.textarea === ta && document.activeElement !== ta) ta.focus();
    });
  }

  private closeTextarea(commit: boolean, shape?: TextShape | CalloutShape, isNew = false) {
    const ta = this.textarea;
    if (!ta) return;
    this.textarea = null;
    const value = ta.value.replace(/\s+$/, "");
    ta.remove();
    if (!shape) return;
    const node = this.shapeLayer.findOne<Konva.Label>(`#${shape.id}`);
    if (node) node.visible(true);
    if (commit && value.trim()) {
      const next = isNew ? addShape(this.doc, { ...shape, text: value }) : updateShape(this.doc, shape.id, { text: value });
      this.commit(next);
      if (isNew) this.events.onSelect(null);
    } else if (!isNew && !value.trim()) {
      this.commit(removeShape(this.doc, shape.id));
    } else {
      this.shapeLayer.batchDraw();
    }
  }

  get isEditingText() {
    return this.textarea !== null;
  }

  // ---------- keyboard actions ----------
  deleteSelected() {
    if (!this.selectedId) return;
    const id = this.selectedId;
    this.select(null);
    this.commit(removeShape(this.doc, id));
  }

  nudgeSelected(dx: number, dy: number) {
    const s = this.selectedShape();
    if (!s) return;
    this.commit(updateShape(this.doc, s.id, moveShape(s, dx, dy) as Partial<Shape>));
  }

  selectNext(dir: 1 | -1) {
    const ids = this.doc.shapes.map((s) => s.id);
    if (ids.length === 0) return;
    const i = this.selectedId ? ids.indexOf(this.selectedId) : -1;
    const next = ids[(i + dir + ids.length) % ids.length]!;
    if (this.tool !== "select") this.setTool("select");
    this.select(next);
  }

  duplicateSelected() {
    const s = this.selectedShape();
    if (!s) return;
    const copy = { ...moveShape(s, 16, 16), id: newId() } as Shape;
    this.commit(addShape(this.doc, copy));
    this.select(copy.id);
  }

  // ---------- zoom / pan ----------
  private resize() {
    this.stage.size({ width: this.container.clientWidth, height: this.container.clientHeight });
    this.clampPan();
    this.stage.batchDraw();
  }

  fit() {
    const cw = this.container.clientWidth - 24;
    const ch = this.container.clientHeight - 24;
    const z = Math.min(1, cw / this.doc.imageWidth, ch / this.doc.imageHeight);
    this.setZoom(z > 0 ? z : 1);
  }

  setZoom(z: number, around?: Point) {
    z = Math.min(8, Math.max(0.1, z));
    const old = this.zoom;
    const center = around ?? { x: this.stage.width() / 2, y: this.stage.height() / 2 };
    const imgPt = { x: (center.x - this.stage.x()) / old, y: (center.y - this.stage.y()) / old };
    this.zoom = z;
    this.stage.scale({ x: z, y: z });
    this.stage.position({ x: center.x - imgPt.x * z, y: center.y - imgPt.y * z });
    this.clampPan();
    this.refreshSelection();
    this.updateCropMask();
    this.stage.batchDraw();
    this.events.onZoom(z);
  }

  private clampPan() {
    const w = this.doc.imageWidth * this.zoom;
    const h = this.doc.imageHeight * this.zoom;
    const sw = this.stage.width();
    const sh = this.stage.height();
    let x = this.stage.x();
    let y = this.stage.y();
    if (w <= sw) x = (sw - w) / 2;
    else x = Math.min(0, Math.max(sw - w, x));
    if (h <= sh) y = (sh - h) / 2;
    else y = Math.min(0, Math.max(sh - h, y));
    this.stage.position({ x: Math.round(x), y: Math.round(y) });
  }

  // ---------- export ----------
  private beautifyOptions: Beautify | null = null;

  setBeautify(options: Beautify | null) {
    this.beautifyOptions = options;
  }

  /** Final image. `raw` skips the beautify backdrop (for OCR and code scanning). */
  render(raw = false): HTMLCanvasElement {
    const canvas = renderDocument(this.doc, this.image);
    return raw || !this.beautifyOptions?.enabled ? canvas : beautify(canvas, this.beautifyOptions);
  }

  destroy() {
    this.closeTextarea(false);
    this.stage.destroy();
  }
}
