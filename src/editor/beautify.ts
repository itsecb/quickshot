// Backdrop for polished exports: padding, background, rounded corners and a soft shadow.
// Applied to the final rendered canvas, so copy/save/drag/pin/guide all get the same result.
import type { Beautify } from "$lib/types";

export interface BackgroundPreset {
  id: string;
  label: string;
  /** two stops = diagonal gradient, one = solid, none = transparent */
  stops: string[];
}

export const BACKGROUNDS: BackgroundPreset[] = [
  { id: "ocean", label: "Ocean", stops: ["#4c8dff", "#7b5cff"] },
  { id: "sunset", label: "Sunset", stops: ["#ff7e5f", "#feb47b"] },
  { id: "mint", label: "Mint", stops: ["#43e97b", "#38f9d7"] },
  { id: "slate", label: "Slate", stops: ["#2c3e50", "#4b6584"] },
  { id: "light", label: "Light", stops: ["#eef0f4"] },
  { id: "dark", label: "Dark", stops: ["#1b1d21"] },
  { id: "none", label: "Transparent", stops: [] },
];

function stopsFor(background: string): string[] {
  const preset = BACKGROUNDS.find((b) => b.id === background);
  if (preset) return preset.stops;
  return /^#[0-9a-f]{3,8}$/i.test(background) ? [background] : BACKGROUNDS[0]!.stops;
}

/** CSS for swatches in the UI. */
export function backgroundCss(background: string): string {
  const stops = stopsFor(background);
  if (stops.length === 0) return "repeating-conic-gradient(#999 0% 25%, #ddd 0% 50%) 50% / 10px 10px";
  if (stops.length === 1) return stops[0]!;
  return `linear-gradient(135deg, ${stops[0]}, ${stops[1]})`;
}

export function beautify(src: HTMLCanvasElement, o: Beautify): HTMLCanvasElement {
  if (!o.enabled) return src;
  const pad = Math.max(0, Math.round(o.padding));
  const radius = Math.max(0, Math.min(o.radius, Math.min(src.width, src.height) / 2));
  const out = document.createElement("canvas");
  out.width = src.width + pad * 2;
  out.height = src.height + pad * 2;
  const ctx = out.getContext("2d")!;

  const stops = stopsFor(o.background);
  if (stops.length === 1) {
    ctx.fillStyle = stops[0]!;
    ctx.fillRect(0, 0, out.width, out.height);
  } else if (stops.length === 2) {
    const g = ctx.createLinearGradient(0, 0, out.width, out.height);
    g.addColorStop(0, stops[0]!);
    g.addColorStop(1, stops[1]!);
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, out.width, out.height);
  }

  const frame = () => {
    ctx.beginPath();
    ctx.roundRect(pad, pad, src.width, src.height, radius);
  };

  // Shadow only makes sense when there is room around the image to show it.
  if (o.shadow && pad > 0) {
    ctx.save();
    ctx.shadowColor = "rgba(0, 0, 0, 0.35)";
    ctx.shadowBlur = Math.max(8, pad * 0.6);
    ctx.shadowOffsetY = Math.max(2, pad * 0.15);
    frame();
    ctx.fillStyle = "#000";
    ctx.fill();
    ctx.restore();
  }

  ctx.save();
  frame();
  ctx.clip();
  ctx.drawImage(src, pad, pad);
  ctx.restore();
  return out;
}
