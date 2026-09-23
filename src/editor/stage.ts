// Konva stage: renders a Document, handles selection, drawing tools, zoom and export.
import Konva from "konva";
import type { Beautify, Rect } from "$lib/types";
import { beautify } from "./beautify";
import { rectFromPoints, rectIntersect, rectRound } from "$lib/geometry";
import {
  addShape,
  moveShape,
  newId,
  nextBadgeNumber,
  removeShape,
  updateShape,
  withShapes,
  type ArrowShape,
  type BlurShape,
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
  fill: boolean;
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

type BoxShape = Extract<Shape, { type: "rect" | "ellipse" | "blur" }>;
const isBox = (s: Shape): s is BoxShape => s.type === "rect" || s.type === "ellipse" || s.type === "blur";

const SHADOW = { shadowColor: "rgba(0,0,0,0.55)", shadowBlur: 6, shadowOffset: { x: 2, y: 2 }, shadowOpacity: 1 };

function shadowProps(on: boolean) {
  return on ? SHADOW : {};
}

/** Build Konva nodes for a document. Shared by the live stage and the export stage. */
export function buildShapeNodes(layer: Konva.Layer, doc: Document, image: HTMLCanvasElement) {
  layer.destroyChildren();
  // The screenshot lives in the same layer so blend modes (highlighter multiply) see it.
  layer.add(new Konva.Image({ image, x: 0, y: 0, listening: false, name: "bg" }));
  for (const s of doc.shapes) {
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
    case "badge": {
      const g = new Konva.Group({ ...common, x: s.x, y: s.y });
      g.add(
        new Konva.Circle({
          radius: s.size / 2,
          fill: s.stroke,
          stroke: "#ffffff",
          strokeWidth: Math.max(1.5, s.size / 14),
          ...shadowProps(s.shadow),
        }),
      );
      g.add(
        new Konva.Text({
          text: String(s.n),
          fontSize: s.size * 0.58,
          fontFamily: "Segoe UI, -apple-system, Helvetica, Arial, sans-serif",
          fontStyle: "bold",
          fill: s.textColor,
          width: s.size,
          height: s.size,
          offsetX: s.size / 2,
          offsetY: s.size / 2,
          align: "center",
          verticalAlign: "middle",
        }),
      );
      return g;
    }
  }
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
    if (style.fontSize !== prev.fontSize && s.type === "text") patch.fontSize = style.fontSize;
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
      const resizable = shape.type === "rect" || shape.type === "ellipse" || shape.type === "blur";
      this.transformer.enabledAnchors(
        resizable
          ? ["top-left", "top-right", "bottom-left", "bottom-right", "middle-left", "middle-right", "top-center", "bottom-center"]
          : shape.type === "text"
            ? ["middle-left", "middle-right"]
            : shape.type === "badge"
              ? ["top-left", "top-right", "bottom-left", "bottom-right"]
              : [],
      );
      this.transformer.keepRatio(shape.type === "badge");
      this.transformer.rotateEnabled(shape.type === "text");
      this.transformer.nodes([node]);
      this.transformer.moveToTop();
    }
    this.uiLayer.batchDraw();
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
          patch = { x: node.x(), y: node.y(), width: Math.max(2, node.width() * sx), height: Math.max(2, node.height() * sy) };
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
          patch = { x: node.x(), y: node.y(), size: Math.max(12, shape.size * sx) };
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
      this.beginDraw(p, e.evt as MouseEvent);
    });
    st.on("mousemove touchmove", (e) => {
      const p = this.pointer();
      this.events.onStatus(`${Math.round(p.x)}, ${Math.round(p.y)}`);
      if (this.drawing) this.updateDraw(p, e.evt as MouseEvent);
    });
    st.on("mouseup touchend", (e) => {
      if (this.drawing) this.endDraw(this.pointer(), e.evt as MouseEvent);
    });
    st.on("dblclick dbltap", (e) => {
      if (this.tool !== "select") return;
      const node = e.target.findAncestor(".shape", true) ?? (e.target.hasName("shape") ? e.target : null);
      const shape = node ? this.doc.shapes.find((s) => s.id === node.id()) : null;
      if (shape?.type === "text") this.editText(shape);
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
        const shape: Shape = { ...base, type: "badge", x: p.x, y: p.y, n: nextBadgeNumber(this.doc), size: st.badgeSize, textColor: "#ffffff" };
        this.commit(addShape(this.doc, shape));
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
    if (!shape || !moved) return;
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
  editText(shape: TextShape, isNew = false) {
    this.closeTextarea(false);
    const node = isNew ? null : this.shapeLayer.findOne<Konva.Label>(`#${shape.id}`);
    if (node) node.visible(false);
    this.shapeLayer.batchDraw();

    const ta = document.createElement("textarea");
    this.textarea = ta;
    const abs = { x: this.stage.x() + shape.x * this.zoom, y: this.stage.y() + shape.y * this.zoom };
    Object.assign(ta.style, {
      position: "absolute",
      left: `${abs.x}px`,
      top: `${abs.y}px`,
      minWidth: "60px",
      width: shape.width ? `${shape.width * this.zoom}px` : "auto",
      padding: `${6 * this.zoom}px`,
      margin: "0",
      border: "1px dashed #4c8dff",
      borderRadius: "4px",
      background: shape.background ?? "rgba(0,0,0,0.25)",
      color: shape.stroke,
      font: `bold ${shape.fontSize * this.zoom}px ${shape.fontFamily}`,
      lineHeight: "1.25",
      outline: "none",
      resize: "none",
      overflow: "hidden",
      whiteSpace: "pre",
      transformOrigin: "left top",
      transform: `rotate(${shape.rotation}deg)`,
      zIndex: "10",
    } as CSSStyleDeclaration);
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
    ta.select();
    // belt and braces: if anything else grabbed focus during this click, take it back
    requestAnimationFrame(() => {
      if (this.textarea === ta && document.activeElement !== ta) ta.focus();
    });
  }

  private closeTextarea(commit: boolean, shape?: TextShape, isNew = false) {
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
