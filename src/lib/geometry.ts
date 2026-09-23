import type { Rect } from "./types";

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Normalise a drag from (x0,y0) to (x1,y1) into a rect with positive size. */
export function rectFromPoints(x0: number, y0: number, x1: number, y1: number): Rect {
  const x = Math.min(x0, x1);
  const y = Math.min(y0, y1);
  return { x, y, width: Math.abs(x1 - x0), height: Math.abs(y1 - y0) };
}

export function rectContains(r: Rect, x: number, y: number): boolean {
  return x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height;
}

export function rectIntersect(a: Rect, b: Rect): Rect | null {
  const x = Math.max(a.x, b.x);
  const y = Math.max(a.y, b.y);
  const r = Math.min(a.x + a.width, b.x + b.width);
  const btm = Math.min(a.y + a.height, b.y + b.height);
  return r > x && btm > y ? { x, y, width: r - x, height: btm - y } : null;
}

export function rectRound(r: Rect): Rect {
  const x = Math.round(r.x);
  const y = Math.round(r.y);
  return { x, y, width: Math.round(r.x + r.width) - x, height: Math.round(r.y + r.height) - y };
}

export function rectClampTo(r: Rect, bounds: Rect): Rect {
  const x = clamp(r.x, bounds.x, bounds.x + bounds.width - r.width);
  const y = clamp(r.y, bounds.y, bounds.y + bounds.height - r.height);
  return { x, y, width: Math.min(r.width, bounds.width), height: Math.min(r.height, bounds.height) };
}

export function rectEquals(a: Rect | null, b: Rect | null): boolean {
  if (!a || !b) return a === b;
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
}

/** A straight edge: `pos` on one axis, spanning `from`..`to` on the other. */
export interface Edge {
  pos: number;
  from: number;
  to: number;
}

export interface Snapped {
  x: number;
  y: number;
  /** the edge each axis snapped to, if any */
  snapX: number | null;
  snapY: number | null;
}

/**
 * Pull a point onto the nearest edge within `threshold`, per axis. Only edges that actually
 * pass near the point count (a window's left edge doesn't attract a point far above it).
 * `vertical` edges have an x position, `horizontal` ones a y position.
 */
export function snapPoint(x: number, y: number, vertical: Edge[], horizontal: Edge[], threshold: number): Snapped {
  const nearest = (v: number, across: number, edges: Edge[]) => {
    let best: number | null = null;
    let bestDist = threshold;
    for (const e of edges) {
      if (across < e.from - threshold || across > e.to + threshold) continue;
      const d = Math.abs(e.pos - v);
      if (d <= bestDist) {
        bestDist = d;
        best = e.pos;
      }
    }
    return best;
  };
  const snapX = nearest(x, y, vertical);
  const snapY = nearest(y, x, horizontal);
  return { x: snapX ?? x, y: snapY ?? y, snapX, snapY };
}

/** Edges of a rect, for snapping. */
export function rectEdges(r: Rect): { vertical: Edge[]; horizontal: Edge[] } {
  const right = r.x + r.width;
  const bottom = r.y + r.height;
  return {
    vertical: [
      { pos: r.x, from: r.y, to: bottom },
      { pos: right, from: r.y, to: bottom },
    ],
    horizontal: [
      { pos: r.y, from: r.x, to: right },
      { pos: bottom, from: r.x, to: right },
    ],
  };
}
