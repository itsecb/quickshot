// Pinned image: always-on-top floating window. Drag anywhere to move, wheel for opacity,
// Ctrl+wheel to zoom, Esc or double-click to close, Ctrl+C copies, Ctrl+S saves.
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

  // The window itself is transparent, so fading the image shows what is underneath. The
  // outline, menu and messages stay solid, and the 10% floor keeps the pin findable.
  let opacity = 1;
  function applyOpacity(next: number) {
    opacity = Math.min(1, Math.max(0.1, next));
    img.style.opacity = String(opacity);
    flash(`Opacity ${Math.round(opacity * 100)}%`);
  }

  root.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      if (e.ctrlKey || e.metaKey) {
        scale = Math.min(6, Math.max(0.1, scale * Math.exp(-e.deltaY * 0.0015)));
        void applyScale();
        return;
      }
      // one notch = 5%; wheel up = more solid, wheel down = more see-through
      const notches = Math.max(-4, Math.min(4, -e.deltaY / 100));
      applyOpacity(Math.round((opacity + notches * 0.05) * 100) / 100);
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
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      applyOpacity(opacity + (e.key === "ArrowUp" ? 0.1 : -0.1));
    }
  });

  // One reusable flash so fast wheel spins update the text instead of stacking messages.
  const flashEl = document.createElement("div");
  flashEl.className = "flash";
  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  function flash(text: string) {
    flashEl.textContent = text;
    root.appendChild(flashEl);
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => flashEl.remove(), 1500);
  }

  function showMenu(x: number, y: number) {
    document.querySelector(".menu")?.remove();
    const menu = document.createElement("div");
    menu.className = "menu";
    menu.style.left = `${Math.min(x, window.innerWidth - 150)}px`;
    menu.style.top = `${Math.min(y, window.innerHeight - 180)}px`;
    const items: [string, () => void | Promise<void>][] = [
      ["Copy image", async () => (await copyCapture(info.id), flash("Copied"))],
      ["Save to folder", async () => flash(`Saved ${await saveCapture(info.id)}`)],
      ["Reset size", () => ((scale = 1), applyScale())],
      ["Opacity 100%", () => applyOpacity(1)],
      ["Opacity 50%", () => applyOpacity(0.5)],
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
