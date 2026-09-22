// Delayed-capture countdown. Rust owns the timing and closes this window just before the
// screen is frozen; this page only shows the seconds left and offers a cancel button.
import { invoke } from "@tauri-apps/api/core";
import "./countdown.css";

declare global {
  interface Window {
    __QS_COUNTDOWN?: number;
  }
}

const total = window.__QS_COUNTDOWN ?? 3;
const started = performance.now();

const root = document.getElementById("app")!;
root.innerHTML = `
  <div class="pill">
    <span class="num"></span>
    <span class="label">Capturing…</span>
    <button class="cancel" title="Cancel (or press the hotkey again)">✕</button>
  </div>`;
const num = root.querySelector<HTMLElement>(".num")!;
root.querySelector<HTMLButtonElement>(".cancel")!.onclick = () => void invoke("cancel_countdown");

function tick() {
  const left = Math.max(0, total - (performance.now() - started) / 1000);
  num.textContent = String(Math.ceil(left));
  if (left > 0) requestAnimationFrame(tick);
}
tick();
