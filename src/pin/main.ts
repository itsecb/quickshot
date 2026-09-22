// Pinned image: always-on-top floating window. Drag anywhere to move, Ctrl+wheel to zoom,
// Esc or double-click to close, Ctrl+C copies, Ctrl+S saves.
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { copyCapture, currentLabel, pinInit, saveCapture } from "$lib/ipc";
import { primaryMod } from "$lib/keys";
import "./pin.css";

const win = getCurrentWindow();

async function main() {
  const info = await pinInit(currentLabel());
  const img = document.createElement("img");
  img.src = info.pngUrl;
  img.draggable = false;
  img.setAttribute("data-tauri-drag-region", "");
  const root = document.getElementById("app")!;
  root.appendChild(img);
  root.setAttribute("data-tauri-drag-region", "");

  let scale = 1;
  const aspect = info.width / info.height;
  const dpr = window.devicePixelRatio || 1;
  const baseW = info.width / dpr;

  async function applyScale() {
    const w = Math.max(48, baseW * scale);
    await win.setSize(new LogicalSize(w, w / aspect));
  }

  root.addEventListener(
    "wheel",
    (e) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      scale = Math.min(6, Math.max(0.1, scale * Math.exp(-e.deltaY * 0.0015)));
      void applyScale();
    },
    { passive: false },
  );
  root.addEventListener("dblclick", () => void win.close());
  root.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    showMenu(e.clientX, e.clientY);
  });
  window.addEventListener("keydown", async (e) => {
    if (e.key === "Escape") return void win.close();
    if (primaryMod(e) && e.key.toLowerCase() === "c") {
      e.preventDefault();
      await copyCapture(info.id);
      flash("Copied");
    }
    if (primaryMod(e) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      const p = await saveCapture(info.id);
      flash(`Saved ${p}`);
    }
    if (primaryMod(e) && e.key === "0") {
      scale = 1;
      await applyScale();
    }
  });

  function flash(text: string) {
    const el = document.createElement("div");
    el.className = "flash";
    el.textContent = text;
    root.appendChild(el);
    setTimeout(() => el.remove(), 1500);
  }

  function showMenu(x: number, y: number) {
    document.querySelector(".menu")?.remove();
    const menu = document.createElement("div");
    menu.className = "menu";
    menu.style.left = `${Math.min(x, window.innerWidth - 150)}px`;
    menu.style.top = `${Math.min(y, window.innerHeight - 120)}px`;
    const items: [string, () => void | Promise<void>][] = [
      ["Copy image", async () => (await copyCapture(info.id), flash("Copied"))],
      ["Save to folder", async () => flash(`Saved ${await saveCapture(info.id)}`)],
      ["Reset size", () => ((scale = 1), applyScale())],
      ["Close pin", () => win.close()],
    ];
    for (const [label, fn] of items) {
      const b = document.createElement("button");
      b.textContent = label;
      b.onclick = () => {
        menu.remove();
        void fn();
      };
      menu.appendChild(b);
    }
    root.appendChild(menu);
    const off = () => {
      menu.remove();
      window.removeEventListener("mousedown", off, true);
    };
    setTimeout(() => window.addEventListener("mousedown", off, true), 0);
  }
}

main().catch((e) => {
  document.getElementById("app")!.textContent = String(e);
});
