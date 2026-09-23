// Windows are created hidden and shown by their page once it has painted, so they never flash
// white while loading. Rust shows them anyway after a short delay if this never runs.
import { getCurrentWindow } from "@tauri-apps/api/window";

export function showWhenReady({ focus = true } = {}) {
  // two frames: the first lays out, the second is on screen
  requestAnimationFrame(() =>
    requestAnimationFrame(async () => {
      const win = getCurrentWindow();
      try {
        await win.show();
        if (focus) await win.setFocus();
      } catch (e) {
        console.warn("show failed", e);
      }
    }),
  );
}
