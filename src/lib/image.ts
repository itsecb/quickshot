// Fetch raw RGBA frames served by the shot:// protocol and turn them into canvases.

export interface RawFrame {
  width: number;
  height: number;
  data: Uint8ClampedArray<ArrayBuffer>;
}

export async function fetchRaw(url: string): Promise<RawFrame> {
  const res = await fetch(url, { cache: "no-store" });
  if (!res.ok) throw new Error(`frame fetch failed: ${res.status} ${await res.text()}`);
  const width = Number(res.headers.get("x-width"));
  const height = Number(res.headers.get("x-height"));
  const buf = await res.arrayBuffer();
  if (!width || !height || buf.byteLength !== width * height * 4) {
    throw new Error(`frame size mismatch (${width}x${height}, ${buf.byteLength} bytes)`);
  }
  return { width, height, data: new Uint8ClampedArray(buf) };
}

export function frameToCanvas(frame: RawFrame): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const ctx = canvas.getContext("2d", { alpha: false })!;
  ctx.putImageData(new ImageData(frame.data, frame.width, frame.height), 0, 0);
  return canvas;
}

export async function fetchRawToCanvas(url: string): Promise<HTMLCanvasElement> {
  return frameToCanvas(await fetchRaw(url));
}

export function canvasToPng(canvas: HTMLCanvasElement): Promise<Uint8Array<ArrayBuffer>> {
  return new Promise((resolve, reject) => {
    canvas.toBlob(async (blob) => {
      if (!blob) return reject(new Error("PNG encode failed"));
      resolve(new Uint8Array(await blob.arrayBuffer()));
    }, "image/png");
  });
}

export function canvasToDataUrl(canvas: HTMLCanvasElement, maxSide = 256): string {
  const scale = Math.min(1, maxSide / Math.max(canvas.width, canvas.height));
  const c = document.createElement("canvas");
  c.width = Math.max(1, Math.round(canvas.width * scale));
  c.height = Math.max(1, Math.round(canvas.height * scale));
  c.getContext("2d")!.drawImage(canvas, 0, 0, c.width, c.height);
  return c.toDataURL("image/png");
}

export function pngToBase64(png: Uint8Array): string {
  let s = "";
  const chunk = 0x8000;
  for (let i = 0; i < png.length; i += chunk) {
    s += String.fromCharCode.apply(null, Array.from(png.subarray(i, i + chunk)));
  }
  return btoa(s);
}

export function rgbToHex(r: number, g: number, b: number): string {
  return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}
