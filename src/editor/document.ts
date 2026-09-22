// Annotation document: plain data, image-pixel coordinates. Konva nodes are derived from it.
import type { Rect } from "$lib/types";

export type ToolId =
  | "select"
  | "arrow"
  | "line"
  | "rect"
  | "ellipse"
  | "pen"
  | "text"
  | "highlighter"
  | "blur"
  | "badge"
  | "crop"
  | "measure";

export interface BaseShape {
  id: string;
  type: string;
  stroke: string;
  strokeWidth: number;
  opacity: number;
  shadow: boolean;
}

export interface RectShape extends BaseShape {
  type: "rect";
  x: number;
  y: number;
  width: number;
  height: number;
  radius: number;
  fill: string | null;
}

export interface EllipseShape extends BaseShape {
  type: "ellipse";
  x: number;
  y: number;
  width: number;
  height: number;
  fill: string | null;
}

export interface LineShape extends BaseShape {
  type: "line";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

export interface ArrowShape extends BaseShape {
  type: "arrow";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  headSize: number;
}

export interface PenShape extends BaseShape {
  type: "pen";
  points: number[];
}

export interface HighlighterShape extends BaseShape {
  type: "highlighter";
  points: number[];
}

export interface TextShape extends BaseShape {
  type: "text";
  x: number;
  y: number;
  text: string;
  fontSize: number;
  fontFamily: string;
  background: string | null;
  rotation: number;
  width: number | null;
}

export interface BlurShape extends BaseShape {
  type: "blur";
  x: number;
  y: number;
  width: number;
  height: number;
  mode: "pixelate" | "blur";
  amount: number;
}

export interface BadgeShape extends BaseShape {
  type: "badge";
  x: number;
  y: number;
  n: number;
  size: number;
  textColor: string;
}

export type Shape =
  | RectShape
  | EllipseShape
  | LineShape
  | ArrowShape
  | PenShape
  | HighlighterShape
  | TextShape
  | BlurShape
  | BadgeShape;

export interface Document {
  version: 1;
  imageWidth: number;
  imageHeight: number;
  shapes: Shape[];
  crop: Rect | null;
}

let counter = 0;
export const newId = () => `s${Date.now().toString(36)}${(counter++).toString(36)}`;

export function emptyDocument(imageWidth: number, imageHeight: number): Document {
  return { version: 1, imageWidth, imageHeight, shapes: [], crop: null };
}

export function withShapes(doc: Document, shapes: Shape[]): Document {
  return { ...doc, shapes: renumberBadges(shapes) };
}

export function addShape(doc: Document, shape: Shape): Document {
  return withShapes(doc, [...doc.shapes, shape]);
}

export function updateShape(doc: Document, id: string, patch: Partial<Shape>): Document {
  return withShapes(
    doc,
    doc.shapes.map((s) => (s.id === id ? ({ ...s, ...patch } as Shape) : s)),
  );
}

export function removeShape(doc: Document, id: string): Document {
  return withShapes(
    doc,
    doc.shapes.filter((s) => s.id !== id),
  );
}

export function bringToFront(doc: Document, id: string): Document {
  const s = doc.shapes.find((x) => x.id === id);
  if (!s) return doc;
  return withShapes(doc, [...doc.shapes.filter((x) => x.id !== id), s]);
}

export function sendToBack(doc: Document, id: string): Document {
  const s = doc.shapes.find((x) => x.id === id);
  if (!s) return doc;
  return withShapes(doc, [s, ...doc.shapes.filter((x) => x.id !== id)]);
}

/** Badges are numbered by their order in the document so deleting one renumbers the rest. */
export function renumberBadges(shapes: Shape[]): Shape[] {
  let n = 1;
  return shapes.map((s) => (s.type === "badge" ? (s.n === n ? (n++, s) : { ...s, n: n++ }) : s));
}

export function nextBadgeNumber(doc: Document): number {
  return doc.shapes.filter((s) => s.type === "badge").length + 1;
}

/** Axis-aligned bounds of a shape, used for hit tests and keyboard nudges. */
export function shapeBounds(s: Shape): Rect {
  switch (s.type) {
    case "rect":
    case "ellipse":
    case "blur":
      return { x: s.x, y: s.y, width: s.width, height: s.height };
    case "line":
    case "arrow":
      return {
        x: Math.min(s.x1, s.x2),
        y: Math.min(s.y1, s.y2),
        width: Math.abs(s.x2 - s.x1),
        height: Math.abs(s.y2 - s.y1),
      };
    case "pen":
    case "highlighter": {
      let x0 = Infinity,
        y0 = Infinity,
        x1 = -Infinity,
        y1 = -Infinity;
      for (let i = 0; i < s.points.length; i += 2) {
        x0 = Math.min(x0, s.points[i]!);
        x1 = Math.max(x1, s.points[i]!);
        y0 = Math.min(y0, s.points[i + 1]!);
        y1 = Math.max(y1, s.points[i + 1]!);
      }
      return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
    }
    case "text":
      return { x: s.x, y: s.y, width: s.width ?? s.fontSize * 8, height: s.fontSize * 1.4 };
    case "badge":
      return { x: s.x - s.size / 2, y: s.y - s.size / 2, width: s.size, height: s.size };
  }
}

export function moveShape(s: Shape, dx: number, dy: number): Shape {
  switch (s.type) {
    case "rect":
    case "ellipse":
    case "blur":
    case "text":
    case "badge":
      return { ...s, x: s.x + dx, y: s.y + dy };
    case "line":
    case "arrow":
      return { ...s, x1: s.x1 + dx, y1: s.y1 + dy, x2: s.x2 + dx, y2: s.y2 + dy };
    case "pen":
    case "highlighter":
      return { ...s, points: s.points.map((v, i) => (i % 2 === 0 ? v + dx : v + dy)) };
  }
}

export function serialize(doc: Document): string {
  return JSON.stringify(doc);
}

export function deserialize(json: string): Document {
  const d = JSON.parse(json) as Document;
  if (d.version !== 1 || !Array.isArray(d.shapes)) throw new Error("unsupported document");
  return d;
}
